"""Programs for the emulator: a C55x COFF executable or the Jukebox2 CODE image.

Jukebox2: payloads from output/code/Jukebox2_code_blocks at their record addresses (byte
addresses; program and data space share memory), decoded instructions from the project
listing (output/analysis/listing), start at the reset vector target 0x039dc0
(reset_c_int00, open question B1), i.e. the state right after the boot loader.
Function names come from analysis/symbols.json (for reports only).
FRESC (the recovery program, output/code/FRESC_decrypted_code_blocks) has the same reset
vector target (open question B1) and no listing: everything is decoded on demand.
Hybrid: Jukebox2 with the reconstructed C functions built by scripts/c55x_hybrid.py
(build/hybrid: hybrid.out at byte 0xF00000 plus 4-byte jump patches); patched addresses
lose their listing entries and are decoded again, C functions are named "<name> [C]".
"""

import argparse
import bisect
import collections
import json
import re
from dataclasses import dataclass, field
from pathlib import Path

from . import coff, listing

from .paths import CODE_DIR, FRESC_DIR, HYBRID_DIR, LISTING_DIR, ROOT, SYMBOLS

JUKEBOX2_ENTRY = 0x039DC0
# bios_idle_loop: XAR0 = dbl(*(#01ce34h)); call 0x03c531 (runs the hooks of the
# zero-terminated list at XAR0); goto bios_idle_loop
JUKEBOX2_IDLE = (0x03C63E, 0x01CE34)
# loops that only read RTCPINTR.UIP (rtc_time_set 0x00826c49, rtc_alarm_set 0x00826cf9;
# src/drivers/rtc.c): time may jump to the next event there
JUKEBOX2_POLL = frozenset((0x826C64, 0x826CEE, 0x826D21))


@dataclass
class Program:
    name: str
    sections: list                      # [(byte address, bytes)]
    entry: int
    insns: dict
    symbols: dict = field(default_factory=dict)     # name -> address (branch targets)
    functions: list = field(default_factory=list)   # sorted [(address, name)]
    bios_idle: tuple = None             # (idle loop pc, word holding the idle hook list)
    poll_loops: frozenset = frozenset() # heads of loops that only poll a peripheral

    def where(self, pc):
        i = bisect.bisect_right(self.functions, (pc, "\uffff")) - 1
        if i < 0:
            return f"0x{pc:06x}"
        a, name = self.functions[i]
        return f"{name}+0x{pc - a:x}" if pc != a else name


LOCAL_LABEL = re.compile(r"\$C\$L\d+(?!\d)")


def resolve_local_labels(insns, symbol_list, funcs):
    """dis55 names a branch target by its label; cl55's local labels ($C$Ln) repeat in
    every object file, so a name can stand for several addresses. Replace such a name with
    the address of the label of that name inside the function holding the branch (cl55
    never branches out of a function: each one is a subsection of its own)."""
    defs = collections.defaultdict(set)
    for name, value in symbol_list:
        if LOCAL_LABEL.fullmatch(name):
            defs[name].add(value)
    starts = [a for a, _ in funcs]
    for insn in insns.values():
        for name in set(LOCAL_LABEL.findall(insn.text)):
            if len(defs.get(name, ())) < 2:
                continue
            i = bisect.bisect_right(starts, insn.addr) - 1
            lo, hi = starts[i], starts[i + 1] if i + 1 < len(starts) else 1 << 24
            inside = [v for v in defs[name] if lo <= v < hi]
            if len(inside) != 1:
                raise ValueError(f"0x{insn.addr:06x}: {name} is {len(inside)} labels in its function")
            insn.text = re.sub(re.escape(name) + r"(?!\d)", f"#0x{inside[0]:x}", insn.text)


def load_coff(path):
    path = Path(path)
    image = coff.load(path.read_bytes())
    syms = image["symbols"]
    funcs = sorted((a, n) for n, a in syms.items() if n.startswith("_"))
    insns = listing.parse(listing.disassemble(path))
    resolve_local_labels(insns, image["symbol_list"], funcs)
    return Program(path.stem, [(a, d) for _, a, d in image["sections"]], image["entry"],
                   insns, syms, funcs)


def code_sections(code_dir):
    manifest = json.loads((code_dir / "manifest.json").read_text())
    return [(b["address_field"], (code_dir / b["filename"]).read_bytes()) for b in manifest["blocks"]]


def load_fresc():
    return Program("FRESC", code_sections(FRESC_DIR), JUKEBOX2_ENTRY, {})


def load_jrm(path):
    """A built main program (CODE container, scripts/c55x_build_firmware.py): its records as
    sections; listing instructions of Jukebox2 where the bytes are unchanged, everything
    else is decoded when it runs."""
    image, records, offset = Path(path).read_bytes(), [], 12
    if image[:4] != b"CODE":
        raise ValueError(f"{path}: not a CODE container")
    end = 8 + int.from_bytes(image[4:8], "big")
    while offset < end:
        addr, size = int.from_bytes(image[offset:offset + 4], "big"), int.from_bytes(image[offset + 4:offset + 8], "big")
        records.append((addr, image[offset + 12:offset + 12 + size]))
        offset += 12 + size
    base = load_jukebox2()
    old = {}
    for addr, data in base.sections:
        for i, byte in enumerate(data):
            old[addr + i] = byte
    changed = {addr + i for addr, data in records for i, byte in enumerate(data) if old.get(addr + i) != byte}
    insns = {a: v for a, v in base.insns.items() if not any(a + k in changed for k in range(6))}
    return Program(Path(path).name, records, base.entry, insns, {}, base.functions, base.bios_idle,
                   base.poll_loops)


def load_program(name):
    """'jukebox2', 'fresc', 'hybrid' / 'hybrid:DIR', 'jrm:PATH' (built CODE container) or a
    COFF path."""
    if name == "jukebox2":
        return load_jukebox2()
    if name == "fresc":
        return load_fresc()
    if name.startswith("jrm:"):
        return load_jrm(name[len("jrm:"):])
    if name == "hybrid" or name.startswith("hybrid:"):
        return load_hybrid(Path(name[len("hybrid:"):]) if ":" in name else HYBRID_DIR)
    return load_coff(name)


def load_hybrid(directory):
    info = json.loads((directory / "hybrid.json").read_text())
    base, c_code = load_jukebox2(), load_coff(directory / "hybrid.out")
    patches = [(a, bytes.fromhex(h)) for a, h in info["patches"]]
    insns = {**base.insns, **c_code.insns}
    for addr, data in patches:
        for a in range(addr, addr + len(data)):
            insns.pop(a, None)
    funcs = sorted(base.functions + [(a, f"{n} [C]") for n, a in info["c_functions"].items()])
    return Program("Jukebox2+C", base.sections + c_code.sections + patches, base.entry, insns,
                   c_code.symbols, funcs, base.bios_idle, base.poll_loops)


def load_jukebox2():
    sections = code_sections(CODE_DIR)
    insns = {}
    for f in sorted(LISTING_DIR.glob("block_*.asm")):
        insns.update(listing.parse(f.read_text(errors="surrogateescape")))
    funcs = []      # names are for reports only; without analysis/symbols.json there are none
    if SYMBOLS.exists():
        symbols = json.loads(SYMBOLS.read_text())["symbols"]
        funcs = sorted((int(k, 16), v["name"]) for k, v in symbols.items() if v.get("kind") == "function")
    return Program("Jukebox2", sections, JUKEBOX2_ENTRY, insns, {}, funcs, JUKEBOX2_IDLE,
                   JUKEBOX2_POLL)


def parse_load(text):
    """--load ADDR:FILE -> (byte address, file bytes); an argparse type"""
    addr, _, name = text.partition(":")
    if not name:
        raise argparse.ArgumentTypeError("expected ADDR:FILE")
    try:
        return int(addr, 0), Path(name).read_bytes()
    except (ValueError, OSError) as e:
        raise argparse.ArgumentTypeError(f"{text}: {e}")
