/* SPDX-License-Identifier: GPL-2.0-or-later
 * Platform layer of the Doom port: timer, MCU keys, LCD output. Hardware access through the
 * helpers shared with the other examples (common/{lcd,i2c,mcu,irq}.c). */
#include "zen_types.h"
#include "doomgeneric.h"
#include "doomkeys.h"
#include "zen_hw_if.h"

/* MCU events: class << 4 | detail (controls.py) */
#define EV_PRESS   1
#define EV_RELEASE 3
#define EV_SCROLL_DOWN 7
#define EV_SCROLL_UP   9

#define KEYQ 16
static struct { unsigned char key; unsigned char pressed; } keyq[KEYQ];
static unsigned int keyq_head, keyq_tail;

static void key_push(unsigned char key, unsigned char pressed)
{
    unsigned int next = (keyq_head + 1) % KEYQ;

    if (next != keyq_tail) {
        keyq[keyq_head].key = key;
        keyq[keyq_head].pressed = pressed;
        keyq_head = next;
    }
}

/* button detail -> Doom key */
static unsigned char key_for_detail(unsigned int detail)
{
    switch (detail) {
    case 2:  return KEY_UPARROW;        /* play */
    case 13: return KEY_DOWNARROW;      /* random */
    case 4:  return KEY_LEFTARROW;      /* previous */
    case 5:  return KEY_RIGHTARROW;     /* next */
    case 1:  return KEY_FIRE;           /* ok */
    case 11: return KEY_ESCAPE;         /* menu */
    case 8:  return KEY_USE;            /* back */
    case 6:  return KEY_ENTER;          /* volume + */
    case 7:  return KEY_TAB;            /* volume - (automap) */
    default: return 0;
    }
}

static void mcu_poll(void)
{
    long ev;

    while ((ev = zen_hw_event()) >= 0) {
        unsigned int cls = ev >> 4, detail = ev & 15;
        unsigned char key;

        if (cls == EV_PRESS || cls == EV_RELEASE) {
            key = key_for_detail(detail);
            if (key != 0) {
                key_push(key, cls == EV_PRESS);
            }
        } else if (cls == EV_SCROLL_UP || cls == EV_SCROLL_DOWN) {
            key = cls == EV_SCROLL_UP ? KEY_UPARROW : KEY_DOWNARROW;   /* touch pad: menu */
            key_push(key, 1);
            key_push(key, 0);
        }
    }
}

void DG_Init(void)
{
    zen_hw_init();
}

uint32_t DG_GetTicksMs(void)
{
    return zen_hw_ticks();
}

void DG_SleepMs(uint32_t ms)
{
    uint32_t end = zen_hw_ticks() + ms;

    while ((long) (end - zen_hw_ticks()) > 0) {
        zen_hw_idle();
    }
}

int DG_GetKey(int *pressed, unsigned char *key)
{
    mcu_poll();
    if (keyq_tail == keyq_head) {
        return 0;
    }
    *key = keyq[keyq_tail].key;
    *pressed = keyq[keyq_tail].pressed;
    keyq_tail = (keyq_tail + 1) % KEYQ;
    return 1;
}

void DG_SetWindowTitle(const char *title)
{
    (void) title;
}

void DG_DrawFrame(void)
{
}
