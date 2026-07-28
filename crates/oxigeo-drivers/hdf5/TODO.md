# TODO: oxigeo-drivers/hdf5

> **Purpose:** HDF5 driver for OxiGeo - Pure Rust minimal HDF5 with optional full C-binding support
> **Status (2026-07-28):** 11,850 Rust LoC (incl. tests, as of last count) - 233 tests (all-features) - Superblock V2/V3 (done 2026-05-31), GZIP/shuffle/Fletcher32 chunk filters, and a contiguous-data hyperslab API are now implemented; full B-tree-driven chunked reads and VLen string (global-heap) support remain open below
> **Roadmap:** v0.1.7 - v0.2.1 (current slice) - v1.0.0

## High Priority (next slice - verified gaps)

- [x] Implement Superblock Version 2 / Version 3 parsing for modern HDF5 files
  - Done: 2026-05-31 (Slice 28). Tests: 9 new (superblock_v2_test) + 120 existing = 129 total.
  - **Verified gap:** `src/reader.rs:144-149` - `SuperblockVersion::V2 | SuperblockVersion::V3 => { Err(Hdf5Error::feature_not_available(format!("Superblock version {:?} (requires hdf5_sys feature)", version))) }`. The `SuperblockVersion` enum (`src/reader.rs:34`) declares V0/V1/V2/V3 but only V0/V1 are parsed in the `match` block. NetCDF-4 and most modern HDF5 files (HDF5 library >= 1.10) use V2 or V3 by default, so this is the dominant compatibility gap.
  - **Goal:** Open and read HDF5 files produced by HDF5 library 1.10+ which uses Superblock V2/V3 by default.
  - **Design:** Per HDF5 File Format Spec §III.A.1 ("Disk Format Level 1A2 - Superblock Version 2"): V2 layout is `[magic(8) || version(1) || size_of_offsets(1) || size_of_lengths(1) || flags(1) || base_address(o) || superblock_extension_address(o) || end_of_file_address(o) || root_group_object_header_address(o) || checksum(4)]` (where `o` = size_of_offsets). V3 adds File Consistency Flags but keeps the layout. Read the fields, locate the root group's object header by `root_group_object_header_address`; the existing V0/V1 path reads root via Symbol Table Entry, V2/V3 reads via Object Header directly. The 32-bit Jenkins checksum at the end must validate.
  - **Files:** `src/reader.rs` (extend `Superblock::read`), (new) `src/superblock_v2.rs` if file size grows past 2000 lines
  - **Tests:** (proposed) `test_superblock_v2_known_layout`, `test_superblock_v3_with_consistency_flags`, `test_superblock_v2_checksum_validation`, `test_superblock_v2_round_trip_via_writer`, `test_real_hdf5_110_file_opens`
  - **Risk:** Object Header v2 (linked from V2 superblock) is a different layout from v1 (linked from V0 superblock); need both. Spec: HDF5 File Format §IV.A.1.b vs §IV.A.1.a.
  - **Prerequisites:** None - error path returns `feature_not_available`, replacement is purely additive.

- [ ] Implement chunked dataset reading via B-tree traversal
  - **Re-verified 2026-07-28 — partially evolved, core gap remains:** `Hdf5Reader::decode_chunk(path, raw_chunk)` now exists (`src/reader.rs`) and is a real per-chunk primitive — it looks up the dataset's chunk dimensions and filter pipeline from real metadata and runs `pipeline.apply_reverse(raw_chunk, &datatype, &chunk_dims)` (or passthrough with no filters) to unfilter a chunk the *caller* supplies. What's still missing is the automatic path: locating a chunk's file offset/size via B-tree v1 traversal and driving that from a high-level "read this whole chunked dataset" call — no `btree_v1.rs` and no caller of `decode_chunk` that walks the index exists yet.
  - **Verified gap:** `src/chunking.rs` (18.9K) defines `ChunkIndex` types but the path from `Hdf5Reader` to actual chunked-dataset values is incomplete. `rg -n "fn .*read.*chunk|btree.*search|btree.*v[12]" -g '*.rs' src/` shows scaffolding only. `src/dataset.rs:370` defines `Dataset::chunked()` for builder use; the reader-side decode is not wired.
  - **Goal:** Read a chunked HDF5 dataset (the predominant layout for any non-trivial dataset; contiguous layout is only for small flat arrays).
  - **Design:** Per HDF5 spec §IV.A.2.b (B-tree v1, Chunked Raw Data Nodes): each dataset with chunked layout has a B-tree whose internal nodes index chunks by `[chunk_dim_size + 1]`-tuple keys (filter mask in last slot). Traverse: at each node, binary-search keys to find child containing target chunk; recurse until leaf; leaf entry gives `[size_on_disk, filter_mask, file_offset]`. Read that many bytes from file, pipe through filter chain (gzip, shuffle, fletcher32) in reverse order specified in dataset's Filter Pipeline message.
  - **Files:** `src/chunking.rs`, `src/reader.rs`, (potentially new) `src/btree_v1.rs`
  - **Tests:** (proposed) `test_chunked_read_2d_array`, `test_chunked_read_with_gzip_filter`, `test_chunked_read_btree_multilevel`, `test_chunked_read_partial_chunk_at_edge`, `test_chunked_read_with_shuffle_then_gzip`
  - **Risk:** B-tree v1 has subtle key comparison rules (lexicographic on chunk_offset tuple, with filter_mask ignored in comparison); validate against real fixtures.
  - **Prerequisites:** Superblock V2/V3 item above (most chunked HDF5 files are 1.10+ vintage).

- [x] Add GZIP decompression for chunked datasets via oxiarc-archive
  - **Done:** `src/filters/mod.rs` now implements `FilterId::Deflate` end-to-end: `apply_deflate_forward` calls `oxiarc_archive::gzip::compress(data, level)` and `apply_deflate_reverse` calls `oxiarc_archive::gzip::decompress(&mut reader)`, both dispatched from `Pipeline::apply_reverse`/forward via the `apply_filter_reverse`/`apply_filter_forward` match on `FilterId`. `standard_compression(level, element_size)` builds a shuffle+deflate pipeline by default. No `flate2` dependency — matches the COOLJAPAN Pure-Rust `oxiarc-*` policy.
  - **Files:** `src/filters/mod.rs`.

- [ ] Implement variable-length string reading (VLen string heap dereference)
  - **Re-verified 2026-07-28:** Still open — no `src/global_heap.rs`, no `global_heap`/`GlobalHeap` symbol anywhere in `src/`.
  - **Verified gap:** `src/datatype.rs:139` declares `VarLen { base_type }` and `src/datatype.rs:161` declares `VarString { .. }`, both with stub size `16` bytes (heap reference). However the actual heap dereference path on read is not present. `rg -n "global_heap|heap_id|vlen_data" -g '*.rs' src/` returns no matches.
  - **Goal:** Reading a dataset with `H5T_STRING` + `H5T_STR_NULLTERM` + variable size returns the actual string contents per element.
  - **Design:** Per HDF5 spec §III.E (Global Heap): VLen values are 16-byte heap references `[length(4) || global_heap_address(o) || heap_index(4)]`. Read the global heap collection at the referenced address (variable-length blocks indexed by `heap_index`). For VarString, treat the heap-resolved bytes as UTF-8.
  - **Files:** (new) `src/global_heap.rs`, `src/datatype.rs` (decode), `src/reader.rs`
  - **Tests:** (proposed) `test_vlen_string_simple`, `test_vlen_string_unicode_utf8`, `test_vlen_string_empty`, `test_vlen_string_across_heap_objects`
  - **Risk:** Global Heap collection layout requires careful offset arithmetic.
  - **Prerequisites:** Chunked reading not strictly required; VLen data can be in contiguous datasets too.

- [x] Implement hyperslab selection (partial / sub-region reading)
  - **Done (contiguous/decoded data; chunk-aware path still open):** The literal verified gap — "high-level `Hdf5Reader` API takes no slice arguments" — is resolved: `pub fn read_slice(&mut self, path: &str, start: &[usize], count: &[usize]) -> Result<Vec<u8>>` now exists on `Hdf5Reader` (`src/reader.rs`), validates the slice against dataset bounds, and gathers the sub-region via `gather_contiguous_slice`. Honest limitation: it only works when `dataset.data()` is already decoded to an in-memory byte buffer; for a dataset whose datatype/layout isn't decoded that way (in particular genuinely chunked datasets, pending the B-tree-traversal item above), it returns a clear `feature_not_available` error rather than wrong or partial data. Efficient chunk-aware hyperslab reads (touching only intersecting chunks, per the original design) remain future work.
  - **Files:** `src/reader.rs`.

## Medium Priority (planned - design sketched)

- [x] Shuffle filter (filter id 2) for chunked data
  - **Done:** `apply_shuffle_forward`/`apply_shuffle_reverse` in `src/filters/mod.rs` implement the real byte-transpose algorithm (interleave/de-interleave by `element_size`), dispatched from the filter pipeline via `FilterId::Shuffle`. `Filter::shuffle(element_size)` is used by default in `Pipeline::standard_compression`.
  - **Files:** `src/filters/mod.rs`.

- [x] Fletcher32 checksum (filter id 3) verification
  - **Done:** `apply_fletcher32_forward`/`apply_fletcher32_reverse` in `src/filters/mod.rs` compute and append/verify a real Fletcher-32 checksum (`calculate_fletcher32`), returning `Hdf5Error::ChecksumMismatch { expected, actual }` on mismatch rather than silently accepting corrupt data. `Pipeline::with_checksum()` builds a Fletcher32-only pipeline.
  - **Files:** `src/filters/mod.rs`.

- [ ] Compound datatype reading (structs with named fields)
  - **Goal:** Read structured records; `src/datatype.rs:117` already declares `Compound { members: Vec<CompoundMember> }`.
  - **Files:** `src/datatype.rs`, `src/reader.rs`
  - **Why deferred:** Lower volume use case; types are declared.

- [ ] Soft link and hard link traversal in groups
  - **Goal:** Walk groups that contain links to other paths.
  - **Files:** `src/group.rs`
  - **Why deferred:** Niche.

- [ ] Dimension Scale convention (NetCDF-4 layer above HDF5)
  - **Goal:** Recognise CLASS / DIMENSION_LIST / REFERENCE_LIST attributes that mark coordinate datasets.
  - **Files:** (new) `src/conventions/dimension_scale.rs`
  - **Why deferred:** Used by NetCDF-4 wrapper; cross-crate concern.

- [ ] HDF-EOS metadata parsing for satellite data products
  - **Goal:** Recognise `HDFEOS_*` group structure for MODIS / NOAA products.
  - **Files:** (new) `src/hdfeos.rs`
  - **Why deferred:** Convention-only; depends on group hierarchy.

- [ ] Virtual Dataset (VDS) read support
  - **Goal:** `src/vds.rs` defines VDS mappings; reader-side dispatch needed.
  - **Files:** `src/vds.rs`, `src/reader.rs`
  - **Why deferred:** Module exists; needs reader wiring.

- [ ] 64-bit object addressing for files > 2 GB
  - **Goal:** Honour `size_of_offsets` = 8 throughout (mostly works; verify on real files).
  - **Files:** `src/reader.rs`
  - **Why deferred:** Likely partially working; needs verification with test fixture.

- [ ] Object reference and region reference datatypes
  - **Goal:** Decode `H5T_REFERENCE` and `H5T_REF_OBJ`.
  - **Files:** `src/datatype.rs`
  - **Why deferred:** Rare.

- [ ] Dataset creation property list: fill value, allocation time
  - **Goal:** Honour `H5D_FILL_VALUE_DEFAULT` etc. on write.
  - **Files:** `src/writer.rs`
  - **Why deferred:** Writer enhancement; not blocking reading.

## Low Priority / Future (speculative - concise)

- [ ] SZIP decompression (Pure Rust AEC/Rice, filter id 4; partial code in `src/filters/szip.rs`)
- [ ] External dataset link support (link a dataset to another file)
- [ ] Parallel I/O for multi-threaded chunk reading
- [ ] HDF5 SWMR (Single Writer Multiple Reader) - skeleton in `src/swmr.rs`
- [ ] HDF5 file repair/recovery tool for corrupted files
- [ ] HDF5 to Zarr conversion for cloud-native migration
- [ ] Custom filter plugin loader
- [ ] HDF5 file diff (compare two files structure and data)
- [ ] NetCDF-4 aware reading mode (interpret CF conventions through HDF5)

## Cross-crate dependencies
- **Blocks:** `oxigeo-drivers/netcdf` NC4 reader/writer (NC4 = HDF5-backed; netcdf TODO depends on this crate's parser).
- **Blocked by:** None.

## Recently completed (kept verbatim from previous TODO.md)
_(Previous TODO.md had no `[x]` entries as of 2026-05-16, aside from the Superblock V2/V3 item completed 2026-05-31. This 2026-07-28 audit additionally found GZIP/shuffle/Fletcher32 chunk filters and a contiguous-data `read_slice` hyperslab API already implemented in source.)_

---
*Last audited: 2026-07-28*
