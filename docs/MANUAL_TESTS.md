# Manual checks after each install

Things the automated tests and scripted screenshots cannot fully prove, because they need a
real mouse, real files or a real window. Takes about five minutes. Report anything that fails
with a screenshot.

## Prepare
1. Open a model, then Fill plate. Click several copies directly in the 3D view: each click
   selects that copy (not the sidebar only). Ctrl+click adds/removes copies from the selection.
2. Drag one copy across the plate: it moves; release and Ctrl+Z puts it back in one step.
   Drag a Ctrl+click multi-selection: all selected copies move together.
3. Generate supports, then drag the model: the supports move with it. Rotate it: the supports
   turn yellow and Print risks offers Regenerate.
4. Drag a model file from Explorer onto the window: it is added to the current plate
   (no new window).
5. Add two more plates: plates form a grid; each has its own row of tool tiles on the bed
   (number, clear, orient, arrange, lock, settings). Click each tile on plate 2 and check it
   acts on plate 2 only. Lock a plate: arrange/orient/fill skip it.
6. With plate 2 active, right-click > Paste: the copy lands on plate 2. Measure tool and
   manual support clicks work on a model on plate 2.

## Preview
7. Slice, open Preview, 2D layer: drag the slider and scroll over it; nothing from the 3D
   view shows through anywhere around the slider.
8. 3D view: the model appears solid and grey, cut at the selected layer with the cut face in
   blue, zoomed onto the model. Only the sliced plate's models are shown.
9. Print risks: click a row; the slider jumps to that layer and the 2D view highlights it.

## Output
10. Export, then open the ZIP: layer PNGs plus run.gcode. Export all writes one file per plate.

## Model tools (R15)
11. On plate 2, select a model and open Cut / model tools. Drag the orange Z handle,
    then enter an exact cut height. Keep both with Close cut faces enabled. Move
    one half aside and inspect the caps. Undo the move, then undo the cut: the
    original model and support-validity state return in one step.
12. Open a mesh containing disconnected shells. Split into parts creates one
    object per shell on its original plate; Undo restores one object.
13. Choose Text label, enter mixed-case ASCII text, size and depth, and click a
    flat model face. Check the emboss, Undo, enable Engrave and click again.
    Verify a model on plate 2 as well. The label must follow the clicked face;
    its depth is measured along that face's outward normal.
14. Ctrl-select two overlapping closed models in order. Boolean Subtract removes
    the second from the first; also check Union and Intersect. Each operation
    undoes in one step. An open/non-manifold model produces a clear refusal.
    Cut, split, labels and Booleans mark owned supports stale; save/reopen must
    retain the warning, and Regenerate must replace those supports.
15. In Advanced Supports settings, check that Symmetric supports and Lattice
    raft occupy separate rows at 1280x800. Open the original R14 tree Benchy,
    slice with AA off, and confirm no Unsupported islands group in Print risks.

## Recipes and label/Preview polish (R16)
16. Open Recipes from the File menu, save a copy of each bundled default, edit a
    parameter in TOML, and use Add step / Up / Down / Remove step. Save, close,
    reopen and verify the saved order and parameters. Delete only the copy.
17. Run scale + supports + slice on a selection, on plate 2 and on all plates.
    Check each live progress row and summary. One Undo must restore all scene
    edits. Cancel during a long step and verify no scene edit was committed.
18. Run hollow on an open model: the summary must name hollow and explain its
    failure. Verify later steps are reported independently. Try a locked plate.
19. In Batch prepare choose Recipe per file; verify per-file failures and that
    successful files retain their recipe preparation. Check export files in the
    recipe's configured folder and that existing outputs are refused.
20. Emboss "Sleptic O" and engrave it on a flat face. Letters must follow smooth
    outline strokes with a visible counter in O and proportional character
    spacing. Inspect the orange cut handle alongside yellow stale supports.
21. Preview 2D: zoom/pan the plate and scrub layers. The lit-area graph must stay
    in its own column beside the slider, clear of the plate's right border.


## R18: orientation, motion and exposure calibration

- Open Orient options, select each preset, and verify the status explains changed or retained orientation. Restart and confirm the preset persists. On Sleptic's plate, orient vice grips and Lapras, then check the complete bounds and compare Balanced with Fastest.
- In printer/material Motion settings, change a slow/fast distance, speed and wait; confirm the timeline changes live. Expand bottom/transition rows and inspect exported moves/dwells. Invalid unequal total lift/retract travel must be refused at resolution/export.
- On an empty active plate, open Calibration > Exposure test. Try a start/step sequence and a comma-separated list. Inspect columns, pins, holes, fins, bridge and both text signs. Slice and check the individual exposures and total estimate in Preview.
- Export G-code ZIP and confirm one movement cycle followed by one masked exposure per tile. Attempt a bare PNG/SVG or sliced 3MF calibration export and confirm an explicit refusal. Save/reopen the project and confirm Record result restores its tile choices.
- After a physical exposure test, choose the best tile and record it. Confirm a newly named material appears in the user library, its selected-printer normal exposure matches the tile, and the original material and its bottom/motion settings remain intact.

## Home, Device and Project tabs
19. Launch from the desktop shortcut with no file: the Home tab opens. Recently opened shows
    your last files with dates; a deleted file shows "File not found" and does nothing.
    New project clears the scene (asks to save first when there are changes) and opens Prepare.
20. Plug in a USB drive or SD card, open Device, click Refresh drives: it is listed by name.
    Slice a plate, then Export here: the file appears on the drive; exporting again writes
    "name (2).zip", never overwriting.
21. Set "After every export, run" to `cmd /c echo {file} > %TEMP%\sleptic_last.txt`, export:
    the success message says the command finished and the file holds the exported path.
22. Device > Print history lists the export; set it to Failed and enter a layer, switch tabs
    and come back: both are kept.
23. Project tab: each plate shows its objects and Sliced / Not sliced; clicking a row opens
    that plate in Prepare.
24. Drag a plate-edge icon with the right mouse button: the view pans and does not stop.

