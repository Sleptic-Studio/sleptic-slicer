# Linux: Flatpak

Two packages: `com.sleptic.slicer` (the app, which also contains the
command-line tool) and `com.sleptic.slicer.cli` (the command-line tool alone,
so it can be run as a command).

## Build and install

```
python3 flatpak.py build
flatpak install --user .cache/com.sleptic.slicer.flatpak
flatpak install --user .cache/com.sleptic.slicer.cli.flatpak
```

Needs `flatpak-builder` and the freedesktop 25.08 SDK with the rust-stable
extension. A rebuild of an unchanged tree reuses the cached build.

## Run

```
flatpak run com.sleptic.slicer
com.sleptic.slicer.cli --help
```

For the second line the Flatpak export folder has to be on `PATH`
(`~/.local/share/flatpak/exports/bin` for a user install,
`/var/lib/flatpak/exports/bin` for a system one). The command-line package
may read and write anywhere (`--filesystem=host`), because its exported
command cannot be given a grant when it is run; narrow it with
`flatpak override` if you like. The app itself has no file access beyond
what its open and save dialogs hand it; to reach the bundled tool with a
folder, use
`flatpak run --filesystem=home --command=sc-slicer-cli com.sleptic.slicer ...`.

## Offline sources

Flatpak builds without network, so the crates and the prebuilt Skia library
are listed ahead of time in `cargo-sources.json` and
`skia-binaries.source.json`. `build` first checks them against `Cargo.lock`
and stops if they are out of date; refresh them with

```
python3 flatpak.py vendor-cargo   # needs uv
python3 flatpak.py vendor-skia    # compiles the workspace to learn the archive
```

The check sees the Skia version but not its feature set (only a compile of
skia-bindings knows that); after changing Skia features, run `vendor-skia`.

Everything `build` generates (the packed tree, the metainfo for this
version, their source lists and the bundles) stays in `.cache/`. The desktop
entry, icon and metainfo template are in [`../../shared/`](../../shared/README.md).
