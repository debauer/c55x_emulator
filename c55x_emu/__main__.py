"""Headless run: python3 -m c55x_emu build/hello_mcu/hello_mcu.out --event 0x11 --ascii

Runs --steps instructions, then for each --event queues the MCU event byte and runs
--steps again."""

import argparse
import sys
from pathlib import Path

from .cpu import EmulationError
from .lcd import HEIGHT, WIDTH
from .diskimage import open_disk
from .loader import load_program
from .machine import Machine
from .png import write_png
from .wav import WavRecorder


def ascii_art(rows):
    out = []
    for y in range(0, HEIGHT, 2):
        pair = (rows[y], rows[y + 1] if y + 1 < HEIGHT else [0] * WIDTH)
        out.append("".join(" .'#"[a * 2 + b] for a, b in zip(*pair)))
    return "\n".join(out)


def parse_load(text):
    """ADDR:FILE -> (byte address, file bytes)"""
    addr, _, name = text.partition(":")
    if not name:
        raise argparse.ArgumentTypeError("expected ADDR:FILE")
    try:
        return int(addr, 0), Path(name).read_bytes()
    except (ValueError, OSError) as e:
        raise argparse.ArgumentTypeError(f"{text}: {e}")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("image", help="C55x COFF executable (.out), 'jukebox2', 'fresc' or 'hybrid' (Jukebox2 with the reconstructed C, scripts/c55x_hybrid.py)")
    ap.add_argument("--steps", type=int, default=5_000_000)
    ap.add_argument("--seconds", type=float, help="stop at this emulated time (no step limit then)")
    ap.add_argument("--png", help="write the LCD contents to this PNG file")
    ap.add_argument("--ascii", action="store_true", help="print the LCD as text")
    ap.add_argument("--event", action="append", default=[], type=lambda s: int(s, 0),
                    help="MCU event byte to inject (repeatable)")
    ap.add_argument("--disk", help="disk image file (a missing file is created as a synthetic image; "
                                   "default: synthetic, in memory)")
    ap.add_argument("--trace", type=int, default=0, metavar="N",
                    help="on a stop, list the last N executed instructions")
    ap.add_argument("--mcu-reg", action="append", default=[], metavar="REG=VALUE",
                    help="MCU register value, e.g. 1=0x4000 (repeatable)")
    ap.add_argument("--load", action="append", default=[], type=parse_load, metavar="ADDR:FILE",
                    help="copy FILE to this byte address before the start (repeatable)")
    ap.add_argument("--wav", help="record the audio McBSP0 sends to the codec (16-bit stereo WAV)")
    args = ap.parse_args()
    regs = {int(k, 0): int(v, 0) for k, v in (r.split("=") for r in args.mcu_reg)}
    program = load_program(args.image)
    m = Machine(program, regs, disk=open_disk(args.disk) if args.disk else None)
    for addr, data in args.load:
        m.bus.load(addr, data)
    if args.trace:
        m.cpu.core.set_trace(args.trace)
    if args.wav:
        m.audio_out = WavRecorder(args.wav)
    steps = 10**15 if args.seconds is not None else args.steps
    try:
        idle = m.run(steps, until_seconds=args.seconds)
        for ev in args.event:
            m.mcu.inject(ev)
            idle = m.run(steps, until_seconds=args.seconds)
    except EmulationError as e:
        for pc in m.cpu.core.trace():
            insn = m.code.insns.get(pc)
            print(f"  0x{pc:06x} {m.program.where(pc):32} {insn.text if insn else '?'}")
        sys.exit(f"emulation stopped in {m.where()}: {e} (after {m.cpu.cycles} instructions)")
    stop = "idle loop" if idle else ("time limit" if args.seconds is not None else "step limit")
    print(f"{stop} at 0x{m.cpu.pc:06x} ({m.where()}) after "
          f"{m.cpu.cycles} instructions, {m.code.decoded_windows} windows decoded on demand")
    if m.cpu.interrupts_taken:
        print("interrupts taken: " + ", ".join(f"{n} x{c}" for n, c in sorted(m.cpu.interrupts_taken.items())))
    print(f"LCD: display {'on' if m.lcd.on else 'off'}, contrast {m.lcd.contrast}, "
          f"unknown accesses {m.lcd.unknown or 'none'}")
    if m.bus.unmodeled:
        print("unmodeled I/O ports: " + ", ".join(f"0x{p:04x} x{n}" for p, n in sorted(m.bus.unmodeled.items())))
    print(f"disk: {m.disk.reads} sectors read, {m.disk.writes} written; ATA commands "
          + ", ".join(f"0x{c:02x} x{n}" for c, n in sorted(m.ata.commands.items())))
    if m.ata.unknown:
        print("ATA not modeled: " + "; ".join(f"{t} x{n}" for t, n in m.ata.unknown.items()))
    st = m.cpu.core.stats()
    print(f"DMA blocks per channel {st['dma_blocks']}, McBSP underruns {st['mcbsp_underruns']}; "
          f"codec: {m.codec.describe()}")
    if m.audio_out is not None:
        m.audio_out.close()
        print(f"audio: {m.audio_out.frames} frames -> {', '.join(map(str, m.audio_out.files)) or 'none'}")
    m.disk.flush()
    if m.mcu.log:
        print("MCU: " + "; ".join(m.mcu.log))
    if m.i2c.anomalies:
        print("I2C anomalies: " + "; ".join(f"{t} x{n}" for t, n in m.i2c.anomalies.items()))
    rows = m.lcd.rows()
    if args.ascii:
        print(ascii_art(rows))
    if args.png:
        write_png(args.png, rows, scale=3)
        print(f"wrote {args.png}")


if __name__ == "__main__":
    main()
