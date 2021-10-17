/*
 *  https://github.com/jamesbarlow/icmptunnel
 *
 *  The MIT License (MIT)
 *
 *  Copyright (c) 2016 James Barlow-Bignell
 *
 *  Permission is hereby granted, free of charge, to any person obtaining a copy
 *  of this software and associated documentation files (the "Software"), to deal
 *  in the Software without restriction, including without limitation the rights
 *  to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 *  copies of the Software, and to permit persons to whom the Software is
 *  furnished to do so, subject to the following conditions:
 *
 *  The above copyright notice and this permission notice shall be included in all
 *  copies or substantial portions of the Software.
 *
 *  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 *  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 *  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 *  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 *  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 *  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 *  SOFTWARE.
 */

#include <netinet/ip.h>
#include <netinet/ip_icmp.h>
#include <arpa/inet.h>

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "peer.h"
#include "options.h"
#include "echo-skt.h"
#include "tun-device.h"
#include "protocol.h"
#include "server-handlers.h"

static void opts_emulation(const struct peer *client)
{
    uint16_t sequence = client->skt.buf->icmph.un.echo.sequence;
    char ip[sizeof("255.255.255.255")];

    if (opts.emulation != 1)
        return;

    /* first data, keepalive or punchthru (client shouldn't send it) received
     * with unchanged sequence number meaning that client accepted emulation
     * option proposal in connection request: make option immutable.
     */
    opts.emulation = 2;

    if (client->nextseq == sequence)
        return;

    inet_ntop(AF_INET, &client->linkip, ip, sizeof(ip));
    fprintf(stderr, "turn off microsoft ping emulation mode for %s.\n", ip);

    opts.emulation = 0;
}

void handle_server_data(struct peer *client, int framesize)
{
    struct echo_skt *skt = &client->skt;
    struct tun_device *device = &client->device;

    /* determine the size of the encapsulated frame. */
    if (!framesize)
        return;

    /* write the frame to the tunnel interface. */
    write_tun_device(device, skt->buf->payload, framesize);

    /* save the icmp id and sequence numbers for any return traffic. */
    handle_punchthru(client);
}

void handle_keep_alive_request(struct peer *client)
{
    struct echo_skt *skt = &client->skt;

    /* write a keep-alive response. */
    struct packet_header *pkth = &skt->buf->pkth;
    memcpy(pkth->magic, PACKET_MAGIC_SERVER, sizeof(pkth->magic));
    pkth->flags = 0;
    pkth->type = PACKET_KEEP_ALIVE;

    /* send the response to the client. */
    send_echo(skt, client->linkip, 0);

    opts_emulation(client);

    client->seconds = 0;
    client->timeouts = 0;
}

void handle_connection_request(struct peer *client)
{
    struct echo_skt *skt = &client->skt;
    uint32_t sourceip = skt->buf->iph.saddr;
    uint32_t id = skt->buf->icmph.un.echo.id;
    char *verdict, ip[sizeof("255.255.255.255")];

    struct packet_header *pkth = &skt->buf->pkth;
    memcpy(pkth->magic, PACKET_MAGIC_SERVER, sizeof(pkth->magic));
    pkth->flags = 0;

    inet_ntop(AF_INET, &sourceip, ip, sizeof(ip));

    /* is a client already connected? */
    if (client->linkip && client->linkip != sourceip) {
        pkth->type = PACKET_SERVER_FULL;
        verdict = "ignoring";
    } else {
        pkth->type = PACKET_CONNECTION_ACCEPT;
        verdict = "accepting";

        if (pkth->flags & PACKET_F_ICMP_SEQ_EMULATION) {
            /* client requested: cannot be turned off. */
            opts.emulation = 2;
        } else if (opts.emulation) {
            /* server requested via command line option: can be turned off. */
            fprintf(stderr, "request microsoft ping emulation on %s.\n", ip);
        }

        if (opts.emulation)
            pkth->flags |= PACKET_F_ICMP_SEQ_EMULATION;

        /* store the id number. */
        if (!client->strict_nextid)
            client->nextid = id;

        client->seconds = 0;
        client->timeouts = 0;

        /* better to start with used sequence number until punchthru. */
        client->nextseq = skt->buf->icmph.un.echo.sequence;
        client->punchthru_idx = 0;
        client->punchthru_write_idx = 0;
        client->linkip = sourceip;
    }

    fprintf(stderr, "%s connection from %s with id %d\n",
            verdict, ip, ntohs(id));

    /* do not respond to non-client IPs to hide from probes. */
    if (client->strict_nextid && client->linkip != sourceip)
        return;

    /* send the response. */
    send_echo(skt, sourceip, 0);
}

/* handle a punch-thru packet. */
void handle_punchthru(struct peer *client)
{
    opts_emulation(client);

    if (!opts.emulation) {
        /* store the sequence number. */
        client->punchthru[client->punchthru_write_idx++] =
            client->skt.buf->icmph.un.echo.sequence;
        client->punchthru_write_idx %= ICMPTUNNEL_PUNCHTHRU_WINDOW;
    }

    client->seconds = 0;
    client->timeouts = 0;
}
