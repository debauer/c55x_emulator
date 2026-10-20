"""Doom port (examples/doom): source conversion, linker command file, tool chain smoke test, and
Doom itself when it is built and a WAD is given (ZEN_DOOM_WAD)."""

import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from c55x_emu.loader import load_program
from c55x_emu.machine import Machine
from c55x_emu.paths import ROOT, TI_BIN

DOOM = Path(__file__).resolve().parents[1] / "examples" / "doom"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


transform = module("zen_transform", DOOM / "port" / "transform.py")
linkcmd = module("zen_linkcmd", DOOM / "linkcmd.py")
HAVE_TI = (TI_BIN / "cl55").exists()


class TransformTests(unittest.TestCase):
    def test_literals_become_long(self):
        self.assertEqual(transform.transform("x = 1<<16; y = 0x80000000; z = 10u;"),
                         "x = 1L<<16L; y = 0x80000000L; z = 10uL;")

    def test_strings_comments_floats_and_includes_stay(self):
        text = '#include "a1.h"\nchar *s = "a 12"; /* 7 */ float f = 3.5; // 9\nx = 1e5;'
        self.assertEqual(transform.transform(text), text)

    def test_bare_unsigned_and_short_int(self):
        self.assertEqual(transform.transform("unsigned a; unsigned char b; short int c;"),
                         "unsigned long a; unsigned char b; short c;")

    def test_enum_with_big_values_becomes_defines(self):
        out = transform.transform("typedef enum { A = 1, B = 0x10000, C = 0x20000 } flags_t;")
        self.assertIn("typedef long flags_t;", out)
        self.assertIn("#define B 0x10000L", out)
        self.assertNotIn("enum", out)

    def test_small_enum_stays(self):
        text = "typedef enum { A, B, C } small_t;"
        self.assertEqual(transform.transform(text), text)


class LinkCmdTests(unittest.TestCase):
    def test_sections_are_packed_into_blocks_of_at_most_64_kb(self):
        sizes = {f"o{i}.obj": {".bss": 0x6000, ".const:.string": 0x100} for i in range(5)}
        original = linkcmd.section_sizes
        linkcmd.section_sizes = lambda path: sizes[path.name]
        try:
            blocks = linkcmd.pack([Path(name) for name in sizes])
        finally:
            linkcmd.section_sizes = original
        self.assertEqual(sorted(len(b) for b in blocks[".bss"]), [1, 2, 2])    # 0x6000 twice per block
        self.assertEqual(len(blocks[".const"]), 1)

    def test_a_section_larger_than_a_block_is_refused(self):
        original = linkcmd.section_sizes
        linkcmd.section_sizes = lambda path: {".bss": 0x20000}
        try:
            with self.assertRaises(SystemExit):
                linkcmd.pack([Path("big.obj")])
        finally:
            linkcmd.section_sizes = original


@unittest.skipUnless(HAVE_TI, "TI C55x tools not found (PATH, ZEN_TI_BIN)")
class SmokeTests(unittest.TestCase):
    def test_runtime_start_up_math_data_and_malloc(self):
        subprocess.run([str(DOOM / "build.sh"), "smoke"], check=True, cwd=ROOT, capture_output=True)
        out = ROOT / "build" / "doom" / "smoke" / "smoke.out"
        program = load_program(str(out))
        machine = Machine(program)
        machine.run(3_000_000)
        raw = machine.bus.mem
        address = program.symbols["_g_result"]
        self.assertEqual((raw[address] << 8) | raw[address + 1], 6)


WAD = os.environ.get("ZEN_DOOM_WAD")
DOOM_OUT = ROOT / "build" / "doom" / "doom" / "doom.out"


@unittest.skipUnless(WAD and Path(WAD).is_file() and DOOM_OUT.exists(),
                     "needs ZEN_DOOM_WAD (DOOM1.WAD) and a built examples/doom/build.sh doom")
class DoomTests(unittest.TestCase):
    def test_e1m1_starts_and_the_play_button_walks(self):
        program = load_program(str(DOOM_OUT))
        machine = Machine(program)
        machine.bus.load(0xBFE000, Path(WAD).read_bytes())
        symbols, mem = program.symbols, machine.bus.mem

        def long_at(name):
            a = symbols[name]
            value = int.from_bytes(mem[a:a + 4], "big")
            return value - (1 << 32) if value >= 1 << 31 else value

        machine.run(150_000_000)
        self.assertEqual((long_at("_viewx") >> 16, long_at("_viewy") >> 16), (1056, -3616))
        machine.mcu.inject(0x12)                    # Play: forward
        machine.run(60_000_000)
        self.assertGreater(long_at("_viewy") >> 16, -3200)


if __name__ == "__main__":
    unittest.main()
