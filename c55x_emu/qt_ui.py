"""PyQt5 front end: python3 -m c55x_emu.qt_ui IMAGE [--disk PATH] [--mute] [--scale N]

IMAGE is a C55x COFF executable (.out), 'jukebox2', 'fresc' or 'hybrid'; --disk as in __main__.

Runs the CPU in timer slices paced to real time (or as fast as possible with "Fast
forward"). Tab "Player": the player's front with LCD and controls (qt_device.py; keyboard
shortcuts in qt_controls.py). Tab "Debug": fast forward, emulated time, speed, CPU state,
interrupts and disk activity (status.py) and the raw MCU panel (events, registers,
traffic); it is only refreshed while shown. The codec output plays on the system audio
device (qt_audio.py) unless --mute is given.
"""

import argparse
import sys
import time

from PyQt5.QtCore import QEvent, Qt, QTimer
from PyQt5.QtGui import QImage, QPixmap
from PyQt5.QtWidgets import (QApplication, QCheckBox, QHBoxLayout, QLabel, QLineEdit, QMainWindow,
                             QScrollArea, QTabWidget, QVBoxLayout, QWidget)

from .cpu import EmulationError
from .diskimage import open_disk
from .lcd import HEIGHT, WIDTH
from .loader import load_program, parse_load
from .machine import Machine
from .pacing import Pacer
from .qt_audio import AudioSink
from .qt_controls import Controls
from .qt_device import LCD_BACK, LCD_OFF, LCD_PIXEL, DeviceView
from .qt_mcu import McuPanel
from .status import describe

SCALE = 3                               # LCD pixels per emulated pixel
STEPS_PER_RUN = 50_000
TICK_MS = 15
TICK_BUDGET_S = 0.014                   # wall time spent emulating per timer tick (live audio needs >= 1x)


class Window(QMainWindow):
    def __init__(self, machine, audio=None, scale=SCALE):
        super().__init__()
        self.m = machine
        self.audio = audio                  # qt_audio.AudioSink or None (muted)
        machine.audio_out = audio
        self.scale = scale
        self.shown_version = None
        self.state = "running"
        self.setWindowTitle("Zen Touch")
        self.panel = McuPanel(machine.mcu)
        self.controls = Controls(machine.mcu, self, self.panel.refresh)
        self.device = DeviceView(self.controls, scale)
        self.screen = self.device.screen
        self.notice = QLabel()              # shown when the emulation stops
        self.notice.setWordWrap(True)
        self.notice.setStyleSheet("color: #b00020; font-weight: bold;")
        self.notice.hide()
        player = QWidget()
        col = QVBoxLayout(player)
        row = QHBoxLayout()
        row.addStretch()
        row.addWidget(self.device)
        row.addStretch()
        col.addLayout(row)
        col.addWidget(self.notice)
        col.addStretch()

        self.status = QLabel()
        self.status.setTextInteractionFlags(Qt.TextSelectableByMouse)
        self.status.setWordWrap(True)
        self.fast = QCheckBox("Fast forward (no real-time pacing, audio muted)")
        self.fast.toggled.connect(self._fast_toggled)
        debug = QWidget()
        dbox = QHBoxLayout(debug)
        left = QVBoxLayout()
        left.addWidget(self.fast)
        left.addWidget(self.status)
        left.addStretch()
        dbox.addLayout(left, 1)
        dbox.addWidget(self.panel)

        self.tabs = QTabWidget()
        self.tabs.addTab(player, "Player")
        scroll = QScrollArea()              # the window size follows the player, not the panels
        scroll.setWidgetResizable(True)
        scroll.setWidget(debug)
        self.tabs.addTab(scroll, "Debug")
        self.tabs.currentChanged.connect(lambda _: self.redraw())
        self.setCentralWidget(self.tabs)
        self.speed = 0.0
        self.pacer = Pacer(machine.seconds())
        self._rebase()
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.tick)
        self.timer.start(TICK_MS)
        self.redraw()

    def _fast_toggled(self, on):
        if self.audio is not None:
            self.audio.pause(on)
        self._rebase()

    def _rebase(self):
        self.pacer.rebase(self.m.seconds())
        self.speed_ref = (time.monotonic(), self.m.seconds())

    def eventFilter(self, obj, e):
        """Keyboard shortcuts at their first receiver, unless a text field has the focus."""
        if e.type() in (QEvent.KeyPress, QEvent.KeyRelease):
            focus = QApplication.focusWidget()
            if not isinstance(focus, QLineEdit) and (obj is focus or obj is self):
                if self.controls.key(e, e.type() == QEvent.KeyPress):
                    return True
        return super().eventFilter(obj, e)

    def closeEvent(self, event):
        self.timer.stop()
        if self.audio is not None:
            self.audio.close()
        self.m.disk.flush()
        super().closeEvent(event)

    def tick(self):
        now = time.monotonic()
        end = now + TICK_BUDGET_S
        target = None if self.fast.isChecked() else self.pacer.target(now)
        try:
            while time.monotonic() < end and (target is None or self.m.seconds() < target):
                idle = self.m.run(STEPS_PER_RUN, until_seconds=target)
                self.state = "idle loop" if idle else "running"
        except EmulationError as e:
            self.state = f"stopped: {e}"
            self.timer.stop()
            self.notice.setText(f"Emulation stopped: {e} (details in the Debug tab)")
            self.notice.show()
        if self.audio is not None:
            self.audio.push()
        if target is not None:
            self.pacer.lagged(self.m.seconds())                # slower than real time
        t, s = self.speed_ref
        if now - t >= 1.0:
            self.speed = (self.m.seconds() - s) / (now - t)
            self.speed_ref = (now, self.m.seconds())
        self.redraw()

    def redraw(self):
        lcd = self.m.lcd
        if lcd.version != self.shown_version:
            self.shown_version = lcd.version
            img = QImage(WIDTH, HEIGHT, QImage.Format_RGB32)
            img.fill(LCD_BACK if lcd.on else LCD_OFF)
            if lcd.on:
                dark = LCD_PIXEL.rgb()
                for y, row in enumerate(lcd.rows()):
                    for x, p in enumerate(row):
                        if p:
                            img.setPixel(x, y, dark)
            self.screen.setPixmap(QPixmap.fromImage(img).scaled(WIDTH * self.scale, HEIGHT * self.scale))
        if self.tabs.currentIndex() != 1:
            return
        self.panel.refresh()
        lines = describe(self.m, self.state)
        lines.insert(1, f"Speed: {self.speed:.2f} x real time"
                        + ("" if self.fast.isChecked() else " (paced to real time)"))
        if self.audio is not None:
            lines.insert(2, f"Audio: {self.audio.underruns} underruns" if self.audio.rate else "Audio: no output yet")
        self.status.setText("\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description="Zen Touch emulator (PyQt5)")
    ap.add_argument("image", help="C55x COFF executable (.out), 'jukebox2', 'fresc' or 'hybrid' (Jukebox2 with the reconstructed C, scripts/c55x_hybrid.py)")
    ap.add_argument("--disk", help="disk image file (a missing file is created as a synthetic "
                                   "image; see python3 -m c55x_emu.diskimage --format-cfs)")
    ap.add_argument("--load", action="append", default=[], type=parse_load, metavar="ADDR:FILE",
                    help="copy FILE to this byte address before the start (repeatable)")
    ap.add_argument("--mute", action="store_true", help="no live audio output")
    ap.add_argument("--scale", type=int, default=SCALE, help=f"LCD zoom (default {SCALE}; 2 for small screens)")
    args = ap.parse_args()
    program = load_program(args.image)
    disk = open_disk(args.disk) if args.disk else None
    app = QApplication(sys.argv[:1])
    machine = Machine(program, disk=disk)
    for addr, data in args.load:
        machine.bus.load(addr, data)
    win = Window(machine, None if args.mute else AudioSink(machine.codec.gains), args.scale)
    app.installEventFilter(win)
    win.show()
    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
