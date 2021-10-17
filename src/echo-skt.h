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

#ifndef ICMPTUNNEL_ECHOSKT_H
#define ICMPTUNNEL_ECHOSKT_H

#include <netinet/ip.h>
#include <netinet/ip_icmp.h>

#include <stdint.h>

#include "protocol.h"

struct echo_buf
{
    struct iphdr iph;
    struct icmphdr icmph;
    struct packet_header pkth;
    uint8_t payload[];
} __attribute__((packed));

struct echo_skt
{
    int fd;

    unsigned int ttl:8;
    unsigned int client:1;
    unsigned int filter:1;

    unsigned int bufsize:16;
    struct echo_buf *buf;
};

/* open an icmp echo socket. */
int open_echo_skt(struct echo_skt *skt, int mtu, int ttl, int client);

/* send an echo packet. */
int send_echo(struct echo_skt *skt, uint32_t targetip, int size);

/* receive an echo packet. */
int receive_echo(struct echo_skt *skt);

/* close the socket. */
void close_echo_skt(struct echo_skt *skt);

#endif
