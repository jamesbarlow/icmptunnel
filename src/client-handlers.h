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

#ifndef ICMPTUNNEL_CLIENT_HANDLERS_H
#define ICMPTUNNEL_CLIENT_HANDLERS_H

#include "options.h"

struct peer;

/* handle a data packet. */
void handle_client_data(struct peer *server, int framesize);

/* handle a keep-alive packet. */
void handle_keep_alive_response(struct peer *server);

/* handle a connection accept packet. */
void handle_connection_accept(struct peer *server);

/* handle a server full packet. */
void handle_server_full(struct peer *server);

/* send a message to the server. */
int send_message(struct peer *server, int pkttype, int flags, int size);

/* send a connection request to the server. */
void send_connection_request(struct peer *server);

/* send a punchthru packet. */
static inline void send_punchthru(struct peer *server)
{
    if (!opts.emulation)
        send_message(server, PACKET_PUNCHTHRU, 0, 0);
}

/* send a keep-alive request to the server. */
static inline void send_keep_alive(struct peer *server)
{
    send_message(server, PACKET_KEEP_ALIVE, 0, 0);
}

#endif
