# Doom on the C55x emulator

A proof of concept: [doomgeneric](https://github.com/ozkl/doomgeneric) (the portable Doom of id Software, GPL-2.0)
ported to the TI C55x of the Zen Touch and run on this emulator as a bare program, without the
original firmware. Shareware Doom, E1M1, on the 160x104 1-bit LCD, steered with the player's keys.

    scripts/run_doom.sh --gui DOOM1.WAD            # Qt window (needs PyQt5, see the main README)
    scripts/run_doom.sh DOOM1.WAD --steps 300000000 --png doom.png

You need the TI C55x tools (`PATH` or `ZEN_TI_BIN`), `git`, `patch` and your own `DOOM1.WAD`
(shareware 1.9, 4196020 bytes; not part of this repository). `scripts/run_doom.sh` builds the
program first (`examples/doom/build.sh doom`, about 25 s): `fetch.sh` clones doomgeneric at a fixed
commit into `build/doom/src`, `port/transform.py` converts a copy for the C55x and `doom-zen.patch`
applies the hand-made changes. `--load 0xBFE000:DOOM1.WAD` of the emulator puts the WAD into RAM.
`build.sh smoke` builds a small tool chain test (`tests/test_doom_port.py` runs it).

## Controls

| Zen Touch | Keyboard (GUI) | Doom |
| --- | --- | --- |
| Play | Space | forward |
| Random | R | backward |
| Previous / Next | Left / Right | turn left / right |
| OK | Return | fire |
| Back | Backspace | use (doors, switches) |
| Menu | M | Escape (menu) |
| Volume + / - | + / - | Enter (menu) / Tab (automap) |
| touch pad up / down | Up / Down | menu up / down |

The emulator's CPU clock is set to 24 MHz. On the author's machine the game then runs at about
12 frames per emulated second and the emulator keeps real time.

## How it works

- **int is 16 bits, char too.** Doom assumes 32-bit ints, so `port/zen_types.h` (force-included)
  makes `int` and `short` a `long` after the C library headers have been read, and
  `port/transform.py` makes every integer literal a `long` (the C55x would compute `1 << 16` in 16
  bits) and turns the one enum with values above 0x7fff into defines. `char` stays 16 bits wide:
  a `byte` takes a word.
- **The WAD** stays in RAM as it is (2 bytes per word). `port/zen_wad.c` reads lumps into the zone
  one byte per word and converts lumps with multi-byte fields (patches, the map lumps) into the
  compiler's struct layout, so the Doom code that lays `patch_t` or `mapvertex_t` over a lump
  works unchanged. Textures (`R_InitTextures`) read their lumps byte by byte.
- **64K-word data pages.** Address increments (`*AR+`) wrap inside a 64K-word page, so no object may
  cross one. `doom-zen.patch` builds the Doom zone from one free block per page (`Z_ClearZone`),
  and `linkcmd.py` packs the static data of the object files into 64 KB blocks.
- **Time and input.** Timer 0 counts milliseconds, `IDLE` sleeps between ticks, keys come from the
  MCU events like on the player (`port/zen_platform.c`). `port/zen_video.c` reduces the
  320x200 screen to 160x100, applies gamma and a 4x4 ordered dither and writes the LCD pages.
- Software floating point and 40-bit `long long` are avoided: `FixedMul`/`FixedDiv` are written
  with 16-bit halves.

## Limits

- No sound. Shareware E1M1 only: the game starts there directly (`-warp 1 1`), because the
  full-screen patches (title, help, credits, intermission map: 68168 bytes) do not fit one page.
  Finishing the level or opening help therefore stops with an error message.
- At most 100 visplanes, low detail mode, no savegames, no configuration file.
- `I_Error` messages end in a ring buffer: `zen_log` (see `port/zen_libc.c`), readable from the
  emulator's memory.

## License

Everything in this directory (port layer, conversion script, patch, build scripts) is under
GPL-2.0-or-later, because it is compiled into one program with doomgeneric and the patch modifies
it. The rest of the repository is MIT. Doom itself and its shareware WAD are copyrighted by id Software.
