# TODO: oxigeo-3d

> **Purpose:** 3-D geospatial — point clouds (LAS/LAZ, COPC, EPT), TINs, meshes (OBJ, glTF 2.0/GLB), 3D Tiles (Cesium), classification (ground/vegetation/building).
> **Status (2026-07-28):** 4,585 LoC · 131 tests · 1 remaining real stub: `pointcloud/ept.rs` EPT `laszip`-tile decoding (explicit typed `Unsupported` error, tested — not a silent stub). COPC VLR parse, hierarchy read, header parse, and COPC-container LAZ decompression are all real (`oxigeo_copc::decompress_chunk`); planarity uses a real PCA eigensolver.
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

- [x] Pure-Rust LAZ path for the plain `LasReader`/`LasWriter` (no async/reqwest stack)
  - **Verified gap:** the crate advertised LAZ read/write via the default `las-laz` feature, but `las-laz = []` was an empty no-op: `LasReader::open` called `las::Reader::new` with the `las` crate's own `laz` feature never enabled, so any compressed `.laz` returned `Error::LaszipNotEnabled`; `LasWriter` silently wrote uncompressed LAS regardless of extension. The only real LAZ decoder (`oxigeo-copc`) was reachable only through the `copc`/`ept` features, which pull `async → reqwest → rustls → aws-lc-sys` (C/asm).
  - **Done:** 2026-07-21. `crates/oxigeo-3d/Cargo.toml` `las-laz = ["las/laz"]` — forwards to the `las` crate's `laz` feature, backed by the pure-Rust `laz` v0.12 crate (already resolved in the workspace; `las` 0.9.11 depends on `laz` 0.12.0). No root `Cargo.toml` change, no C/C++/Fortran dependency, stays in `default`. `LasWriter::create` now sets `builder.point_format.is_compressed = true` for `.laz` targets so writes are genuinely LASzip-compressed; module doc corrected. New test `test_laz_roundtrip_compresses_and_decompresses` writes a `.laz`, asserts the on-disk LASzip VLR (`laszip encoded`) is present (proving real compression), then reads it back losslessly through `LasReader`.
  - **Note:** the COPC/EPT-container decompression path below is a separate concern (chunked LAZ inside COPC/EPT octree containers), still tracked as its own item.

- [ ] LAZ decompression for EPT `laszip` standalone tiles (COPC side done)
  - **Verified gap (updated 2026-07-28):** The `copc.rs` half of this item is DONE — `ChunkDecodeParams::is_laz` routes compressed chunks through the pure-Rust `oxigeo_copc::decompress_chunk` decompressor before deserialization (see doc comment at `copc.rs:79` and call site at `copc.rs:93`). The remaining gap is EPT: `src/pointcloud/ept.rs` explicitly returns `Error::Unsupported("EPT 'laszip' tiles require a standalone LAZ file reader that is not yet wired ...")` for the `laszip` data-type variant — an honest, tested error (`decode_laszip_tile_is_explicit_error`), not a silent stub, but the reader itself still doesn't exist. `binary` and `zstandard` EPT data types are both fully implemented (`parse_binary_points`, `oxiarc_zstd::decompress`).
  - **Goal:** Wire a standalone LAZ file reader (`laz = "0.12"`, already a workspace dep, Pure Rust — or reuse `oxigeo_copc`'s decompressor) for EPT's `laszip` tile variant.
  - **Files:** `crates/oxigeo-3d/src/pointcloud/ept.rs` (replace the `"laszip" => Err(...)` arm).
  - **Tests:** (proposed) `test_ept_laszip_tile_decodes_matches_baseline`, `test_ept_laszip_chunk_table_iteration`.
  - **Risk:** `laz` crate version skew with `las` 0.x — pin the matching version per workspace.
  - **Prerequisites:** None (COPC prerequisite already satisfied).

- [x] Delaunay triangulation for TIN generation from point clouds
  - **Done (verified 2026-07-28):** `src/terrain/tin.rs` — `create_tin(points: &[CloudPoint]) -> Result<Tin>` and `create_tin_from_points(points: &[TinPoint]) -> Result<Tin>` both use `delaunator::{Point, triangulate}` (workspace dep), converting the triangulation result into `TinTriangle`s and validating the resulting `Tin`. 7 tests in `tin.rs`.
  - **Risk (still applies):** Delaunator returns convex-hull triangulation; for non-convex terrain, downstream code must clip.

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

- [x] 3D Tiles (Cesium) tileset.json with content hierarchy
  - **Done (verified 2026-07-28):** `src/visualization/tiles3d.rs` — `create_3d_tileset(mesh: &Mesh, options: &TilesetOptions) -> Result<Tileset>` writes `tileset.json`. `Tile` carries `geometric_error`, `refine: Option<Refinement>` (default `Refinement::Replace`), and `children: Option<Vec<Tile>>` for hierarchy, with builder methods `with_refinement`/`with_children`. Covered by `test_tileset_json_roundtrip` and further round-trip tests.

## Medium Priority
- [x] DEM-to-mesh with configurable LOD levels (Stoter et al. 2020).
  - **Done (verified 2026-07-28):** `src/terrain/dem_to_mesh.rs` — `dem_to_lod_meshes(dem, options, num_levels)` generates a `Vec<Mesh>` across progressively halved simplification levels (`1 << level`) by calling `dem_to_mesh` per level. The "simplified mesh" text at the old line 438 was test-assertion wording (`test_dem_to_mesh_with_simplification`), not a stub marker.
- [x] OBJ export with MTL material file.
  - **Done (verified 2026-07-28):** `src/mesh/obj.rs` — `export_obj` writes `mtllib`/`usemtl` references and delegates to `write_mtl()` (private) when `mesh.material.texture.is_some()`, which emits a real `.mtl` file with `Ka`/`Kd`/`Ks` derived from `Material::base_color`/`metallic`.
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
*Last audited: 2026-07-28*
