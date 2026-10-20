/* Memory layout of the Doom port (byte addresses; the emulator has one flat 16 MB memory).
 * ROM model: variables are initialised by the run-time start-up (_c_int00, .cinit).
 *   0x040000  code, constants and the .cinit tables (1 MB)
 *   0x140000  .data, .bss, small heap (1 MB)
 *   0x510000  Doom zone (set up by the port), up to 0xBFDFFF
 *   0xBFE000  WAD file, packed 2 bytes per word (loaded with --load)
 * 0x500000..0x507FFF is the CE1 window of ATA and LCD, never used for memory. */
-c
-e _zen_reset
-stack 0x1000
-sysstack 0x400
-heap 0x2000
MEMORY
{
    VECT (RX)   : origin = 0x00000100, length = 0x00000200
    DARAM (RW)  : origin = 0x00002000, length = 0x0000E000
    ROM (RX)    : origin = 0x00040000, length = 0x00100000
    RAM (RW)    : origin = 0x00140000, length = 0x00100000
}
SECTIONS
{
    .vectors  : {} > VECT
    .text     : {} > ROM
    .const    : {} > ROM
    .cinit    : {} > ROM
    .switch   : {} > ROM
    .data     : {} > RAM
    .bss      : {} > RAM
    .sysmem   : {} > RAM
    .stack    : {} > DARAM
    .sysstack : {} > DARAM
}
