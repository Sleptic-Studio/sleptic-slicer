# Sleptic Slicer (desktop app)

The desktop app of [Sleptic Slicer](../../README.md): it opens models,
prepares them on the build plate (repair, orientation, supports, hollowing)
and slices them for a resin printer, with a layer-by-layer preview.

Built with [Slint](https://slint.dev/), with a [wgpu](https://wgpu.rs/) 3D
view.

## Running from source

```
cargo run --release -p sc-slicer-gui
```

On Windows use `scripts\cargo-msvc.bat` in place of `cargo`; see
[docs/BUILDING.md](../../docs/BUILDING.md). For editing the interface and
working on translations, see [CONTRIBUTING.md](../../CONTRIBUTING.md).

## Using it

- **Open** STL, OBJ or 3MF files from the File menu, or drop them on the
  window. A window that already holds a model opens dropped files in a new
  window, so nothing is lost.
- **Measure** in Prepare: click two points on a surface to see the distance
  and its X, Y and Z parts in millimetres; a third click starts again and
  Esc clears it.
- **Recovery:** while a project has unsaved changes it is saved to a
  recovery file every two minutes. After a crash, the next start offers to
  restore it (restored projects need Save As). Closing normally removes
  these files.
- **Profiles:** pick a printer and resin in the Settings panel or in
  Preferences. Built-in profiles never change; edit or duplicate one to make
  your own, saved in your user library. A printer's settings must agree with
  its pixel grid, and a job template with an unknown placeholder cannot be
  saved.

## License

AGPL-3.0, see [LICENSE](./LICENSE.md) or the SPDX header of each source
file. The Preferences side bar (`ui/sidebar.slint`) started from an
MIT-licensed example from the Slint project.
