# TODO: oxigdal-shapefile

## High Priority
- [x] Implement full PolyLine/Polygon/MultiPoint geometry conversion to OxiGDAL core types
- [x] Add `.prj` file reading and writing for CRS (projection) support
- [x] Implement `.cpg` code page file reading for proper character encoding
- [x] Add spatial filtering during read (bounding box query using .shx index)
- [x] Implement streaming record iterator for large shapefiles (avoid loading all into memory) — `ShapefileReader::iter_features()` returns `Result<FeatureIter<'_>>` that yields one `ShapefileFeature` per call with O(1) memory
- [~] Shapefile writer: PolygonZ (15), PolygonM (25), PointZ (11), PointM (21), MultiPatch (31), PolyLineZ (13), PolyLineM (23) (planned 2026-04-18)
  - **Goal:** Writer accepts geometries with Z and/or M dimensions and emits correct shape-type byte and coordinate arrays per ESRI Shapefile spec.
  - **Design:**
    - Shape type codes: 11 PointZ, 13 PolyLineZ, 15 PolygonZ, 21 PointM, 23 PolyLineM, 25 PolygonM, 31 MultiPatch
    - Z records: XY arrays → Zmin/Zmax → Z array → optional Mmin/Mmax → M array
    - M records: XY arrays → Mmin/Mmax → M array
    - Dispatch via has_z()/has_m() accessors on Geometry
  - **Files:** polygon_z.rs, polygon_m.rs, point_z_m.rs, multipatch.rs, polyline_z_m.rs (all new)
  - **Tests:** 6 tests covering shape types, record layout, roundtrip
- [x] Implement attribute filtering during read (SQL-like WHERE clause)

## Medium Priority
- [ ] Add `.dbf` memo field support (`.dbt` files) for long text attributes
- [ ] Implement shapefile reprojection during read/write
- [ ] Add field type auto-detection for writer (infer from Rust types)
- [ ] Implement shapefile merge (combine multiple shapefiles into one)
- [ ] Add Date field type writing with proper formatting
- [ ] Implement record-level random access using .shx offsets
- [ ] Add support for Null shape records (mixed geometry types)
- [ ] Implement shapefile validation (check header consistency, bbox accuracy)

## Low Priority / Future
- [ ] Add async shapefile reading for cloud storage backends
- [ ] Implement shapefile splitting by attribute value or spatial extent
- [ ] Add GeoJSON/GeoParquet conversion helpers
- [ ] Implement dBase IV and dBase 7 extended field types
- [ ] Add shapefile statistics (feature count, bbox, field summary) without full read
- [ ] Implement SHX rebuild from SHP (recover from missing index)
- [ ] Add encoding auto-detection when .cpg is missing
- [ ] Implement shapefile to/from WKB/WKT geometry conversion
