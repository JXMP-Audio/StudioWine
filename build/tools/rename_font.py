#!/usr/bin/env python3
"""rename_font.py IN OUT --family NAME [--subfamily STYLE]

Write a copy of IN whose name table carries the family NAME, so DirectWrite and
GDI find it under that name. RIBBI styles (Regular/Bold/Italic/Bold Italic) use
name IDs 1/2 only; any other style follows the Windows convention:
ID 1 = "NAME STYLE", ID 2 = "Regular", ID 16 = NAME, ID 17 = STYLE.
"""
import argparse

from fontTools.ttLib import TTFont

RIBBI = {"Regular", "Bold", "Italic", "Bold Italic"}
NAME_IDS = (1, 2, 3, 4, 6, 16, 17)


def rename(src: str, dst: str, family: str, style: str) -> None:
    font = TTFont(src)
    name = font["name"]
    full = family if style == "Regular" else f"{family} {style}"
    postscript = family.replace(" ", "") + "-" + style.replace(" ", "")
    if style in RIBBI:
        values = {1: family, 2: style}
    else:
        values = {1: full, 2: "Regular", 16: family, 17: style}
    values.update({3: f"StudioWine substitute: {postscript}", 4: full, 6: postscript})

    name.names = [n for n in name.names if n.nameID not in NAME_IDS]
    for name_id, value in values.items():
        name.setName(value, name_id, 3, 1, 0x409)  # Windows, Unicode BMP, en-US
        name.setName(value, name_id, 1, 0, 0)      # Mac, Roman, English
    if "CFF " in font:
        font["CFF "].cff.fontNames = [postscript]
    font.save(dst)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("dst")
    ap.add_argument("--family", required=True)
    ap.add_argument("--subfamily", default="Regular")
    a = ap.parse_args()
    rename(a.src, a.dst, a.family, a.subfamily)


if __name__ == "__main__":
    main()
