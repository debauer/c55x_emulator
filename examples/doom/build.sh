#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Build a program of the Doom port: examples/doom/build.sh smoke|doom
# -> build/doom/<name>/<name>.out (+ .map). Needs the TI C55x tools 4.4.1 (ZEN_TI_BIN or PATH; ZEN_TI_DIR if bin/ is not next to include/ and lib/).
#   smoke  tests the tool chain: rts55x.lib start-up, 32-bit math, initialised data, malloc
#   doom   Doom (doomgeneric): fetches and converts the sources first (fetch.sh), see README.md
# Objects are rebuilt when their source is newer; ZEN_JOBS (default 2) compiles in parallel.
set -eu
NAME=${1:?usage: examples/doom/build.sh <smoke|doom>}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TI=${ZEN_TI_BIN:-$(dirname "$(command -v cl55 || echo "$ROOT/build/ti-cgt-c5500-4.4.1/bin/cl55")")}
EX=$ROOT/examples
HERE=$EX/doom
P=$HERE/port
WORK=$ROOT/build/doom/work
OUT=$ROOT/build/doom/$NAME
# include/ and lib/ (rts55x.lib) sit next to bin/ of the TI tools; ZEN_TI_DIR names that directory
# if the tools were copied or linked elsewhere
if [ -n "${ZEN_TI_DIR:-}" ]; then
    TIDIR=$ZEN_TI_DIR
else
    TIDIR=$TI/..
    [ -f "$TIDIR/lib/rts55x.lib" ] || TIDIR=$(dirname "$(dirname "$(readlink -f "$TI/cl55")")")
fi
[ -f "$TIDIR/lib/rts55x.lib" ] || { echo "rts55x.lib not found in $TIDIR/lib: set ZEN_TI_DIR to the TI tools directory (with bin, include, lib)" >&2; exit 1; }
[ -x "$TI/cl55" ] || { echo "cl55 not found in $TI (set ZEN_TI_BIN)" >&2; exit 1; }
[ -d "$HERE/$NAME" ] || [ "$NAME" = doom ] || { echo "no program $NAME" >&2; exit 1; }
mkdir -p "$OUT"
cd "$OUT"

CF="-q --memory_model=large -O2 -vcpu:2.2 --symdebug:none"
# the Doom sources are compiled with int = long (port/zen_types.h)
DF="$CF --preinclude=$P/zen_types.h -I$P/include -I$P -I$WORK -I$TIDIR/include -DCMAP256 -DDOOMGENERIC_RESX=320 -DDOOMGENERIC_RESY=200"
NF="$CF -I$EX/common -I$EX/include -I$P -I$TIDIR/include"

# compile_if_newer FLAGS SOURCE: writes <basename>.obj
compile_if_newer() {
    flags=$1; src=$2
    obj=$(basename "$src" .c).obj
    if [ ! -f "$obj" ] || [ "$src" -nt "$obj" ] || [ "$P/zen_types.h" -nt "$obj" ]; then
        "$TI/cl55" $flags -c "$src" --output_file="$obj"
    fi
}

OBJS=""
NATIVE="$EX/common/lcd.c $EX/common/font.c $EX/common/i2c.c $EX/common/irq.c $EX/common/mcu.c"
if [ "$NAME" = doom ]; then
    [ -f "$WORK/d_main.c" ] && [ "$WORK/d_main.c" -nt "$HERE/doom-zen.patch" ] || "$HERE/fetch.sh"
    SRCS=$(cat "$HERE/doom_sources.txt")
    JOBS=${ZEN_JOBS:-2}
    for f in $SRCS; do
        OBJS="$OBJS $f.obj"
    done
    for f in $SRCS; do                  # the stale ones, compiled in parallel
        if [ ! -f "$f.obj" ] || [ "$WORK/$f.c" -nt "$f.obj" ] || [ "$P/zen_types.h" -nt "$f.obj" ]; then
            echo "$TI/cl55 $DF -c $WORK/$f.c --output_file=$f.obj"
        fi
    done | xargs -P "$JOBS" -I{} sh -c '{}'
    for f in zen_stubs zen_libc zen_wad zen_platform zen_video zen_main; do
        compile_if_newer "$DF" "$P/$f.c"
        OBJS="$OBJS $f.obj"
    done
    NATIVE="$NATIVE $P/zen_hw_if.c"
else
    for src in "$HERE/$NAME"/*.c; do
        compile_if_newer "$NF" "$src"
        OBJS="$OBJS $(basename "$src" .c).obj"
    done
fi
for src in $NATIVE; do
    compile_if_newer "$NF" "$src"
    OBJS="$OBJS $(basename "$src" .c).obj"
done
for src in "$HERE/reset.asm" "$EX/common/vectors.asm"; do
    obj=$(basename "$src" .asm).obj
    "$TI/asm55" -q --memory_model=large "$src" "$obj" > "$obj.log" 2>&1 || { cat "$obj.log"; exit 1; }
    OBJS="$OBJS $obj"
done
rm -f "$NAME.out"
python3 "$HERE/linkcmd.py" "$NAME.cmd" $OBJS
"$TI/lnk55" -q $OBJS "$NAME.cmd" -i "$TIDIR/lib" -l rts55x.lib -o "$NAME.out" -m "$NAME.map" 2>&1 \
    | grep -v "warning" | grep -v "^   matches are consumed" | grep -v "^$" || true
[ -f "$NAME.out" ] || { echo "link failed" >&2; exit 1; }
echo "built $OUT/$NAME.out"
