/* SPDX-License-Identifier: GPL-2.0-or-later
 * Force-included before every Doom source (cl55 --preinclude).
 * The Doom sources assume a 32-bit int. On the C55x int is 16 bits and char is 16 bits too, so
 * after all system headers are read (they must see the real int) int is redefined as long.
 * The C library prototypes were parsed before, so calls to them keep their 16-bit int. */
#ifndef ZEN_TYPES_H
#define ZEN_TYPES_H

#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdarg.h>
#include <ctype.h>
#include <limits.h>
#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <errno.h>
#include <math.h>

/* TI's stdint.h has no 8-bit types: char is 16 bits; byte values are kept below 256 */
typedef unsigned char uint8_t;
typedef signed char int8_t;

#define int long
/* A short operand is promoted to the compiler's own 16-bit int, so `short << 16` or
 * `short * short` would overflow where Doom expects 32 bits: short is long as well (use
 * int16_t / uint16_t, typedefs made above, where 16 bits are meant) */
#define short long

/* int is 32 bits for Doom: limits and abs() follow */
#undef INT_MAX
#undef INT_MIN
#undef UINT_MAX
#define INT_MAX 2147483647L
#define INT_MIN (-2147483647L - 1L)
#define UINT_MAX 4294967295UL
#undef abs
#define abs labs

/* c_int00 clears the .bss and runs .cinit: no argc/argv, no files, no network */
int zen_printf(const char *fmt, ...);
int zen_sprintf(char *out, const char *fmt, ...);
int zen_snprintf(char *out, size_t n, const char *fmt, ...);
int zen_vsnprintf(char *out, size_t n, const char *fmt, va_list ap);
char *zen_strdup(const char *s);
#define strdup zen_strdup
int zen_strcasecmp(const char *a, const char *b);
int zen_strncasecmp(const char *a, const char *b, size_t n);
#define printf zen_printf
#define sprintf zen_sprintf
#define snprintf zen_snprintf
#define vsnprintf zen_vsnprintf
#define strcasecmp zen_strcasecmp
#define strncasecmp zen_strncasecmp


/* no files, no console: the stubs in zen_libc.c fail or discard */
#undef stderr
#undef stdout
#undef stdin
#define stderr ((FILE *) 2)
#define stdout ((FILE *) 1)
#define stdin ((FILE *) 0)
#define EISDIR 21
FILE *zen_fopen(const char *name, const char *mode);
int zen_fclose(FILE *f);
size_t zen_fread(void *p, size_t size, size_t n, FILE *f);
size_t zen_fwrite(const void *p, size_t size, size_t n, FILE *f);
int zen_fseek(FILE *f, long off, int whence);
long zen_ftell(FILE *f);
int zen_fflush(FILE *f);
int zen_fprintf(FILE *f, const char *fmt, ...);
int zen_vfprintf(FILE *f, const char *fmt, va_list ap);
int zen_puts(const char *s);
int zen_putchar(int c);
int zen_remove(const char *name);
int zen_rename(const char *a, const char *b);
int zen_system(const char *cmd);
char *zen_getenv(const char *name);
int zen_sscanf(const char *s, const char *fmt, ...);
void zen_exit(int code);
void zen_abort(void);
int zen_isatty(int fd);
long zen_atoi(const char *s);
#define fopen zen_fopen
#define fclose zen_fclose
#define fread zen_fread
#define fwrite zen_fwrite
#define fseek zen_fseek
#define ftell zen_ftell
#define fflush zen_fflush
#define fprintf zen_fprintf
#define vfprintf zen_vfprintf
#define puts zen_puts
#define putchar zen_putchar
#define remove zen_remove
#define rename zen_rename
#define system zen_system
#define getenv zen_getenv
#define sscanf zen_sscanf
#define exit zen_exit
#define abort zen_abort
#define isatty zen_isatty
#define atoi zen_atoi
#define mkdir(path, mode) (-1)

#endif
