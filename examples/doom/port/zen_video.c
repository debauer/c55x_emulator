/* SPDX-License-Identifier: GPL-2.0-or-later
 * Video layer of the Doom port (replaces i_video.c): the 8-bit 320x200 screen is reduced to
 * 160x100, dithered to 1 bit and written to the LCD (13 pages of 160 columns, bit 0 = top
 * row of a page). The screen is centred: 2 blank rows above and below. */
#include "zen_types.h"
#include "doomtype.h"
#include "doomkeys.h"
#include "doomgeneric.h"
#include "i_system.h"
#include "i_video.h"
#include "v_video.h"
#include "z_zone.h"
#include "zen_hw_if.h"

void I_GetEvent(void);                          /* i_input.c */

#define OUT_W 160
#define OUT_H 100
#define OUT_Y0 2
#define PAGES 13

byte *I_VideoBuffer = NULL;
boolean screensaver_mode = false;
boolean screenvisible = true;
float mouse_acceleration = 2.0;
int mouse_threshold = 10;
int usegamma = 0;
int usemouse = 0;
int fullscreen = 0;
int aspect_ratio_correct = 0;

static uint16_t lum[256];                       /* brightness 0..255 per palette entry */
static uint16_t page_cols[PAGES][OUT_W];

/* 4x4 Bayer thresholds for the sum of two brightnesses (0..510) */
static const uint16_t bayer[4][4] = {
    {  16, 272,  80, 336 }, { 400, 144, 464, 208 },
    { 112, 368,  48, 304 }, { 496, 240, 432, 176 },
};

void I_InitGraphics(void)
{
    I_VideoBuffer = (byte *) Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, NULL);
    screenvisible = true;
}

void I_ShutdownGraphics(void) {}
void I_StartFrame(void) {}
void I_UpdateNoBlit(void) {}
void I_BeginRead(void) {}
void I_EndRead(void) {}
void I_EnableLoadingDisk(void) {}
void I_CheckIsScreensaver(void) {}
void I_GraphicsCheckCommandLine(void) {}
void I_SetGrabMouseCallback(grabmouse_callback_t func) { (void) func; }
void I_DisplayFPSDots(boolean dots_on) { (void) dots_on; }
void I_BindVideoVariables(void) {}
void I_InitWindowTitle(void) {}
void I_InitWindowIcon(void) {}
void I_SetWindowTitle(char *title) { (void) title; }

void I_ReadScreen(byte *scr)
{
    memcpy(scr, I_VideoBuffer, SCREENWIDTH * SCREENHEIGHT);
}

/* brightness of a palette entry: gamma 2 and some gain, so that the dark Doom scenes keep detail on a 1-bit LCD */
static uint16_t brightness(long r, long g, long b)
{
    long y = (77L * r + 151L * g + 28L * b) >> 8;
    long v = 0, bit;

    y *= 255L;                                  /* v = sqrt(y_linear * 255) */
    for (bit = 128L; bit > 0; bit >>= 1) {
        if ((v + bit) * (v + bit) <= y) {
            v += bit;
        }
    }
    v = v * 3 / 2;                              /* Doom scenes are dark: a little gain */
    return (uint16_t) (v > 255 ? 255 : v);
}

void I_SetPalette(byte *palette)
{
    int i;

    for (i = 0; i < 256; i++, palette += 3) {
        lum[i] = brightness(palette[0], palette[1], palette[2]);
    }
}

int I_GetPaletteIndex(int r, int g, int b)
{
    int best = 0, i;
    long bestdiff = 0x7FFFFFFFL, want = brightness(r, g, b);

    for (i = 0; i < 256; i++) {
        long d = lum[i] - want;

        if (d < 0) {
            d = -d;
        }
        if (d < bestdiff) {
            bestdiff = d;
            best = i;
        }
    }
    return best;
}

void I_StartTic(void)
{
    I_GetEvent();
}

void I_FinishUpdate(void)
{
    const byte *in = I_VideoBuffer;
    int16_t y, x, page;

    memset(page_cols, 0, sizeof(page_cols));
    for (y = 0; y < OUT_H; y++, in += 2 * SCREENWIDTH) {
        const byte *row2 = in + SCREENWIDTH;
        uint16_t *cols = page_cols[(y + OUT_Y0) >> 3];
        uint16_t bit = 1u << ((y + OUT_Y0) & 7);
        const uint16_t *thr = bayer[y & 3];

        for (x = 0; x < OUT_W; x++) {
            if ((uint16_t) (lum[in[2 * x]] + lum[row2[2 * x]]) < thr[x & 3]) {
                cols[x] |= bit;                 /* dark pixel: LCD bit set */
            }
        }
    }
    for (page = 0; page < PAGES; page++) {
        zen_hw_lcd_page(page, page_cols[page]);
    }
}
