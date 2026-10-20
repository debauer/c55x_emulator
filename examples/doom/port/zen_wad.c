/* SPDX-License-Identifier: GPL-2.0-or-later
 * WAD access of the Doom port.
 * The WAD file is loaded into RAM by the emulator (--load): 2 bytes per word, first byte in
 * the high half (the C55x stores words big-endian). Lumps are read into the Doom zone one
 * byte per word, because char is 16 bits wide. Lumps with multi-byte fields are then
 * converted to the compiler's own struct layout (zen_convert_lump), so the Doom code that
 * lays patch_t, mapvertex_t ... over a lump keeps working.
 * Also here: the IWAD lookup (the one WAD is always "doom1.wad"). */
#include "zen_types.h"
#include "doomtype.h"
#include "d_iwad.h"
#include "w_file.h"
#include "zen_wad.h"

static const uint16_t *wad_words = (const uint16_t *) ZEN_WAD_WORD_ADDR;

/* bytes pos .. pos + n of the WAD, one per destination word */
long zen_wad_read(long pos, byte *dest, long n)
{
    long i = 0;

    if ((pos & 1) != 0 && n > 0) {
        dest[i++] = wad_words[pos >> 1] & 0xFF;
        pos++;
    }
    for (; i + 1 < n; i += 2, pos += 2) {
        uint16_t w = wad_words[pos >> 1];

        dest[i] = w >> 8;
        dest[i + 1] = w & 0xFF;
    }
    if (i < n) {
        dest[i++] = wad_words[pos >> 1] >> 8;
    }
    return n;
}

long zen_rd16(const byte *p) { return (int16_t) (p[0] | (p[1] << 8)); }
long zen_rd32(const byte *p)
{
    return (long) p[0] | ((long) p[1] << 8) | ((long) p[2] << 16) | ((long) p[3] << 24);
}

/* ---- wad_file_class (replaces w_file_stdc.c) ---- */

extern wad_file_class_t stdc_wad_file;
static wad_file_t the_wad;

static wad_file_t *zen_open(char *path)
{
    byte hdr[12];

    (void) path;
    zen_wad_read(0L, hdr, 12L);
    if (hdr[1] != 'W' || hdr[2] != 'A' || hdr[3] != 'D') {
        return NULL;                                    /* no WAD loaded */
    }
    the_wad.file_class = &stdc_wad_file;
    the_wad.mapped = NULL;
    the_wad.length = zen_rd32(hdr + 8) + 16L * zen_rd32(hdr + 4);
    return &the_wad;
}

static void zen_close(wad_file_t *file) { (void) file; }

static size_t zen_read(wad_file_t *file, unsigned int offset, void *buffer, size_t len)
{
    (void) file;
    return (size_t) zen_wad_read(offset, buffer, len);
}

wad_file_class_t stdc_wad_file = { zen_open, zen_close, zen_read };

/* ---- lump conversion ---- */

/* Record layouts of the map lumps: S = little-endian short, U = unsigned short (both become a long here), N = 8 char name */
static const char *record_layout(const char *name)
{
    if (strncmp(name, "THINGS", 8) == 0) return "SSSSS";
    if (strncmp(name, "LINEDEFS", 8) == 0) return "SSSSSSS";
    if (strncmp(name, "SIDEDEFS", 8) == 0) return "SSNNNS";
    if (strncmp(name, "VERTEXES", 8) == 0) return "SS";
    if (strncmp(name, "SEGS", 8) == 0) return "SSSSSS";
    if (strncmp(name, "SSECTORS", 8) == 0) return "SS";
    if (strncmp(name, "NODES", 8) == 0) return "SSSSSSSSSSSSUU";                 /* children: unsigned short */
    if (strncmp(name, "SECTORS", 8) == 0) return "SSNNSSS";
    if (strncmp(name, "BLOCKMAP", 8) == 0) return "S";
    return NULL;
}

static void convert_records(byte *data, long size, const char *layout)
{
    long src = 0, dst = 0;
    const char *f;

    for (;;) {
        for (f = layout; *f != 0; f++) {
            long need = *f == 'N' ? 8L : 2L;
            int k;

            if (src + need > size) {
                return;
            }
            if (*f != 'N') {                                /* short -> long, 2 words */
                long v = zen_rd16(data + src);

                if (*f == 'U') {
                    v &= 0xFFFFL;
                }

                data[dst] = (byte) (v >> 16);
                data[dst + 1] = (byte) v;
                dst += 2;
            } else {
                for (k = 0; k < 8; k++) {
                    data[dst++] = data[src + k];
                }
            }
            src += need;
        }
    }
}

/* offsets of the columns of the patch being converted */
#define MAX_COLUMNS 2048
static long column_ofs[MAX_COLUMNS];

/* A patch is recognised by its structure: columnofs[0] follows the table, every column is a
 * chain of posts ending in 0xff inside the lump. Converted to patch_t of r_defs.h: four longs
 * (width, height, leftoffset, topoffset) and a long per column offset; the column data moves
 * down by 2 * width words and the offsets with it. */
static int convert_patch(byte *data, long size)
{
    long width, height, i, shrink, pos, left, top;
    long *hdr;

    if (size < 16L) {
        return 0;
    }
    width = zen_rd16(data);
    height = zen_rd16(data + 2);
    if (width < 1L || width > MAX_COLUMNS || height < 1L || height > 1024L
        || 8L + 4L * width > size || zen_rd32(data + 8) != 8L + 4L * width) {
        return 0;
    }
    for (i = 0; i < width; i++) {
        column_ofs[i] = zen_rd32(data + 8 + 4 * i);
        pos = column_ofs[i];
        if (pos < 8L + 4L * width || pos >= size) {
            return 0;
        }
        while (data[pos] != 0xFF) {                     /* post: top, length, pad, data, pad */
            pos += data[pos + 1] + 4L;
            if (pos >= size) {
                return 0;
            }
        }
    }
    shrink = 2L * width;                                /* (8 + 4w) - (8 + 2w) */
    left = zen_rd16(data + 4);
    top = zen_rd16(data + 6);
    memmove(data + 8L + 2L * width, data + 8L + 4L * width, (size_t) (size - (8L + 4L * width)));
    hdr = (long *) data;
    hdr[0] = width;
    hdr[1] = height;
    hdr[2] = left;
    hdr[3] = top;
    for (i = 0; i < width; i++) {
        hdr[4 + i] = column_ofs[i] - shrink;
    }
    return 1;
}

void zen_convert_lump(const char *name, byte *data, long size)
{
    const char *layout = record_layout(name);

    if (layout != NULL) {
        convert_records(data, size, layout);
    } else {
        convert_patch(data, size);
    }
}

/* ---- IWAD lookup (replaces d_iwad.c): one WAD, no search path ---- */

char *D_FindWADByName(char *filename) { (void) filename; return NULL; }
char *D_TryFindWADByName(char *filename) { (void) filename; return NULL; }
const iwad_t **D_FindAllIWADs(int mask) { (void) mask; return NULL; }
char *D_SaveGameIWADName(GameMission_t mission) { (void) mission; return "doom1.wad"; }
char *D_SuggestIWADName(GameMission_t mission, GameMode_t mode)
{
    (void) mission; (void) mode;
    return "doom1.wad";
}
char *D_SuggestGameName(GameMission_t mission, GameMode_t mode)
{
    (void) mission; (void) mode;
    return "DOOM";
}
void D_CheckCorrectIWAD(GameMission_t mission) { (void) mission; }

char *D_FindIWAD(int mask, GameMission_t *mission)
{
    (void) mask;
    *mission = doom;
    return "doom1.wad";
}
