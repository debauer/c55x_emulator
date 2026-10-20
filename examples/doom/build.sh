#!/bin/sh
# Build a program of the Doom port: examples/doom/build.sh smoke|doom
# -> build/doom/<name>/<name>.out (+ .map, .dis). Needs the TI C55x tools (ZEN_TI_BIN or PATH).
set -eu
NAME=${1:?usage: examples/doom/build.sh <smoke|doom>}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TI=${ZEN_TI_BIN:-$(dirname "$(command -v cl55 || echo "$ROOT/build/ti-cgt-c5500-4.4.1/bin/cl55")")}
EX=$ROOT/examples
OUT=$ROOT/build/doom/$NAME
[ -x "$TI/cl55" ] || { echo "cl55 not found in $TI (set ZEN_TI_BIN)" >&2; exit 1; }
[ -d "$EX/doom/$NAME" ] || { echo "no program $NAME" >&2; exit 1; }
mkdir -p "$OUT"
cd "$OUT"
OBJS=""
for SRC in "$EX/doom/$NAME"/*.c "$EX"/common/lcd.c "$EX"/common/font.c "$EX"/common/i2c.c \
           "$EX"/common/irq.c "$EX"/common/mcu.c; do
    OBJ=$(basename "$SRC" .c).obj
    "$TI/cl55" -q --memory_model=large -O2 -vcpu:2.2 --symdebug:none \
        --include_path="$EX/common" --include_path="$EX/include" --include_path="$TI/../include" \
        -c "$SRC" --output_file="$OBJ"
    OBJS="$OBJS $OBJ"
done
for SRC in "$EX/doom/reset.asm" "$EX/common/vectors.asm"; do
    OBJ=$(basename "$SRC" .asm).obj
    "$TI/asm55" -q --memory_model=large "$SRC" "$OBJ" > "$OBJ.log" 2>&1 || { cat "$OBJ.log"; exit 1; }
    OBJS="$OBJS $OBJ"
done
"$TI/lnk55" -q $OBJS "$EX/doom/doom.cmd" -i "$TI/../lib" -l rts55x.lib -o "$NAME.out" -m "$NAME.map"
"$TI/dis55" -q -r -g "$NAME.out" > "$NAME.dis"
echo "built $OUT/$NAME.out"
