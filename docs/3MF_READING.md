# Reading 3MF and profile packages safely

A 3MF file (and a Sleptic profile package) is a ZIP archive. A file that two
programs read differently is at best broken and at worst crafted to show one
model to a checker and another to the printer, so the reader refuses
anything that could be read two ways, and stays lenient with harmless
oddities that other slicers produce. Introduced in R238.

## The ZIP archive (`libs/3mf/src/zip_check.rs`)

Checked before anything is decompressed, for 3MF files and profile packages
alike (`sc_3mf::zip_check::check_zip_structure`):

- exactly one end-of-directory record, closing the file (a second one that
  would also fit, for example hidden in the first one's comment, is refused);
- one disk only: archives that claim to span several are refused;
- the ZIP64 record and locator, when present, sit right before the end
  record, and the file index ends exactly where they begin;
- every entry has a local header that agrees with the index on its name,
  flags, compression method, CRC and sizes (or, with a data descriptor, the
  descriptor after the data does);
- the listed entries cover the file from its first byte to the index with
  no gap and no overlap: a hidden, unlisted entry, junk before the archive,
  or a second archive glued on is refused;
- no entry is encrypted, no two share a name, and no Unicode Path extra
  field names an entry differently from its header;
- at most 100,000 entries.

The `zip` crate is pinned to one exact version (`=9.7.3` in the workspace
`Cargo.toml`); a newer one comes in only after review of its advisories
(https://github.com/zip-rs/zip2/security/advisories). Our own checks above
cover the reading-side advisories of the versions before it (dual headers,
fake end records, ZIP64 offsets, unlisted local entries, the Unicode Path
case of GHSA-fpwh-q2m5-vfm2). The library's extraction-to-disk, encryption
and stream-reader code, where most of its advisories are, is not used.

## The package (OPC) layer (`libs/3mf/src/package.rs`)

- Every name and declared size is checked before any part is decompressed;
  a part may not exceed 2 GB, the package 4 GB, and `[Content_Types].xml`
  or a relationship part 16 MB (they are parsed whole).
- Part names compare without regard to ASCII case, as OPC requires: content
  type overrides, relationship parts and the start part match whatever the
  case, and two parts whose names differ only in case are refused.
- A part with no content type is ignored with a warning (other programs'
  extra files often are; the formats say to ignore them). Naming such a
  part as the model still fails.
- Interleaved parts (`<part>/[0].piece` … `<part>/[n].last.piece`) are
  joined; missing, repeated or misordered pieces are refused.
- Exactly one StartPart relationship, internal: two of them, or one with
  `TargetMode="External"`, are refused.
- A part that decompresses to a size other than the declared one is
  refused.

Warnings reach the user as `LoadThreeMfWarning::UntypedPartIgnored` (stderr
in the CLI, the load warnings in the GUI). Error messages for damaged
archives say what is wrong in plain words and keep the library's detail in
brackets, for bug reports.

## Pictures

Printer pictures and thumbnails are decoded one at a time behind
`safe_decode::decode_guarded`: a decoder panic on one malformed file skips
that picture only, instead of every printer picture (or the window).

## Conformance suites (R243)

`libs/mesh-formats/tests/conformance.rs` (ignored; set `SC_3MF_SUITES` to a clone of
https://github.com/3MFConsortium/test_suites) reads all 2,271 files of v2.4.1 and writes
`local/conformance/results.tsv`. As of R243:

- no file panics or hangs;
- core (suite 3): all 133 valid files open; 26 of 42 invalid ones are refused (1 more as needing
  an unsupported extension);
- materials (suites 2 and 6): all 770 valid files open. A resin print has one material, so a file
  that requires the materials extension is read for its geometry, its colours and materials are
  dropped, and the reader warns (`MaterialsExtensionIgnored`). Invalid files whose only fault is in
  material data (101) open as well, for the same reason: that data is never used;
- production (suite 5): 163 of 184 valid files open; the rest require the 2021 "alternatives"
  sub-extension and are refused as unsupported. A non-root model part's own unit is ignored, as the
  production extension says: the root model's unit holds;
- every valid file that requires another unsupported extension (beam lattice, secure content,
  booleans, displacement) is refused with the unsupported-extension error, as the format requires
  of a reader without that extension;
- valid slice-extension files (suites 1 and 4) whose objects carry only a low-resolution stand-in
  mesh are refused on purpose: their real shape is only in the slices, which are not loaded yet.

Refused, from the suite: a content type declared twice (N_*_0205), an empty Default extension
(0206), an Override that names no valid part (0207), relationship parts without exactly the
relationships content type, parameters included (0204_01, 0404_03), a relationship part for a part
that is absent (0407_02), relationship Ids that are not XML IDs or repeat (0405_04), thumbnails
outside the package (0403), missing (0405_01) or not an image (0404_04), `xml:space` on the model
(0409), and a property on an object made of components (0424).

Opened on purpose (15 invalid core files), each harmless or handled later:
- 0410_01: metadata with an undeclared namespace prefix. Bambu Studio writes `BambuStudio:`
  metadata so; read with a warning, and declared under a placeholder namespace when written.
- 0410_03: a metadata name given twice; the first counts (it only picks the title shown).
- 0411, 0416, 0418, 0426, 0427: degenerate triangles, negative volume, inward faces, fewer than
  four triangles, non-manifold edges: mesh diagnosis reports these and repair fixes them (the
  suite itself says a reader may ignore 0427).
- 0421: a part outside the printable area: shown as out of the volume, as any misplaced part.
- 0419: a CMYK JPEG thumbnail: thumbnails are not used.
- 0208: unescaped Unicode in part names: unambiguous, and our own writer may produce them.
- 0204_02: a thumbnail target whose case differs from its part name: OPC treats part names as
  case-insensitive, so the target does name the part.
- 0405_05: a thumbnail under an unknown relationship type: unknown types are allowed in OPC.
- 0420_01: the suite's file has no DTD in it (the case it describes); a DTD in a model is refused.

## Known gaps

- The 3MF Production extension is supported as far as splitting a build
  across model parts goes: components and (since R242) build items with
  `p:path` bring in the objects of other model parts. Its newer
  "alternatives" sub-extension (2021) and UUID rules are not; files that
  require them are refused with "unsupported 3MF extension".
- Only one OPC/3MF revision's rules are applied; files valid under one
  revision and read differently under another are not yet detected.
- The 3MF Consortium's full conformance suite (about 2,300 files) is not
  run in CI yet; the samples in `libs/*/tests/fixtures/official` are.
