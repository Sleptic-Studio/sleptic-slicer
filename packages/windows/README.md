# Windows packaging

The Windows installer for Sleptic Slicer: a single setup program, built with
Inno Setup, that ships the desktop app and the command line tool.

## Files

- `sleptic-slicer.iss` is the Inno Setup script. It installs per user by
  default (no admin prompt; the wizard still offers an all-users install),
  adds a Start Menu shortcut, an optional desktop shortcut, an optional PATH
  entry for the command line tool (off by default, removed on uninstall), and
  ends with a "Launch Sleptic Slicer" box. Its `AppId` is `it.sleptic.slicer`;
  keep it, or installs of earlier versions stop upgrading in place.
- `build-sleptic.ps1` builds both release binaries for the x64 MSVC target,
  stages them with the app-local Visual C++ runtime DLLs and the bundled
  profiles, then runs the Inno Setup compiler. Everything lands under
  `.cache`; nothing is installed.

The three runtime DLLs (`vcruntime140.dll`, `vcruntime140_1.dll`,
`msvcp140.dll`) ship beside the executables, so no Visual C++ Redistributable
is needed. The Universal CRT is part of Windows 10 and 11. The bundled
profiles go in a `slicer\profiles` folder beside the binaries, where the app
looks for them.

## Prerequisites

- Visual Studio Build Tools with the C++ workload and the Windows SDK.
- The Rust `x86_64-pc-windows-msvc` target.
- Inno Setup 6.3 or later (https://jrsoftware.org/isdl.php). A per-user
  install (`winget install JRSoftware.InnoSetup --scope user`) is found in
  `%LOCALAPPDATA%\Programs\Inno Setup 6`.

## Building

```
powershell -NoProfile -ExecutionPolicy Bypass -File .\build-sleptic.ps1
# or package binaries built with another CARGO_TARGET_DIR:
powershell -NoProfile -ExecutionPolicy Bypass -File .\build-sleptic.ps1 -NoBuild -ReleaseDir <dir>\x86_64-pc-windows-msvc\release
```

Output: `.cache\Sleptic-Slicer-Setup-<version>-x64.exe`. The installer is not
code-signed, so Windows SmartScreen shows "Windows protected your PC" on first
run: choose More info, then Run anyway.

## The icon

The installer and the app use `apps\slicer-gui\assets\icons\sleptic.ico`.
`cargo run -p sc-icon-gen` regenerates it from
`packages\shared\com.sleptic.slicer.svg` after the logo changes.
