# Mesh core

Apache-2.0 geometry and indexed mesh operations for Sleptic Slicer. Positions
are right-handed, Z-up millimetres. Stored positions and normals use `f32`;
geometry calculations and affine transforms use `f64`.

The compatibility contract is `docs/rewrite/mesh-core.md` at the workspace root.
This implementation was written from that contract without historical source.

## Entry points

- `IndexedMesh` preserves public vertex and face ordering and optional metadata.
- `indexed_mesh_from_soup` assigns vertices in first-appearance order using exact
  component bits, with signed zeros sharing a key.
- Face witnesses establish index safety and distinct vertex identities only.
  Edge witnesses additionally establish two oppositely directed faces per edge.
- `diagnose`, `repair`, and `assess_repair` keep position, face, normal, and edge
  validity separate. Orientation repair plans every recomputation before mutation.
- `MeshTopology` owns sorted edge records and flat fan ranges independently of
  its source mesh.
- `simplify` uses deterministic quadric-ranked edge contractions with local link,
  duplicate, orientation, and original-plane displacement checks. The target is
  an objective; geometric constraints can stop reduction early.

## Implementation choices

Component constructors accept fixed-size arrays: `Vertex::new([x, y, z])`,
`Normal::new([x, y, z])`, and `Vector3D::new([x, y, z])`. Triangle constructors
accept a normal followed by a three-element vertex/index array. Translation
and nonuniform scale constructors likewise take arrays. This resolves the
contract's prose signatures in agreement with dependant compiler diagnostics.
Mutable indexing is retained for the array wrappers as a compatibility
convenience required by dependant tests.

At overloaded edges, distinct angular wedges nest. Coincident starts form an
ordered bundle consumed in face-index order. This resolves coincident wedge
ties so identical copies of a closed shell separate into closed sheets.
The angular zero vector is the cross product of the edge direction and the
least-aligned coordinate axis.

Simplification keeps disjoint vertex-owned chains of original face-plane
references. A contraction splices these chains without copying plane data.
Every accepted position is checked after rounding to stored `f32` coordinates.
Reported deviation is the maximum retained vertex-to-original-plane distance,
not a finite-triangle Hausdorff measurement. Independent neighborhoods are
locked for a round; accepted faces remain in original relative order.

Soup indexing uses a preallocated hash table and a word-at-a-time coordinate hasher;
coordinate keys retain signed-zero canonicalization and NaN bit identity.
Edge validation and topology bucket incidences by their lower vertex ID and
sort only within each bucket, with a comparison-sort fallback for sparse IDs.
Repair reuses edge records and unchanged validation results. Shell fans are
linked in first-corner order per vertex instead of globally sorted. Simplification uses flat
incidence arrays and sorted candidate batches per round, with cancellation
checks at most 32,768 processed candidates apart. Storage grows linearly with
vertices and faces. Rayon handles independent candidate scoring and sorting;
tie-breaking uses vertex identities so execution is deterministic.

## Checks

Run from the workspace root on Windows:

```text
scripts\cargo-msvc.bat test -p sc-mesh-core --offline
scripts\cargo-msvc.bat clippy -p sc-mesh-core --offline --all-targets -- -D warnings
```

The contract tests include exact diagnostics, transaction boundaries, signed
zero and NaN soup keys, pinch fans, coincident shells, welded cubes, property
checks, cancellation, and closed-surface simplification. The dense cube test
uses 40 subdivisions on each side of a 20 mm cube and a 0.02 mm tolerance.

An opt-in binary-STL phase probe reads `SC_MESH_PROFILE_STL` (no model is
bundled). Run `local_mesh_phases` with `--release -- --ignored --nocapture`
to time indexing, validation, topology when closed, orientation and full repair.
Performance contract tests also enforce one edge-record build for clean repair
and compare coordinate/index ordering against an independent hash-map oracle.
