# TODO: oxigeo-3d

> **Purpose:** 3-D geospatial — point clouds (LAS/LAZ, COPC, EPT), TINs, meshes (OBJ, glTF 2.0/GLB), 3D Tiles (Cesium), classification (ground/vegetation/building).
> **Status (2026-05-16):** 4,585 LoC · 78 tests · 9 real stubs in `pointcloud/copc.rs` (VLR parse, hierarchy read, LAZ decompression, header parse), `pointcloud/ept.rs:279` (decompression dispatch), `classification.rs:300` ("simplified planarity"), `terrain/dem_to_mesh.rs:438` ("simplified mesh" test comment).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Real COPC reader — parse COPC VLR + hierarchy pages
  - **Verified gap:** `src/pointcloud/copc.rs:287-289` — `// Simplified: In real implementation, parse VLRs from LAS header / // For now, return default info / Ok(CopcInfo { … })`; `:304-306` `fn read_hierarchy(_file, _info) -> Result<CopcHierarchy> { // Simplified: In real implementation, read hierarchy pages / Ok(CopcHierarchy::new()) }`; `:407-408` `// Simplified: In real implementation, parse LAS header and COPC VLR`.
  - **Goal:** A working COPC 1.0 reader that parses the `copc.org/copc-info` VLR (user-id `copc`, record-id 1) into a real `CopcInfo` (centre, halfsize, spacing, root_hier_offset/size), then walks the hierarchy-page chain (user-id `copc`, record-id 1000) to populate `CopcHierarchy`. Per COPC spec 1.0 (https://copc.io/, 2022-02 release).
  - **Design:**
    1. Iterate `las::Header::vlrs()`; locate `copc` user-id; deserialize 160-byte payload as little-endian `CopcInfo` matching the LAS 1.4 VLR layout. Use `byteorder` (already a dep).
    2. Seek to `root_hier_offset`; read `root_hier_size` bytes; deserialize as `Vec<HierarchyEntry { key: VoxelKey, offset: u64, byte_size: i32, point_count: i32 }>` (32-byte entries).
    3. Follow `byte_size < 0` entries to child pages recursively.
  - **Files:** `crates/oxigeo-3d/src/pointcloud/copc.rs` (replace lines 286-307 and 405-410 with real impl).
  - **Tests:** (proposed) `test_copc_parse_real_vlr_payload`, `test_copc_walk_root_hierarchy_page`, `test_copc_walk_nested_page_via_negative_byte_size`, `test_copc_missing_vlr_returns_error`, `test_copc_info_roundtrip_little_endian`.
  - **Risk:** Fixture: small COPC file (~1 MB) bundled under `tests/data/` or generated via offline tool.
  - **Prerequisites:** None.
  - **Done:** 2026-05-22 (Slice 26). New `src/pointcloud/copc_vlr.rs` (~326 LoC): `COPC_USER_ID = "copc"`, `COPC_INFO_RECORD_ID = 1`, `COPC_HIERARCHY_RECORD_ID = 1000`, `CopcInfoVlrPayload` (160-byte LE struct), `VoxelKey { level, x, y, z }` (16B), `HierarchyEntry { key, offset, byte_size, point_count }` (32B), `parse_copc_info(payload, payload.len() >= 160)`, `parse_hierarchy_page(bytes)` (N × 32B), `find_copc_info_vlr(header)` walking both VLRs + EVLRs via `las::Header::all_vlrs()`. `pointcloud/copc.rs:287-307` two function bodies replaced (signatures byte-for-byte unchanged): `read_copc_info` locates the VLR, parses the payload, and populates the existing `CopcInfo` struct; `read_hierarchy` seeks to `root_hier_offset`, reads `root_hier_size` bytes, parses entries, recurses into child pages on `byte_size < 0`, bound at MAX_DEPTH=32 with `Error::hierarchy_recursion_limit()` on overflow. `error.rs` +21 lines (3 helper constructors `missing_copc_vlr` / `malformed_copc_info` / `hierarchy_recursion_limit` reusing existing `Error::Copc(String)` variant — zero new enum variants). `pointcloud/mod.rs` +10 lines re-exports. Public API requires `--features copc` (existing crate feature).
  - **Tests:** 10 in `crates/oxigeo-3d/tests/copc_vlr_test.rs` (canonical 160-byte payload; wrong-length errors; LE round trip; find-VLR matching; find-VLR returns None; hierarchy single entry; hierarchy multiple entries decoded in order; voxel-key extraction; nested page walk via negative byte_size; missing-VLR typed error). Full crate suite 100/100 (90 pre-existing + 10 new).

- [ ] LAZ decompression in Pure Rust (replace placeholder dispatch)
  - **Verified gap:** `src/pointcloud/copc.rs:342-343` — `// Simplified: In real implementation, use LAZ decompression`; `:494-495` same comment; `src/pointcloud/ept.rs:279` — `// Simplified: In real implementation, handle laszip, binary, or zstandard`.
  - **Goal:** Wire `laz = "0.12"` (already a workspace dep, Pure Rust) to decompress LAZ chunks read out of COPC/EPT containers into `Vec<Point>` of the active point-record format.
  - **Design:** Per voxel/octant: read `byte_size` bytes; pass to `laz::LasZipDecompressor::new(reader, vlr)` constructed from the LAZ-vlr (user-id `laszip encoded`, record-id 22204) extracted alongside the COPC VLR; iterate `decompressor.decompress_one()` until `point_count` reached.
  - **Files:** `crates/oxigeo-3d/src/pointcloud/copc.rs` (replace `read_voxel` decompression paths), `src/pointcloud/ept.rs` (replace the format dispatch).
  - **Tests:** (proposed) `test_laz_decompress_format0_matches_las_baseline`, `test_laz_decompress_format6_extended`, `test_laz_chunk_table_iteration`, `test_laz_corrupted_chunk_errors`.
  - **Risk:** `laz` crate version skew with `las` 0.x — pin the matching version per workspace.
  - **Prerequisites:** Item 1.

- [ ] Delaunay triangulation for TIN generation from point clouds
  - **Goal:** 2.5-D TIN constructor `create_tin(points: &[Point]) -> Tin` using `delaunator` (already a workspace dep). Output: vertex list + triangle indices, with z preserved.
  - **Design:** Project XY to `Vec<delaunator::Point>`, call `delaunator::triangulate`, then build `Tin { vertices: Vec<[f64;3]>, triangles: Vec<[u32;3]> }`. Optional `simplify(max_error)` post-pass via greedy edge collapse with quadric error metric (Garland & Heckbert 1997).
  - **Files:** `crates/oxigeo-3d/src/terrain/tin.rs` (extend or new).
  - **Tests:** (proposed) `test_tin_grid_points_produces_n_minus_2_triangles_per_row`, `test_tin_collinear_points_returns_empty_triangulation`, `test_tin_preserves_z_at_vertices`, `test_tin_export_to_obj_roundtrip_vertex_count`.
  - **Risk:** Delaunator returns convex-hull triangulation; for non-convex terrain, downstream code must clip.
  - **Prerequisites:** None.

- [x] Proper planarity in ground/feature classification
  - **Verified gap:** `src/classification.rs:300` — `// Simplified planarity: ratio of smallest to largest eigenvalue`.
  - **Goal:** Replace the eigenvalue-ratio heuristic with the standard PCA-derived planarity index `P_λ = (λ₂ - λ₃) / λ₁` where `λ₁ ≥ λ₂ ≥ λ₃` are eigenvalues of the local 3×3 covariance matrix (Demantké et al. 2011, "Dimensionality based scale selection in 3D LiDAR point clouds").
  - **Design:** For each candidate point, gather `k` nearest neighbours (`rstar` already in deps); build 3×3 covariance; compute eigenvalues via closed-form 3×3 SVD or `scirs2-core::linalg::SymmetricEigen`; compute `P_λ`, `S_λ = λ₃/λ₁` (sphericity), `L_λ = (λ₁ - λ₂)/λ₁` (linearity). Threshold `P_λ > 0.7` for ground candidates.
  - **Files:** `crates/oxigeo-3d/src/classification.rs` (replace line 300 logic).
  - **Tests:** (proposed) `test_planarity_perfectly_planar_returns_near_one`, `test_planarity_spherical_cluster_returns_near_zero`, `test_planarity_linear_cluster_returns_low`, `test_classification_ground_vs_vegetation_separation`.
  - **Risk:** Closed-form 3×3 SVD numerical stability for near-degenerate matrices — fall back to scirs2-core LAPACK path.
  - **Done:** 2026-05-22 (Slice 27). `src/classification.rs` planarity computation replaced (+290/-11): new private `symmetric_eig_3x3` (closed-form analytic eigenvalues of a symmetric 3×3 matrix, Smith 1961 — `p1≈0` diagonal fast-path, `acos` arg clamped to `[-1,1]`, non-finite guard) and `dimensionality_features` returning `pub(crate) DimensionalityFeatures { linearity, planarity, sphericity }` per Demantké et al. 2011 (`P_λ=(λ₂-λ₃)/λ₁`, `L_λ=(λ₁-λ₂)/λ₁`, `S_λ=λ₃/λ₁`; λ₁ denominator guarded → degenerate cluster yields all-zero). The planarity call site returns `dimensionality_features(&cov).planarity`; surrounding function signature byte-for-byte unchanged; dependency-free (no `scirs2` eigensolver needed); `classification` is not feature-gated.
  - **Tests:** 11 (9 inline `#[cfg(test)]` units in `classification.rs` — eigensolver diagonal/known/sorted, planarity planar/spherical/linear, linearity, sphericity, degenerate-all-zero — + 2 integration in `tests/planarity_test.rs` — ground-vs-vegetation separation, custom-threshold). Full crate suite 93/93.
  - **Prerequisites:** None.

- [ ] glTF 2.0 / GLB binary export with embedded textures
  - **Goal:** Export `Mesh` to GLB (binary glTF 2.0, per Khronos glTF 2.0 spec §4.4.3) with `BIN` chunk containing positions/normals/UVs/indices, and embedded texture(s) referenced from `materials[].pbrMetallicRoughness.baseColorTexture`.
  - **Design:** Build `gltf_json::Root` from `Mesh`; emit positions/indices/normals/UVs accessors; write images as PNG into the BIN chunk via `image` crate (already in workspace); GLB header (12B) + JSON chunk + BIN chunk per `glTF-2.0/#binary-gltf-layout`. Use `bytemuck` (already a dep) for accessor data.
  - **Files:** `crates/oxigeo-3d/src/mesh/gltf.rs` (extend existing `export_gltf`).
  - **Tests:** (proposed) `test_glb_magic_and_version`, `test_glb_chunk_alignment_4byte`, `test_glb_roundtrip_via_gltf_import`, `test_glb_embedded_png_texture_referenced`, `test_glb_pbr_material_baseColor`.
  - **Risk:** Workspace `image` crate version drift; pin and document.
  - **Prerequisites:** Item 3 (TIN → Mesh path) for end-to-end.

- [ ] 3D Tiles (Cesium) tileset.json with content hierarchy
  - **Goal:** Generate `tileset.json` per Cesium 3D Tiles 1.1 spec (https://github.com/CesiumGS/3d-tiles, OGC standard 22-025r4) with a root tile referencing GLB content and refinement strategy `REPLACE`. Bounding volume: oriented bounding box.
  - **Design:** `create_3d_tileset(meshes: &[Mesh], lods: &[u32]) -> Tileset` that writes `tileset.json` + per-LOD `.glb`. Compute geometric error from mesh extent; emit `content.uri`; recurse via `children[]` for LOD hierarchy.
  - **Files:** `crates/oxigeo-3d/src/visualization/tileset.rs` (extend or new).
  - **Tests:** (proposed) `test_tileset_json_schema_valid`, `test_tileset_geometric_error_monotonic_decreasing_per_lod`, `test_tileset_bounding_volume_box_alignment`, `test_tileset_external_content_uri_referenced_from_root`.
  - **Risk:** Cesium 1.1 vs 1.0 minor differences — emit `asset.version = "1.1"` and document.
  - **Prerequisites:** Item 5 (GLB export).

## Medium Priority
- [ ] DEM-to-mesh with configurable LOD levels (Stoter et al. 2020).
  - **Files:** `src/terrain/dem_to_mesh.rs` (extend; previous TODO mentions "simplified mesh" at line 438).
  - **Why deferred:** Lower priority than TIN path.
- [ ] OBJ export with MTL material file.
  - **Files:** `src/mesh/obj.rs` (extend `export_obj`).
  - **Why deferred:** glTF/GLB is the modern path (Item 5).
- [ ] Ground classification via cloth-simulation filter (Zhang et al. 2016 CSF, RemoteSens 8(6)).
  - **Files:** `src/classification.rs` (extend).
  - **Why deferred:** Adds significant code; current PCA heuristic (Item 4) is the first step.
- [ ] Building footprint extraction from classified point clouds.
  - **Files:** `src/classification/buildings.rs` (new).
  - **Why deferred:** Needs ground classification mature first.
- [ ] Vegetation height model (CHM) from normalized clouds.
  - **Files:** `src/classification/vegetation.rs` (new).
  - **Why deferred:** Niche; common in forestry workflows only.
- [ ] Progressive mesh simplification (edge collapse + quadric error per Garland & Heckbert 1997).
  - **Files:** `src/mesh/simplify.rs` (new).
  - **Why deferred:** Useful for LOD pipeline (Item 6).
- [ ] Texture mapping from orthophoto onto terrain mesh.
  - **Files:** `src/mesh/texture.rs` (new).
  - **Why deferred:** Needs camera/projection abstraction.

## Low Priority / Future (one-liners)
- [ ] EPT (Entwine Point Tiles) full reader — currently stubbed in `ept.rs:279`.
- [ ] Implicit surface reconstruction (Poisson, Kazhdan & Hoppe 2013).
- [ ] CityGML / CityJSON export for urban 3D models.
- [ ] Viewshed analysis on 3D mesh surfaces.
- [ ] IFC (Industry Foundation Classes) basic geometry import.
- [ ] 3D Tiles Next with glTF structural metadata.
- [ ] Point-cloud colorization from aerial imagery.
- [ ] Draco mesh compression (note: requires Pure-Rust impl; `oxiarc-*` does not yet cover draco).

## Cross-crate dependencies
- **Blocks:** oxigeo-copc (consumer of COPC reader), oxigeo-services (3D tile serving).
- **Blocked by:** `las` crate version pin, `delaunator` (workspace dep).

## Recently completed (verbatim)
*(No `[x]` entries on previous TODO.)*

---
*Last audited: 2026-05-17*
