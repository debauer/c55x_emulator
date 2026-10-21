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

    def test_clamped_count_result_is_confined_to_the_working_width(self):
        # SPRU374E 4.70.3 with M40 = 0 (default): the shifter works in the
        # 32-bit domain. Count 40 saturates to 31: 1 << 31 leaves the signed
        # 32-bit range, the destination flag is set and the truncated result
        # (guard bits clear) is stored. Count -33 saturates to -32:
        # 0x1234 >> 32 is 0.
        cases = (("T0 = #40", "AC1 = AC0 << T0", 0x01, 0x80000000, [0, 1, 0, 0]),
                 ("T0 = #-33", "AC1 = AC0 >> T0", 0x1234, 0x0, [0, 1, 0, 0]))
        for set_t, insn, value, expected, acov in cases:
            with self.subTest(insn=insn):
                cpu, _ = machine(set_t, insn)
                cpu.ac[0] = value
                run(cpu)
                self.assertEqual(cpu.ac[1], expected)
                self.assertEqual(cpu.core.acov(), acov)


if __name__ == "__main__":
    unittest.main()


class ShiftM40Tests(unittest.TestCase):
    """M40 selects the shifter working width (SPRU374E 4.70.3)."""

    def test_m40_off_extracts_bits_shifted_past_31(self):
        # SPRU371F / v3.x CPU 2.10.2.9: with M40 = 0 shifts work on 32-bit
        # values and left shifts extract bits shifted out past bit 31;
        # overflow is still reported at the boundary.
        cpu, _ = machine("AC0 = #0x10000", "AC0 = AC0 << #16")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0)
        self.assertEqual(cpu.core.acov(), [1, 0, 0, 0])

    def test_m40_off_with_satd_saturates(self):
        cpu, _ = machine("bit(ST1, #9) = #1", "AC0 = #0x10000", "AC0 = AC0 << #16")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0x7FFFFFFF)
        self.assertEqual(cpu.core.acov(), [1, 0, 0, 0])
        cpu, _ = machine("bit(ST1, #9) = #1", "AC0 = #-4", "AC0 = AC0 << #30")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0xFF80000000)
        self.assertEqual(cpu.core.acov(), [1, 0, 0, 0])

    def test_m40_on_keeps_the_guard_bits(self):
        cpu, _ = machine("bit(ST1, #10) = #1", "AC0 = #0x10000", "AC0 = AC0 << #16")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0x100000000)
        self.assertEqual(cpu.core.acov(), [0, 0, 0, 0])

    def test_step_and_run_agree_on_immediate_shifts(self):
        # the interpreter (single-step) and the compiled engine must produce
        # the same values for every shift form
        lines = ("AC0 = #0x1234", "AC0 = AC0 << #8", "AC1 = AC0 << #24",
                 "T1 = #4", "AC2 = AC0 << T1", "AC3 = AC0 >> #4")
        cpu_a, _ = machine(*lines)
        run(cpu_a)
        cpu_b, _ = machine(*lines)
        for _ in lines:
            cpu_b.step()
        self.assertEqual(cpu_a.ac, cpu_b.ac)
