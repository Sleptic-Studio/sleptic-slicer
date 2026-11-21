#!/usr/bin/env python3
# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
"""Refresh the GUI's translation templates (i18n/*.pot) from the source.

    python3 apps/slicer-gui/scripts/extract_strings.py

Needs slint-tr-extractor (for the .slint files) and GNU gettext's xgettext and
msgcat (for tr!/ctr! in Rust). Three templates are built (Slint, Rust without
context, Rust with context) and merged into sc-slicer-gui.pot. A template is
only rewritten when its strings changed: the creation date and the source
line references alone never count, so an unchanged run leaves the files and
git alone.
"""
import subprocess
import sys
from pathlib import Path

GUI = Path(__file__).resolve().parents[1]
I18N = GUI / "i18n"


def comparable(text):
    return [l for l in text.splitlines() if not l.startswith(('"POT-Creation-Date:', "#: "))]


def produce(name, command):
    """Run `command` writing to a scratch file, then keep it only if it differs."""
    target = I18N / name
    scratch = target.with_name(name + ".new")
    subprocess.run([*command, "-o", str(scratch)], cwd=GUI, check=True)
    if target.exists() and comparable(target.read_text(encoding="utf-8")) == comparable(scratch.read_text(encoding="utf-8")):
        scratch.unlink()
    else:
        scratch.replace(target)


def main():
    slint = sorted(str(p.relative_to(GUI)) for p in (GUI / "ui").rglob("*.slint"))
    rust = sorted(str(p.relative_to(GUI)) for p in (GUI / "src").rglob("*.rs"))
    xgettext = ["xgettext", "-L", "Rust", "--from-code=UTF-8"]
    try:
        produce("from-slint.pot", ["slint-tr-extractor", *slint])
        produce("from-rust-without-context.pot", [*xgettext, "--keyword=tr!:1", "--keyword=tr!:1,2", *rust])
        produce("from-rust-with-context.pot", [*xgettext, "--keyword=ctr!:1c,2", "--keyword=ctr!:1c,2,3", *rust])
        parts = [str(I18N / n) for n in ("from-slint.pot", "from-rust-without-context.pot", "from-rust-with-context.pot")
                 if (I18N / n).exists()]
        produce("sc-slicer-gui.pot", ["msgcat", "--use-first", *parts])
    except FileNotFoundError as err:
        sys.exit(f"extract_strings: a tool is missing: {err.filename}")


if __name__ == "__main__":
    main()
