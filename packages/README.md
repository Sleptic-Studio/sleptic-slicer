# Packaging

Build recipes and metadata for the distributable packages of Sleptic
Slicer. Each supported operating system and packaging format has its own
directory here.

## What's here

- [`linux/`](./linux/README.md) packaging for Linux.
- [`windows/`](./windows/README.md) packaging for Windows.
- [`macos/`](./macos/README.md) packaging for macOS (Apple Silicon).
- [`shared/`](./shared/README.md) assets used by more than one format: the
  application icon, AppStream metainfo, and the application id.

Each format lives in a self-contained directory with its own manifest and build
instructions, so building one format does not depend on the others.

## Conventions

- The application id is `com.sleptic.slicer` (Flatpak app id, desktop entry,
  metainfo, macOS bundle id), matching the `com` / `Sleptic` identity the
  binaries use at runtime to locate user data. The Windows installer keeps its
  own `AppId`, `it.sleptic.slicer`, so installs of earlier versions upgrade in
  place.
- The Slicer is licensed under AGPL-3.0-only, declared in each package's
  metadata.
- The GUI (`sc-slicer-gui`) and CLI (`sc-slicer-cli`) are distributed together;
  every package contains both. Flatpak additionally ships the CLI on its own,
  under `com.sleptic.slicer.cli`, because Flatpak exports a single command
  per application id and the GUI's id already spends it on the GUI.

## Building

Packages are built by hand on each target operating system; there is no CI for
packaging. Each format's directory documents how to build it.
