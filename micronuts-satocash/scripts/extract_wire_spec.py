#!/usr/bin/env python3
"""Extract the SatoCash wire-format spec from Satocash.java doc comments.

The extracted blocks land in SPEC-wire.md verbatim (one section per
command). micronuts-satocash pins its Rust encoders to those blocks with
greatspectations (`specquotes.toml`, source SATO) — when Toporin changes
a wire format upstream, `greatspectate check` fails here first.

Usage:
  python3 scripts/extract_wire_spec.py <path-to-Satocash.java> > SPEC-wire.md

Pinned source of truth:
  https://github.com/Toporin/Satocash-Applet
  master @ ee413cc702f79e53f2238d5efce5cfc94a498a1e (2025-08-29)
"""

import re
import sys

IMPLEMENTED_INS = {
    "0x3c": "GET_STATUS",
    "0xb0": "SATOCASH_GET_STATUS",
    "0xb1": "IMPORT_MINT",
    "0xb2": "EXPORT_MINT",
    "0xb4": "IMPORT_KEYSET",
    "0xb7": "IMPORT_PROOF",
    "0xb8": "EXPORT_PROOFS",
}


def command_block(doc_comment: str) -> str | None:
    """The `ins:`..`return:`(incl. wrapped continuation) slice, or None."""
    lines = [re.sub(r"^\s*\*\s?", "", ln).strip() for ln in doc_comment.splitlines()]
    lines = [ln for ln in lines if ln and ln != "*/"]
    out, started, done = [], False, False
    for ln in lines:
        if re.match(r"ins:", ln):
            started = True
        if not started:
            continue
        if done and re.match(r"(Exceptions:|ins:|p1:|p2:|data:|return:)", ln):
            break
        out.append(ln)
        if re.match(r"return:", ln):
            done = True
    return "\n".join(out) if done else None


def extract(src: str) -> list[tuple[str, str]]:
    blocks = []
    for m in re.finditer(r"/\*\*(.*?)\*/", src, re.S):
        body = m.group(1)
        ins = re.search(r"ins:\s*(0x[0-9A-Fa-f]{2})\b", body)
        block = command_block(body)
        if not ins or not block:
            continue
        key = ins.group(1).lower()
        if key in IMPLEMENTED_INS:
            blocks.append((key, block))
    return blocks


def main() -> None:
    src = open(sys.argv[1], encoding="utf-8").read()
    print("# SatoCash wire spec (extracted)")
    print()
    print("Verbatim command blocks from `src/main/java/org/satocash/applet/Satocash.java`")
    print("(Toporin/Satocash-Applet master @ ee413cc702f79e53f2238d5efce5cfc94a498a1e).")
    print("Regenerate: `python3 scripts/extract_wire_spec.py <Satocash.java> > SPEC-wire.md`.")
    print()
    for ins, block in extract(src):
        print(f"## {IMPLEMENTED_INS[ins]} ({ins})")
        print()
        print(block)
        print()


if __name__ == "__main__":
    main()
