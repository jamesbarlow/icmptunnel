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

#include <arpa/inet.h>

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "config.h"
#include "options.h"
#include "client.h"
#include "peer.h"
#include "resolve.h"
#include "privs.h"
#include "protocol.h"
#include "echo-skt.h"
#include "tun-device.h"
#include "handlers.h"
#include "forwarder.h"
#include "client-handlers.h"

static void handle_icmp_packet(struct peer *server)
{
    struct echo_skt *skt = &server->skt;
    int size;

    /* receive the packet. */
    if ((size = receive_echo(skt)) < 0)
        return;

    /* we're only expecting packets from the server ... */
    if (server->linkip != skt->buf->iph.saddr)
        return;

    /* ... and with our id that is used to connect to the server. */
    if (server->nextid != skt->buf->icmph.un.echo.id)
        return;

    /* check the header magic. */
    const struct packet_header *pkth = &skt->buf->pkth;

    if (memcmp(pkth->magic, PACKET_MAGIC_SERVER, sizeof(pkth->magic)))
        return;

    switch (pkth->type) {
    case PACKET_DATA:
        /* handle a data packet. */
        handle_client_data(server, size);
        break;

    case PACKET_KEEP_ALIVE:
        /* handle a keep-alive packet. */
        handle_keep_alive_response(server);
        break;

    case PACKET_CONNECTION_ACCEPT:
        /* handle a connection accept packet. */
        handle_connection_accept(server);
        break;

    case PACKET_SERVER_FULL:
        /* handle a server full packet. */
        handle_server_full(server);
        break;
    }
}

static void handle_tunnel_data(struct peer *server)
{
    struct echo_skt *skt = &server->skt;
    struct tun_device *device = &server->device;
    int framesize;

    /* read the frame. */
    if ((framesize = read_tun_device(device, skt->buf->payload)) <= 0)
        return;

    /* if we're not connected then drop the frame. */
    if (!server->connected)
        return;

    /* write a data packet. */
    if (send_message(server, PACKET_DATA, 0, framesize) < 0)
        return;

    if (device->iopkts > 0)
        device->iopkts--;
}

static void handle_timeout(struct peer *server)
{
    /* send a punch-thru packet. */
    if (server->connected) {
        send_punchthru(server);

        if (server->device.iopkts > 0)
            server->device.iopkts--;
    }

    /* has the peer timeout elapsed? */
    if (++server->seconds == opts.keepalive) {
        unsigned int retries =
            opts.retries ? opts.retries : ICMPTUNNEL_RETRIES;

        server->seconds = 0;

        /* have we reached the max number of retries? */
        if (++server->timeouts == retries) {
            fprintf(stderr, "connection timed out.\n");

            server->connected = 0;
            server->timeouts = 0;

            if (opts.retries) {
                /* stop the packet forwarding loop. */
                stop();
                return;
            }
        }

        if (server->connected) {
            /* otherwise, send a keep-alive request. */
            send_keep_alive(server);
        } else {
            /* if we're still connecting, resend the connection request. */
            send_connection_request(server);
        }
    }
}

static const struct handlers handlers = {
    handle_icmp_packet,
    handle_tunnel_data,
    handle_timeout,
};

int client(const char *hostname)
{
    struct peer server;
    struct echo_skt *skt = &server.skt;
    struct tun_device *device = &server.device;
    int ret = 1;

    /* resolve the server hostname. */
    if (resolve(hostname, &server.linkip) < 0)
        goto err_out;

    /* open an echo socket. */
    if (open_echo_skt(skt, opts.mtu, opts.ttl, 1) < 0)
        goto err_out;

    /* open a tunnel interface. */
    if (open_tun_device(device, opts.mtu) < 0)
        goto err_close_skt;

    /* drop privileges. */
    if (drop_privs(opts.user) < 0)
        goto err_close_tun;

    /* choose initial icmp id and sequence numbers. */
    server.nextid = htons(opts.id > UINT16_MAX ? (uint32_t)rand() : opts.id);
    server.nextseq = htons(rand());

    /* mark as not connected to server. */
    server.connected = 0;

    /* initialize keepalive seconds and timeout retries. */
    server.seconds = 0;
    server.timeouts = 0;

    /* send the initial connection request. */
    send_connection_request(&server);

    /* run the packet forwarding loop. */
    ret = forward(&server, &handlers) < 0;

err_close_tun:
    close_tun_device(device);
err_close_skt:
    close_echo_skt(skt);
err_out:
    return ret;
}
