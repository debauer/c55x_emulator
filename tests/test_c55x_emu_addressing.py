"""C55x emulator: circular addressing, the ARMS dependent short operands and the 32-bit
stack transfers the DSP/BIOS dispatcher uses to save the circular buffer registers, and
the M40 computation mode (used by a 64-bit multiply at 0x93ff63) and CARRY/Borrow for
the 64-bit add and subtract at 0x90b4ba / 0x90b4ea."""

import unittest

from c55x_emu.expr import parse
from c55x_emu.registers import EmulationError
from c55x_emu.testutil import machine, run


class CircularTests(unittest.TestCase):
    def test_index_wraps_in_the_buffer(self):
        cpu, bus = machine("XAR2 = mar(*(#010003h))", "T0 = #-2", "bit(ST2, #2) = #1",
                           "AC0 = *AR2+", "AC1 = *AR2(#0002h)", "AC2 = *+AR2(#-0001h)",
                           "AC3 = *AR2(T0)")
        cpu.bk03, cpu.bsa23 = 4, 0x1600
        for i in range(4):
            bus.write(0x011600 + i, 10 + i)
        run(cpu)
        self.assertEqual([a & 0xFFFF for a in cpu.ac], [13, 12, 13, 11])
        self.assertEqual(cpu.xar[2], 0x010003)

    def test_linear_pointer_ignores_the_buffer(self):
        cpu, bus = machine("XAR3 = mar(*(#01600h))", "AC0 = *AR3+")
        cpu.bk03, cpu.bsa23 = 4, 0x100
        bus.write(0x1600, 7)
        run(cpu)
        self.assertEqual((cpu.ac[0], cpu.xar[3]), (7, 0x1601))

    def test_zero_buffer_size_raises(self):
        cpu, _ = machine("bit(ST2, #3) = #1", "AC0 = *AR3")
        with self.assertRaises(EmulationError):
            run(cpu)


class ShortOperandTests(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(parse("*AR1(short(#3))"), ("mem", "short", "AR1", 3))

    def test_dsp_mode_uses_t1(self):            # SPRU375G table 6-2
        cpu, bus = machine("XAR1 = mar(*(#01600h))", "T1 = #2", "bit(ST2, #15) = #0",
                           "AC0 = *AR1(short(#3))", "AC1 = *AR1(short(#1))",
                           "bit(ST2, #15) = #1", "AC2 = *AR1(short(#3))")
        for i in range(6):
            bus.write(0x1600 + i, 0x100 + i)
        run(cpu)
        self.assertEqual([a & 0xFFFF for a in cpu.ac[:3]], [0x102, 0x100, 0x105])
        self.assertEqual(cpu.xar[1], 0x1602)


class LongStackTests(unittest.TestCase):
    def test_mmr_block_save_and_restore(self):   # BIOS dispatcher 0x03bbbe / 0x03bce0
        cpu, _ = machine("BK47 = #256", "BSA45 = #99", "XAR1 = mar(*(#0002eh))", "repeat(#10)",
                         "push(dbl(*AR1+))", "BK47 = #0", "BSA45 = #0",
                         "XAR1 = mar(*(#00042h))", "repeat(#10)", "dbl(*AR1-) = pop()")
        run(cpu)
        self.assertEqual((cpu.bk47, cpu.bsa45, cpu.xsp), (256, 99, 0x1000))

    def test_long_push_order(self):
        cpu, bus = machine("XAR1 = mar(*(#01600h))", "push(dbl(*AR1))")
        bus.write32(0x1600, 0x12345678)
        run(cpu)
        self.assertEqual((cpu.xsp, bus.read(0xFFE), bus.read(0xFFF)), (0xFFE, 0x1234, 0x5678))


class M40Tests(unittest.TestCase):
    def test_m40_off_keeps_32_bit_semantics(self):
        # SPRU374E 4.70.3: M40 = 0 substitutes the shifter input, detects
        # overflow at bit 31 and saturates per SATD, but does not truncate
        # the 40-bit destination; bit 32 survives in the guard bits and the
        # overflow is reported (Core.acov()). An earlier revision of this
        # test expected the bit to be shifted out.
        cpu, _ = machine("AC0 = #1 << #16", "AC0 = AC0 << #16",
                         "TC1 = AC0 == #0")
        run(cpu)
        self.assertEqual((cpu.ac[0], cpu.tc1, tuple(cpu.core.acov())),
                         (1 << 32, 1, (1, 0, 0, 0)))

    def test_m40_on_uses_the_guard_bits(self):
        cpu, _ = machine("bit(ST1, #10) = #1", "bit(ST1, #8) = #0", "AC0 = #1 << #16",
                         "AC0 = AC0 << #16", "TC1 = AC0 == #0", "AC1 = AC0 <<< #-16",
                         "AC2 = AC0 << #-16")
        run(cpu)
        self.assertEqual((cpu.tc1, cpu.ac[1], cpu.ac[2]), (0, 0x10000, 0x10000))

    def test_sign_extended_low_byte(self):          # gfx_draw_char 0x80022f: glyph bearing
        cpu, bus = machine("XAR3 = mar(*(#01600h))", "AC1 = *AR3", "AC1 = AC1 << #24",
                           "AC1 = AC1 << #-24", "AC2 = *AR3(#0001h)", "AC2 = AC2 << #24",
                           "AC2 = AC2 << #-24")
        bus.write(0x1600, 0x0600)
        bus.write(0x1601, 0x06FE)
        run(cpu)
        self.assertEqual((cpu.ac[1], cpu.ac[2]), (0, (1 << 40) - 2))


def long_op(op, a, b):
    """64-bit a op b as the firmware does it: low longs first, then the high words."""
    carry = "Carry" if op == "+" else "Borrow"
    cpu, bus = machine("bit(ST1, #8) = #0", "XAR1 = mar(*(#01600h))", "XAR2 = mar(*(#01610h))",
                       "XAR3 = mar(*(#01620h))", "AC0 = dbl(*AR1+)", f"AC0 = AC0 {op} dbl(*AR2+)",
                       "dbl(*AR3+) = AC0", "AC0 = dbl(*AR1)",
                       f"AC0 = AC0 {op} *AR2(#0001h) {op} {carry}", f"AC0 = AC0 {op} (*AR2 << #16)",
                       "dbl(*AR3) = AC0")
    for base, v in ((0x1600, a), (0x1610, b)):
        bus.write32(base, v & 0xFFFFFFFF)
        bus.write32(base + 2, v >> 32)
    run(cpu)
    return (bus.read32(0x1622) << 32) | bus.read32(0x1620)


class CarryTests(unittest.TestCase):
    def test_64_bit_add(self):
        for a, b in ((0xFFFFFFFF, 1), (0x1234_8000_FFFF_FFFF, 0x0000_8000_0000_0001), (5, 7)):
            self.assertEqual(long_op("+", a, b), (a + b) & (1 << 64) - 1, hex(a))

    def test_64_bit_subtract(self):
        for a, b in ((0x1_0000_0000, 1), (0x1234_0000_0000_0000, 0x0000_0001_0000_0001), (9, 7)):
            self.assertEqual(long_op("-", a, b), (a - b) & (1 << 64) - 1, hex(a))

    def test_product_leaves_carry(self):
        cpu, _ = machine("AC0 = #0", "AC0 = AC0 - #1", "AC1 = #1", "AC0 = AC0 + (T0 * T1)")
        run(cpu)
        self.assertEqual(cpu.carry, 0)                    # borrow from the subtraction


class BitFieldTests(unittest.TestCase):
    def test_band_sets_tc_on_any_bit(self):        # gfx_clear_rect 0x801a30: TC1 = y0 & 7
        cpu, bus = machine("XAR1 = mar(*(#01600h))", "TC1 = *AR1 & #7", "TC2 = *AR1 & #8")
        bus.write(0x1600, 6)
        run(cpu)
        self.assertEqual((cpu.tc1, cpu.tc2), (1, 0))



class MmrLoadTests(unittest.TestCase):
    def test_mmr_load_sign_extends_like_memory(self):   # a MMR access is an Smem operand
        cpu, _ = machine("T0 = #-2", "AC0 = @T0_L || mmap()", "AR1 = #-2",
                         "AC1 = uns(@AR1_L) || mmap()")
        run(cpu)
        self.assertEqual((cpu.ac[0], cpu.ac[1]), (0xFFFFFFFFFE, 0xFFFE))

if __name__ == "__main__":
    unittest.main()
