/* SPDX-License-Identifier: GPL-2.0-or-later
 * C library parts of the Doom port: formatted output with 32-bit int (Doom code is compiled with
 * int = long, see zen_types.h) and stubs for everything that needs an operating system. */
#include "zen_types.h"

/* last output of printf/fprintf (messages, I_Error): a ring the debugger can read */
#define LOG_SIZE 512
char zen_log[LOG_SIZE];
unsigned int zen_log_pos;
volatile long zen_exit_code;

static void log_put(char c)
{
    zen_log[zen_log_pos] = c;
    zen_log_pos = (zen_log_pos + 1) % LOG_SIZE;
}

static void log_text(const char *s, int n)
{
    int i;

    for (i = 0; i < n; i++) {
        log_put(s[i]);
    }
}

/* %d %i %u %x %X %c %s %% with flags - 0, width and precision; int arguments are long */
int zen_vsnprintf(char *out, size_t cap, const char *fmt, va_list ap)
{
    size_t len = 0;

    while (*fmt != 0) {
        char buf[16];
        const char *text;
        int width = 0, prec = -1, left = 0, zero = 0, n, pad;
        char conv, sign = 0;

        if (*fmt != '%') {
            if (len + 1 < cap) {
                out[len] = *fmt;
            }
            len++;
            fmt++;
            continue;
        }
        fmt++;
        for (;; fmt++) {
            if (*fmt == '-') {
                left = 1;
            } else if (*fmt == '0') {
                zero = 1;
            } else {
                break;
            }
        }
        while (*fmt >= '0' && *fmt <= '9') {
            width = width * 10 + (*fmt++ - '0');
        }
        if (*fmt == '.') {
            fmt++;
            prec = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                prec = prec * 10 + (*fmt++ - '0');
            }
        }
        while (*fmt == 'l') {
            fmt++;
        }
        conv = *fmt++;
        if (conv == 's') {
            text = va_arg(ap, const char *);
            if (text == 0) {
                text = "(null)";
            }
            n = 0;
            while (text[n] != 0 && (prec < 0 || n < prec)) {
                n++;
            }
        } else if (conv == 'c') {
            buf[0] = (char) va_arg(ap, long);
            text = buf;
            n = 1;
        } else if (conv == 'd' || conv == 'i' || conv == 'u' || conv == 'x' || conv == 'X') {
            long v = va_arg(ap, long);
            unsigned long u = (unsigned long) v;
            unsigned long base = (conv == 'x' || conv == 'X') ? 16UL : 10UL;
            const char *digits = conv == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
            int i = 16;

            if ((conv == 'd' || conv == 'i') && v < 0) {
                sign = '-';
                u = -u;
            }
            do {
                buf[--i] = digits[(int) (u % base)];
                u /= base;
            } while (u != 0);
            while (16 - i < prec) {
                buf[--i] = '0';
            }
            text = buf + i;
            n = 16 - i;
        } else {
            if (conv == 0) {
                break;
            }
            buf[0] = conv;                  /* %% and unknown conversions */
            text = buf;
            n = 1;
        }
        pad = width - n - (sign != 0);
        if (!left && !zero) {
            while (pad-- > 0) {
                if (len + 1 < cap) out[len] = ' ';
                len++;
            }
        }
        if (sign != 0) {
            if (len + 1 < cap) out[len] = sign;
            len++;
        }
        if (!left && zero) {
            while (pad-- > 0) {
                if (len + 1 < cap) out[len] = '0';
                len++;
            }
        }
        while (n-- > 0) {
            if (len + 1 < cap) out[len] = *text;
            len++;
            text++;
        }
        if (left) {
            while (pad-- > 0) {
                if (len + 1 < cap) out[len] = ' ';
                len++;
            }
        }
    }
    if (cap > 0) {
        out[len < cap ? len : cap - 1] = 0;
    }
    return (int) len;
}

int zen_snprintf(char *out, size_t cap, const char *fmt, ...)
{
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zen_vsnprintf(out, cap, fmt, ap);
    va_end(ap);
    return n;
}

int zen_sprintf(char *out, const char *fmt, ...)
{
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zen_vsnprintf(out, 0x7FFFUL, fmt, ap);
    va_end(ap);
    return n;
}

int zen_vfprintf(FILE *f, const char *fmt, va_list ap)
{
    char line[256];
    int n = zen_vsnprintf(line, sizeof(line), fmt, ap);

    (void) f;
    log_text(line, n < 127 ? n : 127);
    return n;
}

int zen_fprintf(FILE *f, const char *fmt, ...)
{
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zen_vfprintf(f, fmt, ap);
    va_end(ap);
    return n;
}

int zen_printf(const char *fmt, ...)
{
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zen_vfprintf(stdout, fmt, ap);
    va_end(ap);
    return n;
}

int zen_puts(const char *s)
{
    log_text(s, (int) strlen(s));
    log_put('\n');
    return 0;
}

int zen_putchar(int c)
{
    log_put((char) c);
    return c;
}

/* no files: every open fails */
FILE *zen_fopen(const char *name, const char *mode) { (void) name; (void) mode; return 0; }
int zen_fclose(FILE *f) { (void) f; return -1; }
size_t zen_fread(void *p, size_t size, size_t n, FILE *f) { (void) p; (void) size; (void) n; (void) f; return 0; }
size_t zen_fwrite(const void *p, size_t size, size_t n, FILE *f) { (void) p; (void) size; (void) f; return n; }
int zen_fseek(FILE *f, long off, int whence) { (void) f; (void) off; (void) whence; return -1; }
long zen_ftell(FILE *f) { (void) f; return -1L; }
int zen_fflush(FILE *f) { (void) f; return 0; }
int zen_remove(const char *name) { (void) name; return -1; }
int zen_rename(const char *a, const char *b) { (void) a; (void) b; return -1; }
int zen_system(const char *cmd) { (void) cmd; return -1; }
char *zen_getenv(const char *name) { (void) name; return 0; }
int zen_sscanf(const char *s, const char *fmt, ...) { (void) s; (void) fmt; return 0; }
int zen_isatty(int fd) { (void) fd; return 0; }

void zen_exit(int code)
{
    zen_exit_code = code;
    for (;;) {
    }
}

void zen_abort(void)
{
    zen_exit(-1);
}

long zen_atoi(const char *s)
{
    long v = 0;
    int neg = 0;

    while (*s == ' ') {
        s++;
    }
    if (*s == '-') {
        neg = 1;
        s++;
    }
    while (*s >= '0' && *s <= '9') {
        v = v * 10 + (*s++ - '0');
    }
    return neg ? -v : v;
}

char *zen_strdup(const char *s)
{
    size_t n = strlen(s) + 1;
    char *p = malloc(n);

    if (p != 0) {
        memcpy(p, s, n);
    }
    return p;
}

static int fold(int c)
{
    return c >= 'A' && c <= 'Z' ? c + ('a' - 'A') : c;
}

int zen_strncasecmp(const char *a, const char *b, size_t n)
{
    for (; n > 0; n--, a++, b++) {
        int d = fold(*a) - fold(*b);

        if (d != 0 || *a == 0) {
            return d;
        }
    }
    return 0;
}

int zen_strcasecmp(const char *a, const char *b)
{
    return zen_strncasecmp(a, b, 0xFFFFUL);
}
