#!/usr/bin/env python3
# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
"""Write the AppStream metainfo a package ships, with its release filled in.

    python3 scripts/render_metainfo.py <template.xml> <Cargo.toml> <output.xml>

The tracked template (packages/shared) has no <releases>; the release is
the packaged crate's version and today's date (UTC), added at build time,
so there is never a second copy of the version to forget to bump.
"""
import datetime
import re
import sys
import tomllib
from pathlib import Path


def main(template, manifest, output):
    text = Path(template).read_text(encoding="utf-8")
    if "<releases" in text:
        sys.exit(f"render_metainfo: {template} must not contain <releases>; it is added per build")
    if "</component>" not in text:
        sys.exit(f"render_metainfo: {template} has no </component>")
    version = tomllib.loads(Path(manifest).read_text(encoding="utf-8")).get("package", {}).get("version")
    if not isinstance(version, str):
        sys.exit(f"render_metainfo: {manifest} has no [package] version")
    day = datetime.datetime.now(datetime.timezone.utc).date().isoformat()
    release = f'  <releases>\n    <release version="{version}" date="{day}"/>\n  </releases>\n'
    head, tail = re.split(r"(?=</component>)", text, maxsplit=1)
    Path(output).write_text(head + release + tail, encoding="utf-8")
    print(f"render_metainfo: {output} ({version}, {day})")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
