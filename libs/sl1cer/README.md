<!-- SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0 -->
# sc-sl1cer

Horizontal contour slicing and sparse grayscale rasterization for resin printing.

The public entry points accept checked print profiles and world-space scene
leaves. Validation precedes intersection. Face/layer buckets schedule only
eligible triangles; layers execute independently through Rayon. Strict contour
endpoints join through indexed mesh edges and vertex fans. Tolerant recovery
uses directed segments and a bounded endpoint search. Multiple boundaries use
an integer nonzero material union on a 0.00001 mm grid.

Rasterization maps contours into continuous pixel-edge coordinates, indexes
edges by occupied row, and integrates horizontal coverage on vertical AA sample
lines. Constant-coverage intervals become RLE runs directly. Default processing
needs no dense image; bottom erosion uses separable sliding minima when enabled.
Calibration, geometric compensation, erosion, and grayscale reduction belong to
the raster stage and leave the contour stack unchanged.

The compatibility and numerical contract is `docs/rewrite/sl1cer.md` at the
workspace root. Build and test offline with `scripts\cargo-msvc.bat` on Windows.
