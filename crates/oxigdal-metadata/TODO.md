# TODO: oxigdal-metadata

> **Purpose:** Geospatial metadata standards — ISO 19115/19115-3, FGDC, INSPIRE, DataCite, DCAT — with cross-standard transforms, validation, and extractor stubs for source datasets.
> **Status (2026-05-16):** 6,809 LoC (src) · 119 tests (35 inline + 84 in tests/) · 2 real-code stubs (extractors)
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Implement real NetCDF/CF-Convention metadata extraction (currently inserts only `file_path` attribute).
  - **Verified gap:** `src/extract.rs:486-509` —
    ```rust
    fn extract_from_netcdf<P: AsRef<Path>>(path: P) -> Result<ExtractedMetadata> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let mut attributes = std::collections::HashMap::new();
        attributes.insert("file_path".to_string(), path_str);

        let metadata = ExtractedMetadata {
            format: Some("NetCDF".to_string()),
            attributes,
            ..Default::default()
        };
        // ...
        // Placeholder for NetCDF-specific extraction
        // This would include:
        // - Reading global attributes (title, summary, keywords)
        // - Extracting CF convention metadata
        // ...
        Ok(metadata)
    }
    ```
  - **Goal:** Open the NetCDF file (via sibling `oxigdal-netcdf` crate), extract CF-1.10 (Climate & Forecast Metadata Conventions) global attributes (`title`, `summary`, `keywords`, `institution`, `Conventions`, `history`, `source`, `references`) into `ExtractedMetadata.attributes`; extract bbox from coordinate variables; extract temporal extent from `time` variable units; extract CRS from `grid_mapping` variable.
  - **Design:** Add `oxigdal-netcdf` optional dep with `netcdf` feature flag (Pure Rust policy — confirm sibling crate is Pure Rust or feature-gate). Walk `Dataset::attributes()` for CF globals; map to `ExtractedMetadata` field-by-field. For bbox: find variables with `standard_name = "longitude"` / `"latitude"` (or `axis` attr `X`/`Y`); take min/max. Temporal: parse `time:units = "days since YYYY-MM-DD"` per CF §4.4 with `chrono`. CRS: `grid_mapping` variable's `grid_mapping_name` attribute (e.g., `latitude_longitude`, `transverse_mercator`).
  - **Files:** `src/extract.rs:486-509` (replace stub), `Cargo.toml` (add feature + dep).
  - **Tests:** (proposed) `test_netcdf_extract_cf_globals`, `test_netcdf_extract_bbox_from_lon_lat`, `test_netcdf_extract_temporal_extent_days_since`, `test_netcdf_grid_mapping_to_crs`, `test_netcdf_extract_no_cf_falls_back_to_path_only`.
  - **Risk:** `oxigdal-netcdf` may itself be feature-gated for C/HDF5; document feature-flag chain.
  - **Prerequisites:** `oxigdal-netcdf` Pure-Rust or HDF5-gated reader available.
  - **Done:** 2026-05-22 (Slice 26). Added `[features] netcdf = ["dep:oxigdal-netcdf"]` + `oxigdal-netcdf = { workspace = true, optional = true }` to `Cargo.toml`. New `src/extractors/{mod.rs, netcdf_cf.rs}` (575 LoC): `HasAttributes`/`HasVariables` trait shims keep tests independent of a real NetCDF file (in-test `FakeDataset` implements both); `CfGlobals` struct mapping `title/summary/keywords/institution/Conventions/history/source/references/comment`; `extract_cf_globals/extract_bbox_from_lon_lat/extract_temporal_extent/extract_grid_mapping_crs/parse_cf_time_units`. Bbox lookup tries `standard_name=longitude/latitude` first, falls back to `axis=X/Y`. Temporal: parses CF `"<unit> since <ref-date>"` (days/hours/minutes/seconds variants), arithmetic via `chrono::Duration`. Grid mappings recognised: `latitude_longitude`→EPSG:4326, `transverse_mercator`→assembled WKT, `mercator`→EPSG:3395, `polar_stereographic`→EPSG:3413, plus `lambert_conformal_conic`/`albers_conical_equal_area`/`rotated_latitude_longitude`/`stereographic`. `NetCdfCfExtractor::extract` (feature-gated) wraps `oxigdal_netcdf::Dataset` via private `real_dataset::NetCdfReaderShim`. `src/extract.rs::extract_from_netcdf` body swapped with `#[cfg(feature = "netcdf")]` dispatch and a byte-equivalent fallback for the no-feature path. `src/lib.rs` +1 line `pub mod extractors;`. `oxigdal-netcdf` confirmed present at `crates/oxigdal-drivers/netcdf`.
  - **Tests:** 10 in `crates/oxigdal-metadata/tests/netcdf_cf_test.rs` + 3 inline unit tests (all gated `#[cfg(feature = "netcdf")]`): CF globals canonical + missing optional; bbox via standard_name vs axis fallback; temporal extent for `days/hours/seconds since`; grid_mapping for latitude_longitude + transverse_mercator + absent. Full suite 120/120 (107 existing + 10 integration + 3 unit). No-feature path preserves the original file-path-only `ExtractedMetadata`.

- [ ] Implement real HDF5 metadata extraction (currently inserts only `file_path`).
  - **Verified gap:** `src/extract.rs:511-533` — identical pattern to NetCDF (path-only insert + "Placeholder for HDF5-specific extraction" comment).
  - **Goal:** Extract HDF5 root group attributes and per-dataset attributes; surface embedded ISO 19115 XML if present (NASA EOS files); record group hierarchy as `attributes["groups"] = "/MOD/Aqua/Day,..."`.
  - **Design:** Add `oxigdal-hdf5` dep (sibling crate; feature-gated for C HDF5 lib per Pure Rust policy). Walk groups recursively with bounded depth. If a root attribute named `iso_metadata` or path `/Metadata/iso19115` exists and contains XML, route it through the `iso19115::parser::from_xml` path. Pull NASA EOS standard attrs (`ProductionDateTime`, `ShortName`, `VersionID`, `RangeBeginningDate`, `RangeEndingDate`, `WestBoundingCoordinate`, ...).
  - **Files:** `src/extract.rs:511-533`.
  - **Tests:** (proposed) `test_hdf5_root_attributes_extracted`, `test_hdf5_eos_metadata_to_temporal_extent`, `test_hdf5_embedded_iso19115_xml_parsed`, `test_hdf5_group_hierarchy_summarised`.
  - **Risk:** HDF5 bindings require C; gate accordingly.
  - **Prerequisites:** `oxigdal-hdf5` reader available.

- [x] Replace DataCite/INSPIRE transform placeholders that emit literal `"PLACEHOLDER"` / silently no-op.
  - Done: 2026-05-31 (Slice 29). Tests: 14 new (transform_doi_locator_test) + 108 existing = 122 total.
  - `iso19115_to_datacite`: private `extract_doi` matches bare `10.x/y` and `doi.org/` URLs from `Citation.identifier: Vec<String>`; publisher from `organization_name`/`individual_name`; `"Unknown Publisher"` only as last resort. `iso19115_to_inspire`: walks `distribution_info → transfer_options → online` → `Vec<ResourceLocator>`; `OnlineFunction` mapped to `ResourceLocatorFunction`.

## Medium Priority
- [ ] FGDC ↔ ISO 19115 bidirectional transformation.
  - **Goal:** Round-trip FGDC CSDGM (FGDC-STD-001-1998) ↔ ISO 19115; cover the common 80%.
  - **Files:** `src/transform.rs` (extend), `src/fgdc/mod.rs:677 LoC` (FGDC types already present).
  - **Why deferred:** FGDC is US-government legacy; many fields have no ISO equivalent (e.g., FGDC `purpose` vs ISO `abstract`); requires curated mapping table.

- [ ] Metadata merge for multi-source datasets (mosaics, ensembles).
  - **Goal:** Combine N `ExtractedMetadata` records into one; bbox = union, temporal = union, keywords = deduplicated set, attribution = preserve provenance.
  - **Files:** New `src/merge.rs`.

- [ ] Dublin Core Metadata Element Set 1.1 (ISO 15836).
  - **Files:** New `src/dublin_core.rs`.

- [ ] INSPIRE TG (Technical Guidelines) validation against EU INSPIRE Metadata Regulation (EC 1205/2008).
  - **Files:** `src/validate.rs:631 LoC` (validator framework exists).

- [ ] schema.org/Dataset JSON-LD serialisation (for SEO and Google Dataset Search).
  - **Files:** New `src/schema_org.rs`.

- [ ] DataCite DOI registration API client (REST).
  - **Files:** New `src/datacite/api.rs` (current `src/datacite/mod.rs:815 LoC` is types only).

- [ ] DCAT-AP (DCAT Application Profile for EU data portals 2.1.1) serialisation.
  - **Files:** `src/dcat/mod.rs:525 LoC` (DCAT 3 types exist; add AP-specific shacl-compatible profile).

- [ ] Metadata diff/comparison (field-by-field with reasons).

- [ ] HDF5 dataset-level attribute extraction (separate from item 2 which covers file globals).

- [ ] Quality scoring with weighted field-importance config.

- [ ] CKAN metadata format support for open-data portals.

## Low Priority / Future (one-liners)
- [ ] OGC CSW 2.0.2 client integration (consume external catalogues).
- [ ] Metadata lineage graph (DOT / Mermaid output) per W3C PROV-O.
- [ ] Multilingual metadata (ISO 19115 locale handling, language code map).
- [ ] Template system for batch metadata creation.
- [ ] PDF metadata-report generator (typst or weasyprint).
- [ ] W3C PROV-O provenance model full integration.
- [ ] OGC API Records compatibility (new modern API).

## Cross-crate dependencies
- **Blocks:** `oxigdal-stac` (consumes for catalog enrichment), `oxigdal-services` (CSW responses).
- **Blocked by:** `oxigdal-netcdf` (NetCDF extractor), `oxigdal-hdf5` (HDF5 extractor).

## Recently completed (verbatim)
- [x] Implement ISO 19115-3 XML serialization (currently builder only)
- [x] Add STAC metadata extraction from actual STAC JSON catalogs (feature-gated stac feature, extracts bbox/temporal/CRS/title/description/keywords/resolution, 6 tests)
- [x] Add GeoTIFF metadata extraction (read TIFF tags, GeoKeys)

---
*Last audited: 2026-05-16*
