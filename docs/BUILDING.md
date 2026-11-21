# Building Sleptic Slicer

Sleptic Slicer is written in Rust with a [Slint](https://slint.dev/)
interface and a [wgpu](https://wgpu.rs/) 3D view.

## Requirements

- Rust (stable) with Cargo.
- On Windows: Visual Studio 2022 with the C++ build tools. Build and test
  through `scripts\cargo-msvc.bat`, which loads the MSVC environment, for
  example `scripts\cargo-msvc.bat build -p sc-slicer-gui --release`.
- A graphics card with Vulkan, Metal or DirectX 12 drivers for the 3D view
  (and for the renderer tests, which are skipped without one).

## Run from source

```text
scripts\cargo-msvc.bat run --release -p sc-slicer-gui
```

Elsewhere, plain `cargo run --release -p sc-slicer-gui` works.

## Slice from the command line

```text
scripts\cargo-msvc.bat run --release -p sc-slicer-cli -- slice -s model.stl -t job.zip ^
    --target-format=gcode-zip --printer-profile=sleptic-diy-msla --material-profile=standard-405
```

`gcode-zip` writes the layer images plus a `run.gcode` rendered from the
printer profile's `[job]` templates (start, per layer and end, with
`{placeholders}` and a free-form `[job.custom]` table), so a custom printer
needs a profile rather than code. `assets/profiles/printers/sleptic-diy-msla.toml`
is a commented example. Every subcommand explains itself with `--help`; see
[the CLI's README](../apps/slicer-cli/README.md).

## Tests

```text
scripts\cargo-msvc.bat test --workspace --offline
```

The whole workspace must pass before a change is merged; see
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Portable Windows build

With the Cargo cache already filled and the Visual Studio C++ tools
installed:

```text
powershell -ExecutionPolicy Bypass -File scripts/package_windows.ps1
```

The script builds offline (`scripts\cargo-msvc.bat build -p sc-slicer-gui --release --offline`)
and downloads nothing. It writes a ZIP under `target/dist/` holding
`Sleptic Slicer.exe`, `sleptic.ico`, the bundled profiles under
`slicer/profiles/` (keep that layout when extracting) and the Visual C++
runtime DLLs for app-local use. `scripts/render_sleptic_icon.ps1` renders the
icon's sizes into `sleptic.ico`; the committed icon lets ordinary builds skip
that step.

## Where the app keeps its files

- Bundled profiles are read from beside the executable and never changed;
  profiles you edit or create are saved in your user data library.
- While a document has unsaved changes, a recovery snapshot (3MF) is written
  every two minutes under the platform's application data folder
  (`%LOCALAPPDATA%/Sleptic/Slicer/data/recovery` on Windows). After a crash
  the next start offers to restore it; restored documents need Save As.
  `SLEPTIC_RECOVERY_DIR` points recovery somewhere else, for portable use.

## Automation and screenshots

The app accepts options for scripted checks: `--open`, `--printer`,
`--material`, `--slice`, `--workspace`, `--preview-layer`, `--preview-view`,
`--settings-mode`, `--search`, `--theme`, `--window`, `--screenshot` and
`--dump-state`. They are what the project's own visual checks use.
