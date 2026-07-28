# TODO: oxigeo-noalloc

> **Purpose:** `no_std`, `no_alloc` fixed-size geometry primitives for OxiGeo — for embedded, RISC-V, and Redox OS environments.
> **Status (2026-07-28):** 2,310 Rust LoC · 136 tests · 0 real stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v0.2.1 → v1.0.0

## High Priority (next slice — verified gaps)

- [ ] Add Haversine great-circle distance for `Point2D` interpreted as (lon, lat)
  - **Verified gap:** `/opt/homebrew/bin/rg -n "fn .*haversine" src` returns no matches. `src/lib.rs:41-43` re-exports stop at `mercator_forward`/`mercator_inverse`/`FixedRing` — no `haversine_distance`.
  - **Goal:** `haversine_distance(p1: Point2D, p2: Point2D, radius: f64) -> f64` returning great-circle distance on a sphere of given radius (metres for Earth `EARTH_RADIUS_M = 6_371_008.8`).
  - **Design:** Sinnott 1984 *Virtues of the Haversine*. Formula: `a = sin²(Δφ/2) + cos(φ₁)·cos(φ₂)·sin²(Δλ/2)`; `c = 2·atan2(√a, √(1−a))`; `d = R·c`. Uses our existing no-std `libm_sin`, `libm_cos`, `libm_sqrt`, `libm_atan` (already in `lib.rs:617-924`). atan2 derives from atan via quadrant analysis — add `pub(crate) fn libm_atan2(y: f64, x: f64)`.
  - **Files:** `(new) src/distance.rs`; `src/lib.rs` (module declaration, re-export, add `libm_atan2` helper near other libm functions)
  - **Tests:** `(proposed)` test_haversine_equator_one_degree_111km, test_haversine_pole_to_pole_half_circumference, test_haversine_antipodal_pi_radius, test_haversine_self_zero, test_haversine_symmetry (d(p1,p2) == d(p2,p1)), test_libm_atan2_all_quadrants
  - **Risk:** atan2 numerical accuracy on axis-aligned inputs; verify with reference values at (1,0), (0,1), (-1,0), (0,-1).
  - **Prerequisites:** None — libm shims exist.

- [ ] Implement UTM zone determination and forward/inverse projection (no_std, pure arithmetic)
  - **Verified gap:** `/opt/homebrew/bin/rg -n "fn .*utm" src` returns nothing. `src/lib.rs:25-26` documents only Web Mercator.
  - **Goal:** `utm_zone(lon: f64) -> u8` (1..=60), `utm_forward(lon, lat) -> (zone, hemisphere, x, y)`, `utm_inverse(zone, hemisphere, x, y) -> (lon, lat)`. Spherical-Earth simplification acceptable for embedded use; document accuracy ~few-hundred metres vs ellipsoidal.
  - **Design:** Snyder 1987 *Map Projections — A Working Manual* (USGS Prof Paper 1395) §8 Transverse Mercator. Zone = floor((lon+180)/6) + 1. Central meridian λ₀ = (zone - 1)·6 - 180 + 3. Spherical TM forward: `x = k₀·R·atanh(B)`, `y = k₀·R·[atan(tan(φ)/cos(λ−λ₀)) − φ₀]`, where `B = cos(φ)·sin(λ−λ₀)`. False easting 500000 m; false northing 10000000 m for southern hemisphere. Need new `libm_atanh` (one-term series since |B| < 1).
  - **Files:** `(new) src/utm.rs`; `src/lib.rs` (declare module, add `libm_atanh`)
  - **Tests:** `(proposed)` test_utm_zone_grenwich_is_31, test_utm_zone_180_meridian_boundary, test_utm_forward_inverse_round_trip_within_1m, test_utm_southern_hemisphere_false_northing, test_utm_zone_1_minus_180_to_minus_174, test_utm_polar_input_errors
  - **Risk:** Spherical approximation accuracy declines toward zone edges (|λ−λ₀| → 3°); document and provide error budget. For ellipsoidal precision use `oxigeo-proj` (std-only).
  - **Prerequisites:** Item 1 prerequisite `libm_atan2` is useful here too.

- [ ] Implement `FixedGridIndex<const N: usize, const M: usize>` for no_alloc spatial queries
  - **Verified gap:** `src/lib.rs:32-42` module list — `bbox3d`, `geohash`, `linestring`, `mercator`, `ring`. No spatial index module.
  - **Goal:** `FixedGridIndex<N, M>` indexing up to `M` entries across `N×N` grid cells, all stored inline. Each entry occupies a fixed `(BBox2D, T)` slot; multiple entries per cell tracked via fixed per-cell head pointers + entry next-pointers.
  - **Design:** Single contiguous array `entries: [Option<Entry<T>>; M]` plus `cell_heads: [Option<u16>; N*N]` (`u16` index into entries). Each entry stores `next: Option<u16>` for chaining. `insert` finds touched cells, appends entry with next-pointer to current head. `search` walks chain in each touched cell, dedups by entry index. No allocation, no heap.
  - **Files:** `(new) src/grid_index.rs`; `src/lib.rs` (declare module + re-export)
  - **Tests:** `(proposed)` test_fixed_grid_index_insert_search_simple, test_fixed_grid_index_capacity_exceeded_errors, test_fixed_grid_index_multi_cell_entry, test_fixed_grid_index_search_dedup, test_fixed_grid_index_const_generic_compile_combinations
  - **Risk:** `u16` cell head limits to 65,535 entries — adequate for embedded; document. Two-level const generic may stress old compilers; require Rust ≥ 1.59 (already in workspace `rust-version`).
  - **Prerequisites:** None — `Bbox2D` and `Point2D` already in lib.rs.

## Medium Priority (planned — design sketched)

- [ ] Implement Vincenty inverse distance formula for geodesic accuracy on the ellipsoid
  - **Goal:** Vincenty 1975 *Direct and Inverse Solutions of Geodesics on the Ellipsoid* — accurate to mm on WGS84 ellipsoid.
  - **Files:** `src/distance.rs` (extend Item 1)
  - **Why deferred:** Iterative; convergence failure at antipodal points needs special-case handling.

- [ ] Add fixed-capacity point cloud type (`FixedPointCloud<N>`) with bbox, centroid, and k-nearest for `Point3D`
  - **Files:** `(new) src/point_cloud.rs`
  - **Why deferred:** k-NN over Point3D is O(N²) without index; ship after `FixedGridIndex` (Item 3) generalises to 3D.

- [ ] Add matrix inverse for `CoordTransform` (compute the reverse affine mapping)
  - **Goal:** `CoordTransform::inverse() -> Result<CoordTransform, NoAllocError>` using 2×2 cofactor expansion (a*e − b*d, fail on |det| < ε).
  - **Files:** `src/lib.rs` (extend `CoordTransform` impl at ~line 513)
  - **Why deferred:** Trivial to add; bundle with related linear-algebra additions.

- [ ] Implement `FixedPolygon` point-in-polygon test (ray casting on the inline vertex array)
  - **Files:** `src/lib.rs` (extend `FixedPolygon<N>` at ~line 368)
  - **Why deferred:** Already have `FixedRing::contains_point`; FixedPolygon needs hole handling.

- [ ] Add convex hull computation for `FixedPolygon` (Graham scan variant operating in-place on fixed arrays)
  - **Files:** `(new) src/hull.rs`
  - **Why deferred:** Graham scan needs in-place sort by polar angle without alloc.

- [ ] Implement `libm_atan2` and `libm_asin` for no_std (needed for bearing/azimuth and inverse projections)
  - **Files:** `src/lib.rs` (extend libm shims at ~line 873)
  - **Why deferred:** Folded into Item 1 (Haversine) — atan2 ships as part of that.

- [ ] Add `FixedMultiPoint<N>` type for storing multiple points without allocation
  - **Files:** `(new) src/multipoint.rs`
  - **Why deferred:** Pattern identical to `FixedLineString`; cheap to ship after Item 3.

## Low Priority / Future (speculative — one-liners only)

- [ ] Add RISC-V soft-float verification (ensure `libm_sqrt`/`libm_sin`/`libm_cos` produce correct results on rv32imac).
- [ ] Implement S2 cell ID encoding/decoding as an alternative to geohash.
- [ ] Add no_std WKB (Well-Known Binary) encoder/decoder operating on fixed-size buffers.
- [ ] Implement fixed-size GeoJSON coordinate serialization (write `[x,y]` to `&mut [u8]` without alloc).
- [ ] Add compile-time unit tests via `const_assert` for geometry invariants.
- [ ] Implement no_std tile coordinate computation (`lonlat_to_tile`, `tile_to_bbox` using only core math).
- [ ] Add Redox OS compatibility testing in CI.
- [ ] Implement fixed-capacity R-tree (`FixedRTree<N, M>`) for embedded spatial indexing.
- [ ] Add benchmark suite comparing no_alloc implementations against std counterparts for performance parity.
- [ ] Implement coordinate quantization (f64 to i32 with scale/offset) for compact storage in constrained environments.

## Cross-crate dependencies

- **Blocks:** Embedded / RISC-V / Redox OS targets (downstream-only; no workspace crate forces dependency).
- **Blocked by:** None — zero workspace dependencies (true leaf crate, intentional `no_std + no_alloc`).

## Recently completed (verbatim)

- [x] Add FixedLineString<N> type (completed 2026-04-19, part of N1)
- [x] Implement FixedRing<N> type (completed 2026-04-19, part of N1)
- [x] Add BBox3D type (completed 2026-04-19, part of N1)
- [x] Add Mercator projection (completed 2026-04-19, part of N1)
- [x] Implement geohash neighbour computation (completed 2026-04-19, part of N1)

---
*Last audited: 2026-07-28*
