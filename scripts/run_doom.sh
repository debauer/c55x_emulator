#!/bin/sh
# Run Doom (examples/doom) on the emulator: scripts/run_doom.sh [--gui] WAD [emulator options]
# WAD: your own DOOM1.WAD (shareware, version 1.9; not part of this repository).
# Builds the program first. Without --gui the run is headless: pass e.g. --steps 300000000 --png doom.png
#   scripts/run_doom.sh --gui DOOM1.WAD
#   scripts/run_doom.sh DOOM1.WAD --steps 300000000 --png doom.png
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
GUI=0
if [ "${1:-}" = --gui ]; then
    GUI=1
    shift
fi
WAD=${1:?usage: scripts/run_doom.sh [--gui] DOOM1.WAD [emulator options]}
shift
[ -f "$WAD" ] || { echo "no such file: $WAD" >&2; exit 1; }
WAD=$(cd "$(dirname "$WAD")" && pwd)/$(basename "$WAD")
cd "$ROOT"
examples/doom/build.sh doom >&2
if [ "$GUI" = 1 ]; then
    if command -v uv >/dev/null 2>&1; then
        exec uv run --extra gui c55x-emu-gui build/doom/doom/doom.out --load "0xBFE000:$WAD" "$@"
    fi
    exec python3 -m c55x_emu.qt_ui build/doom/doom/doom.out --load "0xBFE000:$WAD" "$@"
fi
exec python3 -m c55x_emu build/doom/doom/doom.out --load "0xBFE000:$WAD" "$@"
