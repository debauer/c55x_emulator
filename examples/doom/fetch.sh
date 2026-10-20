#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Prepare the Doom sources: examples/doom/fetch.sh
# Clones doomgeneric (ozkl/doomgeneric, GPL-2.0) at a fixed commit into build/doom/src, converts a
# copy for the 16-bit C55x (port/transform.py) and applies doom-zen.patch -> build/doom/work.
# The Doom sources are not part of this repository.
set -eu
COMMIT=e257a09527062bfb986b6e5e0780839b6cbd5b71
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
HERE=$ROOT/examples/doom
SRC=$ROOT/build/doom/src
WORK=$ROOT/build/doom/work

if [ ! -d "$SRC/.git" ]; then
    mkdir -p "$ROOT/build/doom"
    git clone -q https://github.com/ozkl/doomgeneric "$SRC"
fi
git -C "$SRC" cat-file -e "$COMMIT^{commit}" 2>/dev/null || git -C "$SRC" fetch -q origin
git -C "$SRC" checkout -q "$COMMIT"
rm -rf "$WORK"
python3 "$HERE/port/transform.py" "$SRC/doomgeneric" "$WORK"
patch -s -p1 -d "$WORK" < "$HERE/doom-zen.patch"
echo "doom sources ready in $WORK"
