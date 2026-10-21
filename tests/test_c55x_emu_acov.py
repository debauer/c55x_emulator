"""Synthetic tests for the accumulator overflow flags (ACOV0..ACOV3) on Tx shift counts."""

import unittest

from c55x_emu.testutil import machine, run


class AcovTests(unittest.TestCase):
    # SPRU374E 4.70.3: shifting an accumulator by a Tx content outside -32..+31
    # saturates the count to that bound and reports an overflow on the destination
    # accumulator (sticky per-accumulator flag, clear at reset).
    def test_flags_are_clear_after_reset(self):
        cpu, _ = machine("AC0 = #0")
        self.assertEqual(cpu.core.acov(), [0, 0, 0, 0])

    def test_in_range_count_leaves_the_flags_clear(self):
        for count, value in ((5, 0x1234), (-4, 0x5678)):
            with self.subTest(count=count):
                cpu, _ = machine(f"T0 = #{count}", "AC1 = AC0 << T0")
                cpu.ac[0] = value
                run(cpu)
                self.assertEqual(cpu.core.acov(), [0, 0, 0, 0])

    def test_count_above_31_sets_the_destination_flag(self):
        cpu, _ = machine("T0 = #40", "AC1 = AC0 << T0", "AC0 = #7")
        cpu.ac[0] = 0x01
        run(cpu)
        self.assertEqual(cpu.core.acov(), [0, 1, 0, 0])     # sticky, source flag clear

    def test_count_below_minus_32_sets_the_destination_flag(self):
        cpu, _ = machine("T0 = #-33", "AC2 = AC0 >> T0")
        cpu.ac[0] = 0x1234
        run(cpu)
        self.assertEqual(cpu.core.acov(), [0, 0, 1, 0])

    def test_clamped_count_result_is_unchanged(self):
        # count 40 shifts by 31; count -33 shifts right by 32 through '>>'
        # (the 40-bit accumulator keeps the low result bits)
        cases = (("T0 = #40", "AC1 = AC0 << T0", 0x01, 0x80000000),
                 ("T0 = #-33", "AC1 = AC0 >> T0", 0x1234, 0x3400000000))
        for set_t, insn, value, expected in cases:
            with self.subTest(insn=insn):
                cpu, _ = machine(set_t, insn)
                cpu.ac[0] = value
                run(cpu)
                self.assertEqual(cpu.ac[1], expected)


if __name__ == "__main__":
    unittest.main()
