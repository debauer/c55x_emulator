/* SPDX-License-Identifier: GPL-2.0-or-later
 * WAD access of the Doom port (zen_wad.c) */
#ifndef ZEN_WAD_H
#define ZEN_WAD_H

#include "doomtype.h"

/* WAD file in RAM: byte address 0xBFE000 = word address 0x5FF000 (doom.cmd), loaded with
 * --load 0xBFE000:DOOM1.WAD */
#define ZEN_WAD_BYTE_ADDR 0xBFE000L
#define ZEN_WAD_WORD_ADDR 0x5FF000L

long zen_wad_read(long pos, byte *dest, long n);    /* one byte per destination word */
long zen_rd16(const byte *p);                       /* little-endian field of a widened lump */
long zen_rd32(const byte *p);
void zen_convert_lump(const char *name, byte *data, long size);

#endif
