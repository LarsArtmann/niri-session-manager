#!/usr/bin/env python3
"""Realign markdown table pipes to the aligned style markdownlint MD060 wants.

Display-width aware: East Asian Wide/Fullwidth characters (emoji, CJK) count
as 2 columns, zero-width characters (variation selectors) as 0 — char-count
padding breaks MD060 on emoji-bearing tables (the 2026-09-16 lesson).

Usage: python3 scripts/md-table-realign.py FILE [FILE ...]
Only lines that belong to a table (start with `|`) outside fenced code blocks
are touched; everything else passes through unchanged. Idempotent.
"""

import sys
import unicodedata
from pathlib import Path


def display_width(text: str) -> int:
    width = 0
    for char in text:
        if unicodedata.combining(char) or char == "\ufe0f":
            continue
        width += 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
    return width


def split_row(line: str) -> list[str] | None:
    stripped = line.strip()
    if not (stripped.startswith("|") and stripped.endswith("|")):
        return None
    inner = stripped[1:-1]
    cells: list[str] = []
    current: list[str] = []
    in_code = False
    for char in inner:
        if char == "`":
            in_code = not in_code
        if char == "|" and not in_code:
            cells.append("".join(current))
            current = []
        else:
            current.append(char)
    cells.append("".join(current))
    return cells


def is_separator(cells: list[str]) -> bool:
    return all(set(c.strip()) <= set("-: ") and c.strip() for c in cells)


def render(cells: list[str], widths: list[int]) -> str:
    padded = [
        f" {c.strip()}{' ' * (w - display_width(c.strip()))} "
        for c, w in zip(cells, widths)
    ]
    return "|" + "|".join(padded) + "|"


def realign(text: str) -> str:
    lines = text.split("\n")
    out: list[str] = []
    i = 0
    in_fence = False
    while i < len(lines):
        line = lines[i]
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            out.append(line)
            i += 1
            continue
        if in_fence or split_row(line) is None:
            out.append(line)
            i += 1
            continue
        block: list[list[str]] = []
        start = i
        while i < len(lines):
            cells = split_row(lines[i])
            if cells is None:
                break
            block.append(cells)
            i += 1
        if len(block) < 2 or not any(is_separator(c) for c in block):
            out.extend(lines[start:i])
            continue
        ncols = max(len(c) for c in block)
        block = [c + [""] * (ncols - len(c)) for c in block]
        widths = [max(display_width(c[j].strip()) for c in block) for j in range(ncols)]
        for cells in block:
            out.append(render(cells, widths))
    return "\n".join(out)


def main() -> None:
    for name in sys.argv[1:]:
        path = Path(name)
        new = realign(path.read_text())
        if new != path.read_text():
            path.write_text(new)
            print(f"realigned {path}")
        else:
            print(f"already aligned {path}")


if __name__ == "__main__":
    main()
