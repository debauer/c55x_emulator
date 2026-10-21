"""SFTL (algebraic <<< and >>>) pinned to SPRU374E 4.37: the shift works
on the M40-selected width, the destination guard bits are cleared when
M40 = 0, CARRY holds the shifted-out bit and is cleared on a zero count."""

import unittest
from tests.test_c55x_emu_long_words import machine, run


class SftlTests(unittest.TestCase):
    def test_manual_example_4_37_3(self):
        # SFTL AC0, T0, AC1 with T0 = -6, M40 = 0: the manual's worked
        # example expects AC1 = 00 02C0 0048 (guard bits cleared)
        cpu, _ = machine("AC1 = AC0 <<< T0")
        cpu.ac[0], cpu.t[0] = 0x5FB0001234, 0xFFFA
        run(cpu)
        self.assertEqual(cpu.ac[1], 0x02C00048)
        self.assertEqual(cpu.carry, 1)

    def test_right_by_one_clears_the_guards(self):
        cpu, _ = machine("AC3 = AC3 >>> #1")
        cpu.ac[3] = 0xFF80000001
        run(cpu)
        self.assertEqual(cpu.ac[3], 0x40000000)
        self.assertEqual(cpu.carry, 1)

    def test_register_destination_shifts_16_bits(self):
        cpu, _ = machine("T2 = T2 >>> #1")
        cpu.t[2] = 0x0003
        run(cpu)
        self.assertEqual(cpu.t[2], 1)
        self.assertEqual(cpu.carry, 1)

    def test_zero_count_clears_carry(self):
        cpu, _ = machine("AC1 = AC0 <<< T0")
        cpu.ac[0], cpu.t[0], cpu.carry = 0x5FB0001234, 0, 1
        run(cpu)
        self.assertEqual(cpu.ac[1], 0xB0001234)     # guards still cleared
        self.assertEqual(cpu.carry, 0)


if __name__ == "__main__":
    unittest.main()
