#!/usr/bin/env python3
# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
"""Measure test coverage with cargo-tarpaulin and list files worst first.

    python3 scripts/coverage.py

Needs cargo-tarpaulin 0.34 or later (`cargo install cargo-tarpaulin`); its
settings are in tarpaulin.toml. The run's log and JSON report go to
target/coverage/. Without a GPU the mesh_render tests skip themselves and that
area reads lower than it is.
"""
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/coverage"
# "|| libs/x/src/y.rs: 12/40" with an optional change since the last run.
ROW = re.compile(r"^\|\| ((?:apps|libs)/\S+\.rs): (\d+)/(\d+)(?: [+-][\d.]+%)?$")


def main():
    if not shutil.which("cargo-tarpaulin"):
        sys.exit("cargo-tarpaulin is not installed: cargo install cargo-tarpaulin")
    OUT.mkdir(parents=True, exist_ok=True)
    log = OUT / "tarpaulin-run.log"
    with open(log, "w", encoding="utf-8") as fh:
        subprocess.run(["cargo", "tarpaulin", "--config", "tarpaulin.toml"], cwd=ROOT, stdout=fh,
                       stderr=subprocess.STDOUT, check=True)
    rows = []
    for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
        m = ROW.match(line)
        # Integration tests and packaged copies are not ours to cover.
        if m and "/tests/" not in m[1] and "target/package" not in m[1] and int(m[3]):
            rows.append((100 * int(m[2]) / int(m[3]), int(m[2]), int(m[3]), m[1]))
    for share, hit, total, path in sorted(rows):
        print(f"{share:6.1f}%  {hit:5}/{total:<5}  {path}")
    print(f"\nlog: {log}\njson: {OUT / 'tarpaulin-report.json'}")


if __name__ == "__main__":
    main()
