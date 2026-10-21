"""MAC/MPY overflow reporting per SPRU374E 4.47/4.48 and the M40 mode
definition of SPRU371F / C55x v3.x CPU 2.10.2.9: the accumulate checks
overflow at the M40-selected boundary, reports the destination ACOVy and
saturates per SATD; multiplication overflow detection depends on SMUL."""

import unittest
from tests.test_c55x_emu_long_words import machine, run


class MacOverflowTests(unittest.TestCase):
    def test_m40_off_accumulate_overflow_reports_and_saturates(self):
        cpu, _ = machine("T1 = #0x7FFF", "AC0 = #0x7FFFFFFF",
                         "AC0 = AC0 + (T1 * #0x7FFF)")
        run(cpu)
        # product 0x3FFF0001 + 0x7FFFFFFF leaves the signed 32-bit range
        self.assertEqual(cpu.ac[0], 0x7FFFFFFF + 0x7FFF * 0x7FFF)
        self.assertEqual(cpu.core.acov(), [1, 0, 0, 0])
        cpu, _ = machine("bit(ST1, #9) = #1", "T1 = #0x7FFF",
                         "AC0 = #0x7FFFFFFF", "AC0 = AC0 + (T1 * #0x7FFF)")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0x7FFFFFFF)
        self.assertEqual(cpu.core.acov(), [1, 0, 0, 0])

    def test_m40_off_negative_overflow_saturates(self):
        cpu, _ = machine("bit(ST1, #9) = #1", "T1 = #0x7FFF",
                         "AC0 = #-0x7FFFFFFF", "AC0 = AC0 - (T1 * #0x7FFF)")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0xFF80000000)
        self.assertEqual(cpu.core.acov(), [1, 0, 0, 0])

    def test_m40_on_accumulate_uses_40_bits(self):
        cpu, _ = machine("bit(ST1, #10) = #1", "T1 = #0x7FFF",
                         "AC0 = #0x7FFFFFFF", "AC0 = AC0 + (T1 * #0x7FFF)")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0x7FFFFFFF + 0x7FFF * 0x7FFF)
        self.assertEqual(cpu.core.acov(), [0, 0, 0, 0])

    def test_in_range_product_and_sum_report_nothing(self):
        cpu, _ = machine("T1 = #0x40", "AC0 = #0x1000",
                         "AC0 = AC0 + (T1 * #0x40)")
        run(cpu)
        self.assertEqual(cpu.ac[0], 0x1000 + 0x40 * 0x40)
        self.assertEqual(cpu.core.acov(), [0, 0, 0, 0])

    def test_smul_reports_the_multiplier_overflow(self):
        # FRCT = 1, SMUL = 1, SATD = 1: 0x8000 x 0x8000 saturates the
        # product to 0x7FFF FFFF and reports
        cpu, _ = machine("bit(ST1, #6) = #1", "bit(ST1, #9) = #1",
                         "bit(ST3, #1) = #1", "T1 = #0x8000",
                         "AC0 = AC1 + (T1 * #-0x8000)")
        run(cpu)
        self.assertEqual((cpu.ac[0], cpu.core.acov()[0]), (0x7FFFFFFF, 1))

    def test_unsigned_product_reports_nothing(self):
        cpu, bus = machine("XAR3 = mar(*(#01600h))", "T1 = #0xFFFF",
                           "AC0 = uns(T1 * *AR3)")
        bus.write(0x1600, 0xFFFF)
        run(cpu)
        self.assertEqual(cpu.ac[0], 0xFFFE0001)
        self.assertEqual(cpu.core.acov(), [0, 0, 0, 0])

    def test_step_and_run_agree_on_mac(self):
        lines = ("T1 = #0x7FFF", "AC0 = #0x7FFFFFFF",
                 "AC0 = AC0 + (T1 * #0x7FFF)", "AC1 = AC0 >> #4")
        cpu_a, _ = machine(*lines)
        run(cpu_a)
        cpu_b, _ = machine(*lines)
        for _ in lines:
            cpu_b.step()
        self.assertEqual(cpu_a.ac, cpu_b.ac)


if __name__ == "__main__":
    unittest.main()
