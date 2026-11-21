# Copyright and third-party material

Beta release, 2026.


This file records who owns what in this repository and under which license
each part is used. If a file's own notice and this file disagree, the
file's notice governs, and the disagreement should be reported as a bug.

## 1. Sleptic Covered Material

Copyright (c) 2026 Sleptic.

Sleptic's own material is Covered Material under the
[Sleptic Source License](LICENSE.md), identified by its per-file notice:

```
SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
```

## 2. Third-party material in this repository

| Material | Where | Owner | License |
|---|---|---|---|
| Noto Sans outlines | `libs/print-glue/src/label_font.rs` | The Noto Project Authors | SIL Open Font License 1.1 |
| Noto Sans font | `apps/slicer-gui/assets/fonts/NotoSans-Variable.ttf` | The Noto Project Authors | SIL Open Font License 1.1 (`OFL.txt` beside it) |
| 3MF sample files | `libs/mesh-formats/tests/fixtures/official/3mf-samples/`, `libs/3mf/tests/fixtures/official/` | 3MF Consortium | BSD 2-Clause (`LICENSE.txt` beside them) |
| 3MF test suite files | `libs/mesh-formats/tests/fixtures/official/3mf-test-suites/` | 3MF Consortium | BSD 2-Clause |
| STL test models | `libs/mesh-formats/tests/fixtures/official/burkardt-stla/` | John Burkardt | GNU LGPL 3.0 (`LICENSE.txt` beside them) |
| Binary STL sample | `libs/mesh-formats/tests/fixtures/official/numpy-stl-stlb/` | Rick van Hattem | BSD-style (`LICENSE.txt` beside it) |
| Colored STL sample | `libs/mesh-formats/tests/fixtures/official/threejs-stlb-color/` | three.js authors | MIT (`LICENSE.txt` beside it) |
| Printer brand symbols | `apps/slicer-gui/ui/brands/` | SVG drawings from Simple Icons | CC0 1.0 (see section 4) |

The test files are used only by the tests and are not part of the
application.

## 3. Libraries the application uses

Sleptic Slicer is built with open source Rust libraries (crates). Their
licenses are checked automatically against an allowed list in `deny.toml`:

- MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, CC0-1.0,
  Unicode-3.0 and BSL-1.0;
- MPL-2.0, which applies file by file.

Every release carries their notices in `THIRD_PARTY_NOTICES`.

The user interface is built with **Slint**, by SixtyFPS GmbH, under the
Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0.
The About window carries the `AboutSlint` attribution.

## 4. Other companies' names and marks

Names and logos of printer and resin makers belong to their owners. That
includes the brand symbols drawn on the build plate and the brands shown
in printer and resin profiles. Sleptic Slicer uses them only to identify
the printer or resin a user has chosen. Their use does not mean those
companies endorse Sleptic Slicer.

Simple Icons' CC0 dedication covers its drawings, not the marks
themselves.

The product pictures of printers and resins were made for Sleptic Slicer.
They show the products only to identify them.

Printer specifications in the printer profiles are technical facts taken
from public sources.

## 5. Contributors

Contributors keep the copyright in their contributions. A signed
[Contributor License Agreement](CLA.md) is required before code, asset or
documentation Contributions are merged; issues and comments not intended
for inclusion are not Contributions. The
repository's history records who contributed what.

## 6. Your models and prints

Sleptic claims no rights in the models you open in Sleptic Slicer, the
files you export, or the objects you print.

## 7. Sleptic's names and logos

"Sleptic" and "Sleptic Slicer", the Sleptic logo and the application
icon are Sleptic's marks, governed by trademark law and the
[Trademark Policy](TRADEMARK_POLICY.md). The logo artwork is Covered
Material under the Sleptic Source License when identified by its license notice.
