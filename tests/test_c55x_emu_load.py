"""--load ADDR:FILE of the headless entry."""

import argparse
import tempfile
import unittest
from pathlib import Path

from c55x_emu.__main__ import parse_load
from c55x_emu.bus import Bus


class LoadTests(unittest.TestCase):
    def test_file_lands_at_the_byte_address(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "data.bin"
            path.write_bytes(bytes([0x12, 0x34, 0xAB, 0xCD]))
            addr, data = parse_load(f"0x9000:{path}")
        self.assertEqual(addr, 0x9000)
        bus = Bus()
        bus.load(addr, data)
        self.assertEqual([bus.read(0x4800), bus.read(0x4801)], [0x1234, 0xABCD])

    def test_bad_arguments(self):
        for text in ("0x9000", "zz:/nonexistent", "0x9000:/nonexistent/file"):
            with self.assertRaises(argparse.ArgumentTypeError):
                parse_load(text)


if __name__ == "__main__":
    unittest.main()
