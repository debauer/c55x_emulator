/* SPDX-License-Identifier: GPL-2.0-or-later
 * Hardware interface of the Doom port. Declared with explicit widths only (intN_t, long), so the
 * Doom sources (int = long, zen_types.h) and the hardware helpers (int = 16 bits) agree. */
#ifndef ZEN_HW_IF_H
#define ZEN_HW_IF_H

#include <stdint.h>

void zen_hw_init(void);                                         /* clock, LCD, MCU, timer 0 */
unsigned long zen_hw_ticks(void);                               /* milliseconds */
void zen_hw_idle(void);                                         /* sleep until an interrupt */
long zen_hw_event(void);                                        /* next MCU event byte, -1: none */
void zen_hw_lcd_page(int16_t page, const uint16_t *columns);  /* 160 column bytes */

#endif
