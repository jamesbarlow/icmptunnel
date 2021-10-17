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

#include <netinet/if_ether.h>

#include <getopt.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>

#include "config.h"
#include "client.h"
#include "server.h"
#include "options.h"
#include "forwarder.h"
#include "echo-skt.h"

/* default tunnel mtu in bytes; assume the size of an ethernet frame
 * minus ip, icmp and packet header sizes.
 */
#define ICMPTUNNEL_MTU (1500 - (int)sizeof(struct echo_buf))

#ifndef ETH_MIN_MTU
#define ETH_MIN_MTU 68
#endif
#ifndef ETH_MAX_MTU
#define ETH_MAX_MTU 0xFFFFU
#endif

static void version()
{
    fprintf(stderr, "icmptunnel is version %s (built %s).\n", ICMPTUNNEL_VERSION, __DATE__);
    exit(0);
}

static void help(const char *program)
{
    fprintf(stderr,
"icmptunnel %s.\n"
"usage: %s [options] -s|server\n\n"
"  -v               print version and exit.\n"
"  -h               print help and exit.\n"
"  -u <user>        user to switch after opening tun device and socket.\n"
"                   the default user is %s.\n"
"  -k <interval>    interval between keep-alive packets.\n"
"                   the default interval is %i seconds.\n"
"  -r <retries>     packet retry limit before timing out.\n"
"                   the default is %i retries.\n"
"  -m <mtu>         max frame size of the tunnel interface.\n"
"                   the default tunnel mtu is %i bytes.\n"
"  -e               emulate the microsoft ping utility.\n"
"                   will be negotiated with peer via protocol, default is off.\n"
"  -d               run in the background as a daemon.\n"
"  -s               run in server-mode.\n"
"  -t <hops>        use ttl security mode.\n"
"                   the default is to not use this mode.\n"
"  -i <id>          set instance id used in ICMP request/reply id field.\n"
"                   the default is to use generated on startup.\n"
"  server           run in client-mode, using the server ip/hostname.\n"
"\n"
"Note that process requires CAP_NET_RAW to open ICMP raw sockets\n"
"and CAP_NET_ADMIN to manage tun devices. You should run either\n"
"as root or grant above capabilities (e.g. via POSIX file capabilities)\n"
"\n",
            ICMPTUNNEL_VERSION, program, ICMPTUNNEL_USER,
            ICMPTUNNEL_TIMEOUT, ICMPTUNNEL_RETRIES, ICMPTUNNEL_MTU
    );
    exit(0);
}

static void fatal(const char *fmt, ...)
{
    va_list ap;

    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);

    exit(1);
}

static void usage(const char *program)
{
    fatal("use %s -h for more information.\n", program);
}

static void optrange(char c, const char *optname,
                     unsigned int min, unsigned int max)
{
    fatal("for -%c option <%s> must be within %u ... %u range.\n",
          c, optname, min, max);
}

static void signalhandler(int sig)
{
    /* unused variable. */
    (void)sig;

    stop();
}

static unsigned int nr_keepalives(const char *s)
{
    const unsigned int poll_secs = ICMPTUNNEL_PUNCHTHRU_INTERVAL;
    const unsigned int max_secs = 30;
    unsigned int k = atoi(s);

    /* use default keepalive interval if
     *  1) keepalive interval isn't specified (i.e. 0)
     *  2) too long that state entry may timeout on firewall.
     */
    if (!k || k > max_secs)
        optrange('k', "interval", 1, max_secs);

    /* compiler shall optimize this. */
    return k / poll_secs + (poll_secs > 1) * (k % poll_secs >= poll_secs / 2);
}

static unsigned int nr_retries(const char *s)
{
    const unsigned int max_retries = 4 * ICMPTUNNEL_RETRIES;
    unsigned int r = strcmp(optarg, "infinite") ? atoi(s) : 0;

    /* use default retries number if not infinite (i.e. 0) and
     * 4 times greather than default retry numbers.
     */
    if (r && r > max_retries)
        optrange('r', "retries", 0, max_retries);

    return r;
}

struct options opts = {
    ICMPTUNNEL_USER,
    ICMPTUNNEL_TIMEOUT,
    ICMPTUNNEL_RETRIES,
    ICMPTUNNEL_MTU,
    ICMPTUNNEL_EMULATION,
    ICMPTUNNEL_DAEMON,
    255,
    UINT16_MAX + 1,
};

int main(int argc, char *argv[])
{
    char *program = argv[0];
    char *hostname = NULL;
    int servermode = 0;

    /* parse the option arguments. */
    opterr = 0;
    int opt;
    while ((opt = getopt(argc, argv, "vhu:k:r:m:edst:i:")) != -1) {
        switch (opt) {
        case 'v':
            version();
            break;
        case 'h':
            help(program);
            break;
        case 'u':
            opts.user = optarg;
            break;
        case 'k':
            opts.keepalive = nr_keepalives(optarg);
            break;
        case 'r':
            opts.retries = nr_retries(optarg);
            break;
        case 'm':
            opts.mtu = atoi(optarg);
            if (opts.mtu < ETH_MIN_MTU || opts.mtu > ETH_MAX_MTU)
                optrange('m', "mtu", ETH_MIN_MTU, ETH_MAX_MTU);
            break;
        case 'e':
            opts.emulation = 1;
            break;
        case 'd':
            opts.daemon = 1;
            break;
        case 's':
            servermode = 1;
            break;
        case 't':
            opts.ttl = atoi(optarg);
            if (opts.ttl > 254)
                optrange('t', "hops", 0, 254);
            break;
        case 'i':
            opts.id = atoi(optarg);
            if (opts.id > UINT16_MAX)
                optrange('i', "id", 0, UINT16_MAX);
            break;
        case '?':
            /* fall-through. */
        default:
            fprintf(stderr, "unknown or missing option -- '%c'\n", optopt);
            usage(program);
            break;
        }
    }

    argc -= optind;
    argv += optind;

    /* if we're running in client mode, parse the server hostname. */
    if (!servermode) {
        if (argc < 1) {
            fprintf(stderr, "missing server ip/hostname.\n");
            usage(program);
        }
        hostname = argv[0];

        argc--;
        argv++;
    }

    /* check for extraneous options. */
    if (argc > 0) {
        fprintf(stderr, "unknown option -- '%s'\n", argv[0]);
        usage(program);
    }

    /* check for non-empty user. */
    if (!*opts.user)
        opts.user = ICMPTUNNEL_USER;

    /* register the signal handlers. */
    signal(SIGINT, signalhandler);
    signal(SIGTERM, signalhandler);

    srand(getpid() + (time(NULL) % getppid()));

    if (servermode) {
        /* run the server. */
        return server();
    } else {
        /* run the client. */
        return client(hostname);
    }
}
