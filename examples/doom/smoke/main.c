/* Smoke test of the Doom build: rts55x.lib start-up (cinit), string functions, 32-bit math,
 * initialised data. Result on the LCD: "OK" lines or the failing check. */
#include <string.h>
#include <stdlib.h>
#include "fw.h"

static long table[4] = { 100000L, -7L, 65536L * 3, 12345678L };
static char greeting[] = "HELLO";
static char copy[8];

int main(void)
{
    long a = table[0] * 3, b = table[3] / table[1];
    char *p = malloc(10);

    emif_ce1_slow();
    lcd_init();
    lcd_on();
    memcpy(copy, greeting, 6);
    lcd_text(0, strlen(copy) == 5 ? "STR OK" : "STR BAD");
    lcd_text(1, a == 300000L ? "MUL OK" : "MUL BAD");
    lcd_text(2, b == -1763668L ? "DIV OK" : "DIV BAD");
    lcd_text(3, table[2] == 196608L ? "DATA OK" : "DATA BAD");
    lcd_text(4, p != 0 ? "MALLOC OK" : "MALLOC BAD");
    lcd_text(5, sizeof(int) == 1 && sizeof(long) == 2 ? "SIZES OK" : "SIZES BAD");
    for (;;) {
    }
}
