# Architecture of Sleptic Slicer

## Model editing and support/raster corrections (R15)

`sc_print_glue::model_tools` stages horizontal cuts, edge-connected shell splits,
closed-solid Booleans and printable ASCII labels without mutating inputs.
Booleans use oriented convex polygon partitioning, split intersected faces,
resolve coplanar material sides, insert matching edge subdivisions and triangulate
the resulting boundary. Input and output require closed, consistently wound
edge and vertex manifolds. Degenerate or numerically invalid results are refused.
Cut uses a bounded half-space solid and can omit its generated plane faces.
The current limits are 100,000 polygons per partition, depth 256, five million
partition polygon visits and 100 million edge/subdivision comparisons. Excess
work returns an explicit error. These are finite-precision mesh operations,
not an exact-arithmetic CAD kernel.

R16 replaces the original 5x7 bitmap label font with outlines extracted offline
from the already bundled Noto Sans variable font (default master). The font and
derived outline data are SIL OFL 1.1; the full license and copyright are in
`apps/slicer-gui/assets/fonts/OFL.txt`. `scripts/generate_label_font.py` reproduces
the printable ASCII data using the locally installed fontTools, without network
access or a new Rust dependency. Quadratic curves use three line segments per
curve; text positions accumulate each glyph's horizontal advance in em units.
Horizontal monotone trapezoids cap contours with even-odd crossings, retaining
counters such as the hole in O. Side walls follow the original outlines. The
Boolean kernel receives the double-precision polygons directly, avoiding a
lossy f32 intermediate mesh before partitioning. A clicked geometric face normal
defines depth and an upright tangent defines the text axes. Emboss unions the
extrusion; engrave subtracts it. A 0.01 mm overlap avoids coincident contact-only
faces. Emboss refuses newly detached glyph shells. Labels use one planar face
frame and do not wrap around curved surfaces.

GUI plans bake into plate-local world millimetres, compute complete results, then
map meshes back into the original object's coordinates. Additional parts preserve
the source world transform and plate assignment. A single compound edit owns
all mesh, membership and metadata changes. Names/user comments survive; obsolete
editable hollow sources and paint projections are cleared. Owned supports remain
visible and acquire `geometry_stale = true`, persisted in the existing support
metadata part with a backward-compatible default. Boolean operands' supports
are reassigned to the surviving owner and marked stale. Undo restores every
snapshot. Inline mesh installation initializes spatial caches even when replacing
a resource-backed leaf at the same ID, keeping results pickable after edits.

Strict manifold slicing now preserves geometric material-side ring winding and
performs a NonZero union when a layer contains several rings. Edge-manifold
shells can geometrically overlap; containment parity alone incorrectly turned
tree/trunk intersections into holes. Existing reversed cavity shells remain
voids. The canonical union shares the tolerant slicer's 10 nm integer grid.
This corrects the original R14 tree fixture's 27 AA-off islands; AA x4 had masked
their raster disconnections in the old benchmark.

Rasterization indexes edges by occupied rows and skips empty rows and exterior
columns while preserving original crossing order and exact coverage arithmetic.
An independent R14 full-frame AA reference verifies identical raster bytes;
work-count tests guard sparse-row traversal. Branch clearance uses an XY BVH
with the same exact vertical triangle test. Trees cluster contacts around a
shared centroid across downward patches of the same connected shell, constrain
every branch's inclination and sample clearance, and retain elevated repair feet.
Low contacts can shorten their terminal taper to 20% of available height; the
configured tip length remains its maximum. Raster completion still verifies the
final geometry rather than assuming contact placement is sufficient.

## Multi-plate projects and batch preparation (R13)

`sc_scene::plates::Plates` owns ordered, stable plate identities, lock state,
opaque process settings, and leaf assignments. A legacy scene starts with plate
1; an explicit assignment overrides that default. Reordering preserves identity.
Generated support membership resolves through its model owner. Multi-model support
generation emits separate owned rafts, so overflow arrangement can keep each
model and its supports together. `SceneEdit::SetPlates` snapshots plate changes
for atomic compound edits, undo and redo.

Geometry remains in each plate's local millimetre frame. The Prepare renderer
adds `(width + 32 mm) * plate_index` only for display. Picking subtracts the same
offset; gizmo input uses the selected plate's camera frame. Cross-plate releases
normalize the moved group's transforms and change membership in one history
entry. `Scene::plate_scene` keeps stable node IDs and parent transforms while
excluding leaves belonging to other plates. GUI and CLI slicing use that scene,
so another plate's geometry never enters its layer schedule or risk analysis.

The GUI retains a slice and presentation cache per plate, including statistics,
risks and layer position. Slice all runs sequentially through the existing worker
and cancellation path. Export all snapshots the current plate jobs and respects
the selected export format. A sliced 3MF export contains one independently
loadable plate; ordinary project Save retains the complete project. CLI plate
numbers are one-based positions in the saved order, rather than stable IDs.
`--all-plates` takes a target folder and an explicit target format and writes
`<project>_plate<N>.<ext>`.

`/Metadata/sleptic-plates.toml` stores version 1 of the ordered plate table and
assignments by expanded leaf order, with a namespaced OPC relationship and
MustPreserve relationship. Settings use `sc_print_profiles::scene_overrides::PlateSettings`.
Missing metadata retains the legacy one-plate default. The ordinary project
writer is also the autosave/recovery writer, so plate data uses the same path.

`batch_prepare::prepare_paths` repairs and optionally orients, hollows and
supports files on a worker, records file failures, and checks cancellation
between files and before committing. Oversized files are reported individually.
`batch_prepare::plan` remaps leaf/support owners, shares immutable meshes and
uses the deterministic shelf arranger to overflow onto additional plates.
The live document receives one compound edit only when its revision still
matches the captured revision. Optional slicing and export use the ordinary
bulk actions after that commit. Cancellation discards the staged batch.

## Hollowing and plate tools (R8)

`sc_print_glue::hollow` builds a BVH over the input triangles and uses exact
point/triangle distance queries and directed scanline winding to identify the
solid interior. It requires a closed edge-manifold input; malformed/open input,
nonpositive settings, a missing cavity, axes above 2,048 cells, or more than
32 million grid cells are explicit errors. No third-party geometry kernel or
new crate was added; glam 0.32.1 was already used by the GUI.

Grid pitch starts at `min(0.125 mm, wall thickness / 16)` and grows in 10% steps
only when necessary to meet the axis/cell limits, up to
`min(0.5 mm, wall thickness / 4)` (raised from 0.25 mm / 8 so plate-sized figures
can be hollowed; walls only get more conservative). Larger grids are refused. A cavity cell is admitted only
when its centre lies inside the model and its distance from every triangle is
at least `thickness + sqrt(3)*pitch/2`. The bounding sphere therefore keeps
the entire removed cell at least the requested wall thickness from the original
surface. The interior is a staircase surface with up to `sqrt(3)*pitch`
additional conservative wall thickness on resolved watertight surfaces.
R9 recognizes closed manifolds whose triangles all lie on the six bounding-box
planes. For these axis-aligned box surfaces only, the exact cell support radius
is `pitch/2`; other geometry retains the conservative half-diagonal rule. At
2 mm on the 20 mm cube this admits an exact 16 mm cavity at the existing pitch,
with a 2.000 mm minimum wall and 3,904 mm3 shell. Resolution and limits remain
unchanged for curved models. Auto-drain suggestions choose nearest cavity-floor
samples to quarter/three-quarter targets along its longer XY extent, centered
on the other axis, avoiding the old lexicographic samples along a side edge.

The original exterior remains exact when there are no drains. Before R9, for the 20 mm
cube at 2 mm, the cavity is 15.75 mm across and the minimum wall is 2.125 mm:
shell volume 4,093.015625 mm3 versus analytic 3,904 mm3, a 4.84% excess. The
regression tolerance is 5%; this trades volume accuracy for guaranteed walls.
There is no internal infill. Cavity normals face inward and the ordinary slicing
contract retains the void; irregular voxel edge contacts can use the existing
tolerant/union path.

Drain holes subtract finite cylinders from the material grid before meshing.
Their surfaces and the exterior are then voxel sampled, with a maximum
half-cell-diagonal sampling error (0.109 mm at default pitch, 0.217 mm at
the coarsest allowed pitch). The cylinder
diameter is a nominal geometric diameter, not a printer-pixel measurement.
Manual clicks use the picking ray as the channel direction, diameter default
3 mm, and depth to the first cavity intersection plus two grid cells. Rays
that miss the cavity are refused instead of creating blind holes. Auto placement finds each 6-connected
cavity, chooses two separated samples from its lowest occupied grid level,
and cuts downward from the cavity through the exterior. Its channel length
covers the distance to the model's bottom. Suggestions are geometry based;
the warning specifically counts hollow objects with no recorded drain channels,
and does not assert that a manually placed channel vents every cavity.

GUI mesh metadata holds the source vertices/faces, thickness, saved volume and
local drain position/direction plus millimetre diameter/depth. Rebuilding always
starts from that original source, avoiding cumulative voxel erosion. The
`/Metadata/sleptic-hollow.toml` 3MF part maps this data by expanded leaf order,
alongside the final printable geometry. Undo snapshots source/settings together
with the final mesh. The saved-volume readout includes cavity and channel cells;
with drains it is relative to the sampled original solid. Warnings and saved
volume refresh on document events, including undo and project loading. Saved
volume is stored in local coordinates and multiplied by the current absolute
world determinant, so scaling and mirroring keep the readout correct. Scaling
the baked hollow geometry also scales its walls; press Hollow again to rebuild
at the nominal millimetre wall setting after a scale change.

Arrange retains the R7 deterministic shelf algorithm but translates the complete
packed bounds to the plate centre. Parent transforms are respected. Fill plate
tries complete model/support groups until the next group cannot fit, shares
immutable source mesh storage, rewrites support ownership to each new model,
and records all copies plus placement as one undo command. An explicit 10,000
copy safety bound protects pathological footprints. A 20 mm cube at 3 mm gap
on Sleptic's 192 x 120 mm plate fits 8 x 5 = 40 total, adding 39. Mirror reflects
in a selected world-axis plane through the object's bounding-box centre and
drops its minimum Z to zero. Existing baking reverses triangle winding for
negative determinants. All three mirror axes use the standard undo history.

R1 audit, 2026-10-24. Paths below are relative to the repository root.
Model/world coordinates are right-handed, Z-up millimetres; the plate is XY-centered
at the origin with its floor at Z=0. Geometry slicing produces vector contours;
rasterization happens when exporting PNG, not inside the contour slicing pass.

## Mesh file to placed scene

- **Loading:** `libs/project-formats/src/loader.rs::load_scene` is the common
  application entry point, returning `LoadedScene` (scene, optional project payload,
  saved slices, warnings). Plain meshes go through
  `libs/mesh-formats/src/loader.rs::load_mesh_from_reader`; the lower-level
  `load_mesh_from_path` returns `LoadedMesh { graph, warnings }`.
  `libs/mesh-formats/src/stl/{mod,stla,stlb}.rs` implement `Stl`, `StlAscii`,
  `StlBin`; `obj/reader.rs` and `threemf/reader.rs` handle OBJ and 3MF.
- **Representation:** `libs/mesh-core/src/indexed_mesh/mod.rs::IndexedMesh`
  stores indexed vertices/faces. `libs/scene/src/scene.rs::Scene` owns roots,
  nodes and shared mesh resources; `node.rs::{NodeKind,LeafPayload}` describe
  groups, mesh leaves, CSG and references with local transforms.
  `libs/scene/src/bake.rs::bake_leaf_to_world` composes ancestor transforms.
  Scene representation supports more node kinds than the slicing engine accepts:
  its current API refuses CSG and subtree references.
- **Validation and repair:** `libs/sl1cer/src/api.rs::validate_baked` checks baked
  vertex finiteness, face indices/collapsed edges, then edge-manifoldness, using
  `libs/mesh-core/src/indexed_mesh/{validate,validated}.rs`. Scene slicing aggregates
  leaf validation failures and emits no partial result. This is distinct from the
  broader diagnostic/health functions in `libs/scene/src/{diagnose,mesh_health}.rs`.
  `libs/scene/src/repair.rs::Scene::repair_meshes` calls the per-mesh policy in
  `libs/mesh-core/src/indexed_mesh/repair.rs::IndexedMesh::repair`.
  `RepairPasses::default_repair` removes repeated-index faces and corrects flipped
  faces; normal recomputation is off. It does not fill holes.
- **Placement:** `libs/scene/src/aabb.rs::{translation_to_drop_at,drop_at}`
  translate the entire scene so its AABB center reaches the target XY and its
  minimum Z reaches the floor. They preserve relative layout and existing rotation;
  they introduce no rotation. CLI auto-placement is in
  `apps/slicer-cli/src/commands/slice.rs::place_on_build_plate`; saved projects
  retain placement, and plain meshes can opt out with `--no-auto-place`.
  GUI import uses `apps/slicer-gui/src/mesh_loader.rs::place_on_plate`.

## Profile selection, slicing and pixels

`libs/print-glue/src/slice_plate.rs::slice_plate` is the shared application service.
`SlicingParams` carries optional layer/first-layer height and AA. Preparation in
`prepare.rs::slice_profile_for_selection` resolves layer height, measures scene top
and derives a covering schedule. `projection.rs::project_hardware_to_slice_profile`
constructs the engine's `SliceProfile` from hardware, schedule, mirror and AA;
`slice_plate` uses `PackingMode::None`. Exposure and motion do not gate geometry
slicing. `SlicedPlate` holds per-leaf contours plus the exact profile; `merge()`
concatenates polygons on the common layer grid, without a Boolean union.

`libs/sl1cer/src/api.rs::{slice_scene_with_progress,slice_leaf}` bake and validate
meshes, then intersect scheduled Z planes. `profile.rs::LayerSchedule` uses slab
centers (first plane at half first-layer thickness). `z_bins.rs::LayerBuckets`
limits candidate faces; `mesh_slice.rs` and `plane_intersect.rs::intersect_triangle_z`
produce crossing segments. `stitch.rs::{stitch_layer,normalise_to_ccw}` joins rings;
`grouping.rs::group_into_polygons` assigns outers/holes. `contour.rs` defines
`ContourStack`, `ContourLayer`, `Polygon`, `ClosedRing`. Cancellation and progress
are exposed through `CancelToken` and `SliceProgress`.

`libs/sl1cer/src/raster/pixel_map.rs::PixelMap::mm_to_pixel` shifts centered XY into
the pixel grid, scales each axis and applies its mirror flag. `raster/scanline.rs`
implements `rasterize_layer`: Off samples pixel centers with nonzero winding;
x2/x4/x8 use 2/4/8 Y sub-scanlines with analytic X-span coverage, quantized to u8.
`min_alpha_pct` can suppress low coverage. `raster/rle.rs::{LayerRaster,RleRun,
RleRunAccumulator}` stores and merges grayscale runs across row boundaries.
This is an in-memory representation, not a vendor file codec.

## Writers and application entry points

`libs/sla-formats/src/format.rs::Format` currently contains only `SvgPerLayer`,
`SvgCombined`, `PngPerLayer`, `ThreeMf`, with names, file/directory classification
and `format_from_path`. `write.rs::write_slice` validates the target and dispatches
to `debug/svg.rs::{write_per_layer,write_combined}` or
`debug/png.rs::write_per_layer`. PNG export rasterizes and encodes one layer at a
time; it does not retain a full raster stack, though vector contours are retained.
`threemf.rs::write_scene_slices` needs the scene and per-leaf contours; sending
`ThreeMf` to `write_slice` returns `SceneContextRequired`.

`options.rs::WriterOptions::from_profile` carries build volume, resolution, mirror,
AA, layer schedule, slicer version and optional minimum alpha. It contains **no
exposure or lift configuration**. No printer job script is generated by PNG export.

CLI: `apps/slicer-cli/src/config/cmd_options.rs` parses flags;
`commands/slice.rs::slice` loads profiles/scene, optionally repairs, places, calls
`slice_plate`, merges for debug exports and calls the appropriate writer.
GUI: `apps/slicer-gui/src/mesh_loader.rs::parse_source` loads through `load_scene`;
`slice_run.rs::run_slice` invokes `slice_plate` on a worker with progress/cancellation.
The result is captured in `slice_result.rs::SliceResult`;
`slice_export.rs::{prepare_slice_export,write_slice_export}` builds writer options
from the held profile and calls `write_slice`, with UI wiring in `wiring.rs`.
`print_estimate.rs::estimated_volume_mm3` sums contour area times slab thickness.

## Printer and material TOML

`libs/print-profiles/src/loader.rs` provides typed TOML loading; `profile_libraries.rs`
provides bundled/user selection and material extension composition. Examples live
in `assets/profiles/{printers,materials}/`.

- `printer_profile.rs::PrinterProfile`: `[identity]`, `[hardware]`,
  `[capabilities]`, `[constraints]`. `printer_hardware.rs::PrinterHardware` holds
  `build_volume`, `pixel_resolution`, `pixel_size_microns`, `mirror` and checks XY
  grid agreement. Capabilities include output-format bindings, grayscale,
  two-stage lift, optional transition-layer capability, camera/network protocol.
  Constraints include Z step, layer-height/speed ranges, maximum lift distance and
  light intensity. `[instance]` is reserved and rejected.
- `material_profile.rs::MaterialProfile`: `[identity]`, `[intent]`, `[defaults]`,
  `[per_printer."<id>"]`. `material_intent.rs::MaterialIntent` holds target energy
  and layer height. `material_defaults.rs::MaterialDefaults` holds optional bottom
  layer count, scale compensation and resin price.
  `material_per_printer.rs::MaterialPerPrinter` holds normal/bottom exposure seconds,
  bottom count, lift distance and lift/retract speeds. A generic-machine calibration
  block is supported. No faded exposure ramp or elephant-foot setting exists here.
- `resolver.rs::resolve` selects printer-specific or generic calibration, applies
  defaults/intent derivations, constraints and `scene_overrides.rs::SceneOverrides`,
  yielding `ResolvedPrintConfig` and `MaterialResolved` with source diagnostics.
  `resolve_layer_height` resolves only the input required for geometry slicing.

## Image orientation

Measured from **decoded final PNG pixels**, not just `PixelMap` coordinates:

| mirror.x | mirror.y | Model +X in PNG | Model +Y in PNG |
|---|---|---|---|
| false | false | right | up |
| true | false | left | up |
| false | true | right | down |
| true | true | left | down |

```text
Unmirrored PNG (screen rows increase downward):
       +Y ^
          #
          #
          #
          ### --> +X
          Z = 0 floor; extrusion along +Z
```

Internally `PixelMap` increases raster Y with model +Y; PNG's
`debug/png.rs::materialize_top_down` reverses rows. Both mirrors therefore give a
180-degree image rotation. `apps/slicer-cli/tests/cli.rs::orientation_audit_tests`
constructs a closed extruded notched L on Z=0, writes STL, runs the real CLI/profile/
slicer/PNG pipeline at 50x50, and checks every pixel in both layers for all four
mirror pairs. Each case checks auto-placement against explicitly centered geometry:
placement does not rotate. These tests establish software convention, not which
mirror pair Sleptic's physical LCD/optics requires. No orientation behavior changed.

## Benchy repair measurement

Local input: a copy of 3DBenchy (its licence does not allow redistribution, so it is not in the repository).
Mars 3 bundled profile, requested 0.05 mm uniform layers, AA x4, auto-centered.
Raw validation reports 554 ZeroLengthEdge diagnostics; repair removes **552 distinct
faces**, from **225,706 to 225,154**. The fix list contains 554 entries because
`indexed_mesh/fix.rs::fix_zero_length_edges_impl` builds it before deduplicating face
indices. Signed mesh volume before/after is **15,550.531015103028 mm3**, delta **0**.

The strict unrepaired pipeline produces no slices, so its sliced volume is
**unavailable**, not zero. For a controlled tolerant baseline, the measurement
independently omits repeated-index faces only, asserting each has exactly zero
area, without applying orientation repair. That reference and default repair both
produce **960 layers**, **15,553.455861182712 mm3** by the GUI's contour-area-times-
thickness method: delta **0 mm3 (0%)**, with identical complete contour stacks.
This is not an unrepaired CLI success or an AA-weighted pixel-volume measurement;
AA does not affect contour volume. Defaults remain unchanged.

Reproduce with `apps/slicer-cli/tests/tests_benchy_audit.rs`: set `SC_BENCHY_STL`
to the local file, then run
`scripts\cargo-msvc.bat test -p sc-slicer-cli --offline --test tests_benchy_audit -- --ignored --nocapture`.
The external-model measurement is ignored in ordinary workspace runs.

## Where a new output format plugs in

Add an Apache-2.0 writer module in `libs/sla-formats/src/`, extend `Format` parsing,
classification/inference and `write_slice` dispatch, and export any new public entry
point from `lib.rs`. Reuse `rasterize_layer`/`LayerRaster` for streamed layer output.
Update CLI format dispatch and GUI export wiring/filter choices. Printer capability
`libs/print-primitives/src/format.rs::FormatKind` is a separate catalogue (including
planned vendor formats); naming a capability does not implement a writer or select
one automatically. A PNG-plus-job-script format needs resolved process configuration
passed explicitly: current `WriterOptions` cannot supply it.

## Where printer-specific settings would live

Existing physical geometry/orientation belongs in printer `[hardware]`, machine
limits in `[constraints]` and feature declarations in `[capabilities]`. Resin/printer
process calibration belongs in material `[per_printer."<id>"]`; per-job process
choices use `SceneOverrides` and `resolve`. New motion/script dialect or device
configuration requires an explicit schema and export contract designed by the lead;
no job-script template or usable physical-device `[instance]` overlay exists today.
Keep that configuration above the geometry engine and feed its resolved values to
the future writer. Transition exposure ramps and bottom compensation would require
new settings and implementation, not merely populating existing exposure fields.

## Tolerant slicing and canonical contours (R6)

Strict slicing remains the default engine API. The CLI enables
`slice_plate_with_fallback` with its default repair policy; `--no-repair` disables
both mesh repair and the fallback. The GUI repairs its worker copy and enables
the same fallback. Support completion receives the same policy, so its retained
raster analysis describes the final slicing behavior.

Fallback is selected per baked leaf, only after finite-vertex and face validation
succeed and edge-manifold validation fails. Valid leaves use the unchanged strict
intersection, topological stitching and polygon grouping pipeline. A manifold
leaf's later stitching error is still an error, rather than a fallback trigger.

For each tolerant layer, half-open triangle/plane intersections are directed by
`Z cross face normal`, preserving the natural winding of outward shells and
inside-out cavities. Equal directed segments are deduplicated. Endpoint buckets
find deterministic nearest directed continuations (face order breaks ties),
searching forward and backward. Short gaps up to **0.01 mm** are bridged while
preserving original segment direction; remaining open connected chains are
discarded. The diagnostic counts discarded chains across all layers, **not faces,
endpoints, or unique 3D defects**.

Closed rings enter one `i_overlay` union per leaf/layer with `FillRule::NonZero`.
Integer coordinates are signed 64-bit, at **100,000 units/mm** (0.00001 mm / 10 nm
resolution, at most 5 nm rounding per coordinate). Coordinates outside the
conservative +/-2^61-unit budget are refused before integer conversion. This is
sub-micron precision across the build plate without reducing precision for large
plates. Output shapes become checked `Polygon::new(outer, holes)` values: CCW
outers, CW holes, no overlaps within a tolerant leaf/layer. NonZero merges
interpenetrating shells and keeps reversed interior shells empty. All existing
SVG, 3MF, raster, signed-area, graph and stack-preview consumers keep the same
canonical contour contract. This operation does not union distinct scene leaves.

`LeafSlices` exposes whether fallback was used and its dropped-fragment total.
The CLI emits a warning per recovered leaf; the GUI aggregates recovered leaves
and fragments in the status bar. Cancellation and progress retain the existing
per-layer semantics; only contour vectors, not full-resolution layer images, are
retained in memory.

## Grouped support rows (R6 item A)

Generated contact metadata records its source model's stable scene ID, selected
by the closest model surface at the contact's XY position after baking to world
space. The project support part stores the owner by expanded leaf index and maps
it to the loaded model ID on restore; the optional field is compatible with older
projects. Manual and automatic contacts use the same ownership helper.

The object list maps display rows explicitly to underlying leaf indices: each
model is followed by one translated **Supports (N)** row, counting columns and
summing the group's triangles. Visibility toggles every leaf in a support group.
The shared plate raft belongs to the first model's support row and is excluded
from N; legacy supports without ownership also use that row. If no model remains,
orphan supports remain visible in one group. Support rows clear the single-model
transform selection when clicked; Tab cycles model rows. Viewport picking still
addresses actual geometry, and selection mirrors back to its containing row.

Opt-in real-model acceptance uses `SC_R6_MODELS_DIR` and
`scripts\cargo-msvc.bat test -p sc-slicer-cli --release --offline --test tests_r6_local_models -- --ignored --nocapture`.
The three files are local fixtures, never workspace-test prerequisites.


## Print accuracy and placement (R7)

Accuracy resolves independently of motion/exposure so uncalibrated material/printer
pairs can still produce geometry. Material defaults, per-printer calibration and
SceneOverrides carry `bottom_compensation_mm` (nonnegative, default 0.0),
`xy_compensation_mm` (signed, default 0.0), and `aa_levels` (integer 2..256,
default 256). Scene overrides win over calibration, then material defaults.
The resolved SliceProfile retains these values and bottom/transition counts;
WriterOptions copies them rather than resolving them again during export.

`sl1cer::raster::accuracy::rasterize_accurate` is the shared PNG, gcode-zip,
volume and island path. It offsets the layer contours with i_overlay's i64
outline API (CCW outside, CW holes), using a miter bound of four times the offset.
It then rasterizes coverage, erodes with a separable rectangular grayscale minimum
filter, and quantizes to evenly spaced grayscale values including 0 and 255.
Each one-dimensional minimum uses a monotonic deque: every input index enters
and leaves once. Black padding outside the screen prevents edge wrapping.
X/Y radii round independently to the closest printer pixel. Bottom layers use
the full radius; transition layer t (1-based) uses `radius * (1-t/(N+1))`,
matching the existing exposure ramp. Normal layers have no bottom erosion.
Zero compensation and 256 levels preserve the original raster runs exactly.
Only one layer is materialized at a time, with one temporary image for erosion.
The viewport cap uses the same corrections at its existing bounded preview
resolution; this is an approximation of final printer pixels, never the export.
Island overlay spans use corrected coverage at actual printer resolution.
SVG and project contours remain the original geometry; corrections belong to
the print raster contract and do not mutate the editable scene.

`apps/slicer-gui/src/layout_tools.rs::auto_orient` evaluates the 26 nonzero
{-1,0,1} directions plus both signs of at most 12 face-normal clusters, selected
from largest triangles first (10-degree clustering). Candidate order and score
ties are deterministic. The score is:

`0.8 * weighted_downward_area / surface_area + peak_section / (surface_area/2)
 + 1.5 * height / original_bounding_diagonal`.

Downward faces beyond the support-angle threshold contribute their area weighted
by the downward cosine. Peak section is sampled at 24 equally spaced heights;
oriented triangle/plane segments contribute their signed shoelace integrals,
so holes subtract without stitching. This is a coarse sampled objective rather
than a guarantee of the maximum over every possible layer. It favors lower peel
area while accounting for support burden and print height. The 20x20x1 mm plate
regression expects a diagonal tilt: flat has a 400 mm2 peel, while vertical costs
the full 20 mm height. The selected tilt reduces peel with less height than a
vertical plate. The rotation preserves XY centre, drops to Z=0 and uses the
existing undo stack.

Arrange uses deterministic shelf packing with a configurable gap (Advanced
sidebar, default 3 mm), checked against XYZ plate bounds. Each model and its
owned support leaves form one bounding box and receive one common translation.
Shared raft and legacy support ownership follow the object list's first-model
convention. All transforms form one undoable compound edit; failure applies no
partial changes and reports model names (stable IDs when unnamed).


## Printer authoring, recovery, and measurement (R9)

`printer_editor.rs` derives the hardware fields from a full printer profile,
keeps its capabilities, constraints and custom job values, and deserializes
edits through the existing typed schema. All three templates are rendered with
`sc_print_job::render_template` before saving. Missing job blocks start from
`default_job_templates()`, shared with export. Saves target only the user
library; same-ID edits override bundled profiles without modifying them.
Duplicate chooses a free `-copy-N` ID across all library roots. Saving refreshes
the catalog, selects the saved printer and publishes the new build envelope.

`recovery.rs` owns one locked snapshot path per window and a two-minute timer.
On a dirty document it snapshots the ordinary project save bundle on the UI
thread, then writes its atomic 3MF on a worker. Writes never mark a document
saved. A later tick waits for an earlier worker to finish rather than overlap
writes; normal close stops the timer, joins the worker, and removes the file.
Startup claims only unlocked 3MF paths. Restore opens each in its own window;
the loaded project retains profiles/process data but is dirty and requires
Save As. Locks remain held while the prompt or restored window owns a file.
Pending files survive dismissal of the host window, load failure, or abnormal
termination. `SLEPTIC_RECOVERY_DIR` overrides the platform local data directory
for portable deployments and reproducible desktop verification.

The measure tool shares support/drain surface picking, using the actual
world-space ray/triangle intersection rather than screen distance. Its two
points are view-only state, outside undo and scene serialization. The overlay
shows their distance and signed second-minus-first axis deltas; projected
markers and a line follow camera changes. Esc clears them and a third surface
click starts over. Importing a new scene clears measurement state.

Windows uses Slint 1.17's enabled `unstable-winit-030` window-event accessor.
HoveredFile, HoveredFileCancelled and DroppedFile are already exposed here.
R9 routes additional files to new windows whenever the receiving window is
occupied, loading, or has a request queued before GPU setup. This preserves
all files in a batch and avoids replacing existing work. Empty windows accept
the first drop in place. Unsupported extensions use the usual error path.


## Input handling and sliced stack (R10)

Viewport right-clicks retain the existing click/drag classifier. The popup uses
compact themed Slint rows, with disabled actions visible, Escape/click-outside
dismissal, keyboard navigation, and a Mirror submenu. Plate commands preserve
selection. An object hit preserves a selection containing it; otherwise the hit
becomes the sole selection. Commands share the normal document/tool paths.

Selection stores an ordered set of stable leaf IDs; its last member is the primary.
Ctrl-click toggles membership, Ctrl+A selects all, Delete removes the whole set in
one command, and Escape retains the existing gesture-abort cascade before clearing.
Copy/paste/duplicate preserve relative transforms for a multi-object fragment.
Duplicate searches near the primary object rather than the mouse projection.
All selected leaves receive viewport tint/outline; model rows mirror membership.
Arrange still packs the whole plate, including all selected objects and supports.
Mirror applies to the selected leaves as one compound command. Other transform,
hollow, orientation, fill, object support and object export tools use the primary.

OS drops accumulate for 200 ms into a batch on the receiving window. STL/OBJ are
parsed off-thread and appended to the current document. Deterministic packing
searches fixed obstacle edges with 3 mm clearance; only imported leaves move.
Failure refuses the complete batch; success is one undo edit, including on an
initially empty plate. Drops wait for an existing load/project-choice prompt.
A 3MF choice imports geometry or uses the existing replace/save/export/cancel flow.
Additional files following a project wait for that project's successful install.
No drop path creates another window. Command-line startup retains its prior policy.

The sliced-stack preview has at most 48 evenly sampled historical bands. Each
starts at the previous band's top, so there are no intentional air gaps; its top
comes from the actual schedule. The last band ends at the current layer's bottom.
The lower 18 percent of each band (at least one scheduled layer, clamped to the
band height) is a separate dark seam mesh. The current layer stays a blue slab.
The source contours and printer raster/export pipeline are unchanged. Existing
2 micrometre contour simplification remains. The shared PrimaryButton component
uses Sleptic theme tokens and handles Enter/Space for primary dialog actions.


## Canvas navigation and plate tools (R11)

Object presses first test transform handles, then object geometry. The existing
`GizmoFsm` captures `DragOp::Plate` with a world Z=0 anchor, original local
transform and press pixel. It waits for the 4 logical pixel drag threshold,
intersects each cursor ray with the same plane, optionally rounds the world
XY delta to 1 mm, and converts that delta through the parent inverse before
applying it to the captured original. Live edits use the existing document
mutation event; release pushes one undo entry, while Esc or locking the plate
reverts without committing. Empty presses return false to MeshViewer so its
existing left-button camera pan remains available.

`picking::{lands_on_plate,menu_for_click}` separates object, bounded plate rectangle
and global background menus. The object hit has priority. Plate actions reuse
the existing arrange, orientation, selection and compound deletion operations.
The single plate's lock is viewport state, with a controller guard on arrange,
fill, orientation and pointer transforms. It does not alter project geometry
or undo history. Plate settings opens Preferences directly on Printer.

The plate strip follows the rightmost projected build-plate corner and clamps
inside the viewport. The plate renderer adds three coloured line segments at
the minimum XY corner, using the cage line pipeline. The navigation cube is
orthographic camera-relative UI geometry: `viewport::navigation` projects
visible faces and performs convex polygon picking, then maps the face ID to
an existing axis view. Its XYZ tripod shares that rotation. Only angular
changes rebuild the cube model. Default and Isometric use the existing default
isometric framing; Zoom to fit uses the existing Frame view action.

Menu input state and keyboard navigation stay in MeshViewer; the exported
`CanvasMenuOverlay` receives its entries/anchor and routes activation back to
those callbacks. MainWindow paints this overlay after all canvas tools and before
busy/recovery/project dialogs, preventing projected plate icons from painting
over menu rows. The origin axes use a small clip-depth bias to remain visible
against their coincident cage edge. Desktop pixel checks cover both regressions.

## Unified print risks (R12)

`sc_print_job::analyze_layers` now returns a typed `PrintRisk` list alongside
volume and islands. `analyze_layers_with_settings` accepts physical thresholds:
minimum feature 0.3 mm, peel area 2,000 mm2, and adjacent-layer area change
500 mm2. It uses each corrected raster produced by the existing single pass.
No full raster stack is retained. Nonzero grayscale spans are coalesced before
connectivity and morphological operations, so AA run boundaries are not walls.

Empty space is labelled with 4-connectivity in XY and overlapping spans between
adjacent layers (6-connectivity in XYZ). Each component carries exterior
connectivity through unions, including splits, later joins and side openings.
The first and last empty planes and XY frame boundary connect to the outside.
An enclosed component born above solid material is a suction cup; its warning
survives a later top opening. Components never connected to outside are trapped
resin cavities. A closed hollow box can therefore have both diagnostics.
Cups retain separate warning records after opening into the common exterior.

Thin features are the difference between lit material and rectangular erosion
followed by reconstruction at the physical minimum's half-width. This detects
vanishing fins and necks without marking every normal wall edge. Physical X/Y
pixel sizes are independent. This is a raster-resolution diagnostic, with
pixel-sized uncertainty and a rectangular structuring element, not a mechanical
strength guarantee. Highlights recompute thin spans only for the viewed layer.
Peel warnings group consecutive offending layers and retain the worst layer.

`sc_print_glue::scene_placement_risks` supplies original baked model bounds and
per-leaf vector stacks to the pure `sc_print_job::risks::placement_risks`.
Bounds are checked before raster clipping, including Z and negative coordinates.
Overlap uses polygon material interiors, excluding holes and boundary-only
contacts; generated supports are excluded from intentional model contacts.
No second per-leaf raster pass is needed.

Preview consumes the same sorted report as CLI `slice --risks`. Rows group by
kind, show ranges/plate positions, expand explanations and fixes, and jump to
the worst layer in 2D. Errors use the theme's red and warnings its yellow.
The status bar counts errors. The minimum-feature field is a per-session
analysis setting; changing it requires Slice to refresh the report. Stale
reports disable fix actions. Auto-support fixes reuse the existing worker.
Drain fixes choose the nearest model floor under the actual cavity sample.
Editable hollows retain their source and hole metadata through the existing
rebuild; imported natural cups use `hollow::drill_holes`, which subtracts only
channels from existing material. Both paths use the existing bounded voxel
grid and one undoable edit. A direct cut clears obsolete hollow metadata;
undo restores the original mesh and metadata. Re-slice after fixes verifies
whether the risk was resolved.


## Automation recipes (R16)

`sc_print_profiles::recipes` owns the version-1 ordered TOML schema, parameter
validation, two bundled defaults and the user library. The normal library lives
under the platform application data directory's `recipes` subfolder;
`SLEPTIC_RECIPES_DIR` redirects it for portable use and verification. Recipe
filenames are independent of display names. Explicit files or unique display
names resolve through the same loader in the CLI and GUI. Malformed files and
ambiguous names produce explicit errors. Bundled defaults cannot be overwritten
or deleted. User copies are independent files.

`apps/slicer-cli/src/recipe/service.rs` is AGPL application code shared by both frontends.
Execution snapshots a scene, isolates requested models and their owned supports
by plate, runs each step transactionally, and records named failures while
continuing later steps. Locked targets refuse the run. Slicing and export use
the ordinary slicing engine, support completion, profile resolver and format
writers. Existing export targets are refused. Cancellation is checked between
steps and passed into the slicer; completed scene edits are discarded on cancel.
Already exported files are external outputs and are not undone.

The GUI worker returns one compound edit, node-slot count and summary. It commits
only if the document revision still matches the captured revision. One Undo
restores all recipe scene edits. The editor exposes step insertion, removal and
ordering plus the full TOML parameters; Save copy duplicates and Save updates a
user entry. Batch prepare can use the selected recipe for each file.

Preview 2D reserves a separate graph column: the plate's clipped viewport ends
380 px before the window's right edge, the graph begins 362 px before that edge,
and the slider occupies the next 40 px. The orange `ShellTheme.cut-plane`
(#e46b26) denotes an editable plane; `ShellTheme.warning` (#e5a500) remains the
yellow stale-support warning. Blue on a model remains the current-layer cap;
selection and keep-paint semantics are unchanged.
