/* SPDX-License-Identifier: GPL-2.0-or-later
 * Entry point of the Doom port (called by _c_int00 of rts55x.lib) */
#include "zen_types.h"
#include "doomgeneric.h"

/* The full-screen patches (title, help, credits, intermission map: 68168 bytes each) do not fit
 * one 64K-word data page, so the game starts directly in E1M1 instead of the title screen. */
static char *args[] = { "doom", "-warp", "1", "1", "-skill", "3" };

int main(void)
{
    doomgeneric_Create(sizeof(args) / sizeof(args[0]), args);
    for (;;) {
        doomgeneric_Tick();
    }
}
