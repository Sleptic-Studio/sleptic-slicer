# Scene graphs

`sc-scene` stores editable geometry with stable scene-local identities. Nodes
and resources keep their allocation history across removal and restoration.
Cloning a scene copies its bookkeeping while sharing mesh buffers through `Arc`.

The three geometry scopes serve different callers:

- Structural walks follow roots and ordered children without expanding references.
- Expanded walks include each referenced subtree occurrence and its placement.
- Stored-mesh queries include detached leaves and unreferenced resources.

New nodes are detached. Attach them with `add_root` or `add_child`, or register
an unattached root as a subtree resource. CSG nodes describe operations; this
crate does not evaluate them. Consumers choose whether to refuse or descend.

Baking applies placement to new geometry, preserves mirrored winding, and omits
mesh metadata. Bounds include unused vertices. Plate grid offsets affect display
only; geometry remains in plate-local millimetres with Z up.

Repair assesses shared meshes before obtaining mutable storage; uniquely owned
meshes run the repair policy in place. Clean and refused targets keep their
allocations; shared targets needing work use copy-on-write.

The compatibility contract is documented in `docs/rewrite/scene.md` at the
workspace root. Run tests on Windows through `scripts\cargo-msvc.bat test -p
sc-scene --offline`.
