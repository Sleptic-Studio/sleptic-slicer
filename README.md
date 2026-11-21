# Sleptic Slicer

Beta release, 2026.

**A source-available slicer for resin (MSLA) 3D printers, made by
Sleptic.** The source code is public: read it, build it, change it for
your own use and contribute to it. Publishing, redistributing or reusing
it elsewhere generally needs Sleptic's permission, subject to contribution,
platform and other preserved rights
([Sleptic Source License](LICENSE.md)).

Public source for personal use, education, and printing; strictly protected against competitor extraction and commercial redistribution.

Sleptic Slicer takes your models from file to printer: it repairs and
orients them, generates supports, slices them into layers and exports a job
your printer can run. It is built to be clear for newcomers and complete for
experienced resin printers.

> **Status:** beta (0.5.0). Windows is the platform it is built and tested
> on today; expect rough edges and please report them.

## About Sleptic

Sleptic is building toward the first commercial, affordable
**multi-material, multi-colour resin 3D printer**: one that can print any
number of colours, shades and gradients in a single part.

Sleptic Slicer is the first step. Today it is a full slicer for ordinary
resin printers, filling a gap in the resin slicer landscape with a tool
anyone can use, study and help improve. As Sleptic's own printers come
together, the slicer will grow the features their multi-material and
multi-colour systems need.

## What it does

- **Auto prepare:** one click orients each part, arranges the plate and
  generates supports tuned to your models.
- **Supports:** supports placed where the part needs them, with angled
  arms, bracing for tall supports, and a choice of rafts including a lattice
  raft that peels off easily. Supports can be added and removed by hand.
- **Mesh repair:** broken meshes (flipped faces, holes, duplicate or
  degenerate faces) are checked and repaired, with undo.
- **Hollowing and drain holes** for large solid parts.
- **Several build plates** in one project.
- **Preview:** a layer slider with a 2D layer view and a 3D view of the
  sliced stack, plus checks for print risks such as islands, with the layer
  where each one is.
- **Estimates:** print time, resin volume and cost.
- **Printer and resin profiles:** 93 printers from Anycubic, Elegoo,
  Phrozen, Creality, UniFormation and Concepts 3D built in and grouped by
  brand and series, a generic printer, and editable resin profiles with
  calibration values per printer. Writing each brand's own job file
  format is in progress; until then these printers are profiles for
  preparing and slicing. Profiles can be forked, extended and
  shared as packages where permitted by the license.
- **Calibration tools:** exposure and overhang tests, LCD orientation,
  XY scale and Z checks, and print time calibration from a print log.
- **Projects** saved as 3MF with their settings, automatic recovery after a
  crash, recipes and batch preparation, and a print history.
- **Exports:** a printer job (layer images plus G-code from the printer
  profile's templates, so a custom printer needs only a profile), SVG or PNG
  layers, and STL.
- **A command line slicer** (`sc-slicer-cli`) for scripted slicing, mesh
  conversion and repair.

## Getting started

Download the Windows installer from this repository's
[Releases page](https://github.com/Sleptic-Studio/sleptic-slicer/releases).
The installer and the app ask you to accept the license before use. To
build Sleptic Slicer from source instead, see
[docs/BUILDING.md](docs/BUILDING.md).

## Support Sleptic

Sleptic is self-funded. Donations go toward Sleptic's multi-material,
multi-colour printer and keep the slicer free to use. Sleptic's website,
with the full story and a way to donate, is coming soon.

## Contributing

Bug reports, ideas, translations, documentation and code are all welcome.
Please read [CONTRIBUTING.md](CONTRIBUTING.md) first. Contributions of
code, documentation or assets need the
[Contributor License Agreement](CLA.md), which a bot asks you to sign on
your first pull request. Everyone taking part is expected to
follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Sleptic Slicer is **source-available**, not open source in the sense of
the Open Source Definition. Its own code is licensed under the
[Sleptic Source License](LICENSE.md):

| You may | Only with Sleptic's written permission |
|---|---|
| Read, download and study the source code | Publish or share source, except as permitted for contributions or by platform terms and other preserved rights |
| Run official releases, at home or at work | Publish a modified version, build or installer |
| Make and sell printed parts with it | Share or host official installers yourself (link to them instead) |
| Build unchanged source privately; individuals may modify for personal use | Reuse its code in other software, including other slicers |
| Fork and share changes to prepare contributions as sections 3.6 and 3.11 permit | Run a public hosted service |
| Contribute fixes and features | Sell or sublicense the code |

Organizations may modify only to prepare contributions intended for the
official project or with written authorization. Internal network access by
an organization's employees and contractors is permitted. GitHub's terms
and earlier and third-party license rights are not overridden.

Writing your own software that does similar things, without copying
Sleptic's code, is not restricted. Your models, exported files and prints
are yours.

Parts of the repository come from others and keep their own licenses;
[COPYRIGHT.md](COPYRIGHT.md) lists them. Releases you received under an
earlier license keep that license.

Official releases are the releases Sleptic itself publishes, unchanged,
on this repository's Releases page, on Sleptic's website, or on another
channel Sleptic names as official. Sleptic's names and logos are covered
by the [Trademark Policy](TRADEMARK_POLICY.md).

## Credits

Sample 3MF files from the 3MF Consortium are used in tests under the BSD
2-Clause licence.
