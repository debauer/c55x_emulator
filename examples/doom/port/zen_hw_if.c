/* SPDX-License-Identifier: GPL-2.0-or-later
 * Hardware interface of the Doom port, compiled with the native 16-bit int (no zen_types.h). */
#include "fw.h"
#include "zen_hw_if.h"

#define CLKMD_24MHZ   0x0110        /* PLL enable, multiplier 2 on 12 MHz */
#define TICK_PRD  1499              /* timer 0: 16 * 1500 clocks = 1 ms at 24 MHz */
#define TICK_TDDR 15

void zen_hw_init(void)
{
    IOREG(0x1C00) = CLKMD_24MHZ;
    emif_ce1_slow();
    lcd_init();
    lcd_on();
    irq_init();
    i2c_init();
    irq_enable(0, FW_INT1);
    mcu_cmd2(6, 9);
    mcu_cmd(33);
    mcu_cmd2(36, 3);
    timer0_start(TICK_PRD, TICK_TDDR);
    irq_enable(FW_TINT0, 0);
}

unsigned long zen_hw_ticks(void)
{
    static unsigned int last;
    static unsigned long total;
    unsigned int now = g_ticks;

    total += (unsigned int) (now - last);           /* 16-bit counter: call at least every 65 s */
    last = now;
    return total;
}

long zen_hw_event(void)
{
    static unsigned int seen_requests;
    static int more;
    unsigned int ev;

    if (!more) {
        if (g_mcu_requests == seen_requests) {
            return -1L;
        }
        seen_requests = g_mcu_requests;
        more = 1;
    }
    if (!mcu_event_read(&ev) || ev == 0) {
        more = 0;
        return -1L;
    }
    return ev;
}

void zen_hw_lcd_page(int16_t page, const uint16_t *columns)
{
    lcd_page(page, (const unsigned int *) columns);
}

/* wait for the next interrupt (timer 0, MCU) with the CPU domain idle */
void zen_hw_idle(void)
{
    IOREG(0x0001) = 1;                  /* ICR: CPUI */
    asm(" IDLE");
}
