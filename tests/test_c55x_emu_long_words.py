"""Synthetic tests for addressed-word ordering of long transfers."""

import unittest

from c55x_emu.testutil import machine, run


class LongWordAddressOrderTests(unittest.TestCase):
    # TI SWPU073E section 3.3.2, p. 3-6, and section 6.5.3.1, p. 6-64:
    # The addressed word is MSW; the paired LSW is at address XOR 1.
    def test_memory_load_both_parities_and_postincrement(self):
        for parity in (0, 1):
            with self.subTest(parity=parity):
                cpu, bus = machine("AC2 = uns(dbl(*AR2+))")
                cpu.xar[2] = 0x2400 + parity
                bus.write(0x2400, 0x2468)
                bus.write(0x2401, 0xB135)
                run(cpu)
                self.assertEqual(cpu.ac[2], (0x2468B135, 0xB1352468)[parity])
                self.assertEqual(cpu.xar[2], 0x2402 + parity)

    def test_memory_store_both_parities_and_postincrement(self):
        for parity in (0, 1):
            with self.subTest(parity=parity):
                cpu, bus = machine("dbl(*AR3+) = AC2")
                cpu.xar[3], cpu.ac[2] = 0x2600 + parity, 0x6ACE3579
                run(cpu)
                expected = ((0x6ACE, 0x3579), (0x3579, 0x6ACE))[parity]
                self.assertEqual((bus.read(0x2600), bus.read(0x2601)), expected)
                self.assertEqual(cpu.xar[3], 0x2602 + parity)

    def test_mmr_load_both_parities(self):
        for register, expected in (("AC0_L", 0x13576ACE), ("AC0_H", 0x6ACE1357)):
            with self.subTest(register=register):
                cpu, _ = machine(f"AC2 = uns(dbl(@{register})) || mmap()")
                cpu.ac[0] = 0x3D6ACE1357
                run(cpu)
                self.assertEqual(cpu.ac[2], expected)

    def test_mmr_store_both_parities(self):
        for register, expected in (("AC0_L", 0x35796ACE), ("AC0_H", 0x6ACE3579)):
            with self.subTest(register=register):
                cpu, _ = machine(f"dbl(@{register}) = AC2 || mmap()")
                cpu.ac[0], cpu.ac[2] = 0x3D00000000, 0x6ACE3579
                run(cpu)
                self.assertEqual(cpu.ac[0] & 0xFFFFFFFF, expected)


if __name__ == "__main__":
    unittest.main()
