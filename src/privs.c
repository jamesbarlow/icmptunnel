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

#include <sys/types.h>
#include <pwd.h>
#include <grp.h>
#include <unistd.h>

int drop_privs(const char *user)
{
    /* hope it is found in libc, not in POSIX. */
    extern int setgroups(size_t n, const gid_t *list);

    struct passwd *pw;
    struct group *gr;

    if (!user || !*user)
        return 0;

    /* user */
    pw = getpwnam(user);
    if (!pw)
        return -1;

    /* group */
    gr = getgrgid(pw->pw_gid);
    if (!gr)
        return -1;

    /* main group */
    if (setgid(pw->pw_gid) < 0)
        return -1;

    /* supplementary group */
    if (setgroups(1, &pw->pw_gid) < 0)
        return -1;

    /* user */
    if (setuid(pw->pw_uid) < 0)
        return -1;

    return 0;
}
