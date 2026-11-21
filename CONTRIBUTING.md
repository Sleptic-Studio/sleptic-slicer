# Contributing to Sleptic Slicer

Beta release, 2026.


Thank you for helping. Sleptic Slicer is a resin (MSLA) slicer made by
Sleptic. Contributions of every kind are welcome: bug reports, ideas,
testing, translations, documentation and code.

## Before you start: how the license works

Sleptic Slicer's source code is public under the
[Sleptic Source License](LICENSE.md). It is source-available, not open
source. In short:

- **You may** read and study the code, build it, and change it for your
  own personal use as an individual; organizations may modify it to prepare
  contributions intended for this project or with written permission. You
  may fork on GitHub to develop contributions.
- **You may not**, without Sleptic's written permission, do any of the
  following:
  - publish your own copy, modified version, build or installer, except
    where contribution permissions or preserved platform or other rights apply;
  - host the code or builds anywhere else;
  - run a public hosted service (internal access by an organization's own
    employees and contractors is allowed);
  - reuse the code in other software.
- **Contributing does not change that.** Even if your pull request is
  merged, you do not get permission to release your own version of
  Sleptic Slicer.

The license permits contribution-related sharing under sections 3.6 and 3.7
and does not override rights under GitHub's terms (section 3.11). Earlier
licenses and third-party licenses also preserve their rights.

Some parts of the repository come from others and keep their own licenses.
[COPYRIGHT.md](COPYRIGHT.md) lists them.

## Your fork

A GitHub fork is the normal way to prepare a pull request, and you are
welcome to make one. License sections 3.6, 3.11 and 4 govern sharing:
contribution-related sharing and rights under GitHub's terms are preserved;
other distribution needs permission unless another right applies. The
trademark policy governs presentation. As project practice, we ask you to
use forks to propose changes, avoid attaching releases or presenting them
as alternatives, and avoid outside mirrors. These requests do not override
GitHub's terms or other rights.

## The Contributor License Agreement

Before code, asset or documentation contributions can be merged, their
author signs the
[Sleptic Contributor License Agreement](CLA.md). A bot explains how on
your first pull request. Signing takes one comment, and it covers all your
later contributions. Issues and comments not intended for inclusion are
not Contributions and do not require signing.

Please read it before signing. Key points:

- **What you keep.** You keep the copyright in what you write.
- **What Sleptic may do.** Sleptic may use your contribution in its
  releases. It may also license your contribution under the Sleptic
  Source License, under other licenses it chooses later, and under
  proprietary terms.
- **No open source promise.** Your contribution is not promised to stay
  under an open source license.

## What you may contribute

- **Your own work.** Contribute only material you wrote yourself, or
  material you have the right to give under the CLA.
- **Code from elsewhere.** Do not copy code from other slicers or projects
  (for example PrusaSlicer, OrcaSlicer, Chitubox or Lychee) or from
  anywhere else. Ideas are fine; their code is not. If a change needs
  third-party material, say so in the pull request and name its source and
  license before adding it. New third-party GPL or AGPL code cannot be
  accepted; changes to existing AGPL or Apache files are accepted under
  both the CLA and the file's license.
- **Models.** Do not add 3D models whose licenses forbid redistribution,
  including 3DBenchy.
- **Employer rights.** If your employer has rights in your work, get their
  permission first.

## Reporting bugs

Open an issue in this repository's Issues tab. Please include:

1. how to make the problem happen, step by step;
2. what you expected and what happened instead;
3. your Sleptic Slicer version, operating system and printer;
4. if a model is involved, its format and size. Attach it if you can
   share it.

To report a security problem, use **Report a vulnerability** in the
Security tab instead. See [SECURITY.md](SECURITY.md).

## Proposing features

Open an issue describing:

- the problem the feature solves;
- how you picture it working;
- any alternatives you considered.

## Translations

Translations live in
`apps/slicer-gui/i18n/<language>/LC_MESSAGES/sc-slicer-gui.po`. You can
edit them with a tool such as [Poedit](https://poedit.net/). The
developers regenerate the template (`apps/slicer-gui/i18n/sc-slicer-gui.pot`)
from the interface files.

## Code

1. Read [docs/BUILDING.md](docs/BUILDING.md) to build the project, and
   [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.
2. On Windows, build and test through `scripts\cargo-msvc.bat`, for
   example `scripts\cargo-msvc.bat test --workspace --offline`.
3. Every fix or feature needs a test that fails without it, and the
   whole workspace must pass.
4. New files start with the license notice used by the files around
   them.
5. Propose the change as a pull request and fill in its template. The
   automatic checks, including the CLA check, must pass before a
   maintainer reviews it.

## Code of Conduct

Everyone taking part is expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
