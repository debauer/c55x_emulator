#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Mechanical source changes for the C55x (16-bit int and char), applied to a copy of the
doomgeneric sources: transform.py SRC_DIR WORK_DIR. Hand-made changes are in doom-zen.patch."""

import re
import shutil
import sys
from pathlib import Path

# bare `unsigned` means unsigned int, which zen_types.h turns into 32 bits only when spelled out
BARE_UNSIGNED = re.compile(r"\bunsigned\b(?!\s+(?:char|short|long|int)\b)")
SHORT_INT = re.compile(r"\b(short|long)\s+int\b")


# Doom computes in 32 bits (1 << 16, 5 * 8192 ...): every integer literal becomes long, so the
# 16-bit int of the C55x never limits an expression. Comments, strings and #include lines
# are left alone.
TOKEN = re.compile(r"""
    (?P<skip>//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|\#\s*include[^\n]*)
  | (?P<num>(?<![\w.])(?:0[xX][0-9a-fA-F]+|\d+)(?P<suffix>[uUlL]*)(?![\w.]))
""", re.VERBOSE | re.DOTALL)


def long_literal(match):
    if match.group("skip") is not None:
        return match.group(0)
    suffix = match.group("suffix").upper()
    if "L" in suffix:
        return match.group(0)
    return match.group("num") + "L"


# The compiler's enum is a 16-bit int: an enum with values above 0x7fff (mobjflag_t) becomes a
# long typedef with one #define per member.
ENUM = re.compile(r"typedef\s+enum\s*\{(?P<body>[^{}]*)\}\s*(?P<name>\w+)\s*;")
COMMENT = re.compile(r"//[^\n]*|/\*.*?\*/", re.DOTALL)


def big_enum(match):
    members, value = [], -1
    for item in COMMENT.sub("", match.group("body")).split(","):
        item = item.strip()
        if not item:
            continue
        name, _, expr = (part.strip() for part in item.partition("="))
        try:
            value = eval(re.sub(r"(?<=\w)[uUlL]+\b", "", expr)) if expr else value + 1
        except (NameError, SyntaxError, TypeError):
            return match.group(0)           # members defined by other names: left alone
        members.append((name, value))
    if all(v <= 0x7FFF for _, v in members):
        return match.group(0)
    lines = [f"typedef long {match.group('name')};"]
    lines += [f"#define {n} {v:#x}L" for n, v in members]
    return "\n".join(lines) + "\n"


def transform(text):
    text = ENUM.sub(big_enum, text)
    text = SHORT_INT.sub(r"\1", text)
    text = BARE_UNSIGNED.sub("unsigned long", text)
    return TOKEN.sub(long_literal, text)


def main():
    src, work = Path(sys.argv[1]), Path(sys.argv[2])
    work.mkdir(parents=True, exist_ok=True)
    for path in sorted(src.iterdir()):
        if path.suffix in (".c", ".h"):
            text = path.read_bytes().decode("latin-1")
            (work / path.name).write_bytes(transform(text).encode("latin-1"))


if __name__ == "__main__":
    main()
