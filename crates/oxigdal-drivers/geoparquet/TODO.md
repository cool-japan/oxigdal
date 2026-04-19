# TODO: oxigdal-geoparquet

## High Priority
- [ ] Implement row group-level spatial filtering using bounding box metadata
- [ ] Add predicate pushdown for attribute filters (push to Parquet reader)
- [ ] Implement GeoParquet 1.1 covering column support (bbox columns)
- [ ] Add native geometry encoding support (Point/LineString/Polygon arrays, not just WKB)
- [ ] Implement column projection (read only selected columns for performance)
- [ ] Add parallel row group reading using rayon
- [ ] Implement GeoParquet metadata validation against specification

## Medium Priority
- [ ] Add spatial partitioning writer (Hilbert curve, geohash-based row group layout)
- [ ] Implement GeoParquet to GeoJSON/Shapefile streaming conversion
- [ ] Add support for multiple geometry columns per file
- [ ] Implement schema evolution (add/remove columns without full rewrite)
- [ ] Add Parquet statistics exposure (min/max per column per row group)
- [ ] Implement Delta Lake / Iceberg integration for versioned geospatial tables
- [ ] Add CRS transformation during read/write
- [ ] Implement row group compaction and optimization tool

## Low Priority / Future
- [ ] Add GeoArrow native integration (zero-copy geometry arrays)
- [ ] Implement GeoParquet partitioned dataset reading (directory of .parquet files)
- [ ] Add cloud-native reading via object store (S3, GCS, Azure Blob)
- [ ] Implement GeoParquet to/from DuckDB spatial extension bridge
- [ ] Add geometry column statistics (centroid, bbox, hull) in footer metadata
- [ ] Implement streaming Parquet writer for unbounded feature streams
- [ ] Add nested struct and list column support for complex properties
- [ ] Implement GeoParquet file merge with spatial re-partitioning

## WKB Reader Extensions
- [x] GeoParquet WKB reader: nested GeometryCollection + Z/M/ZM variants (planned 2026-04-18)
  - **Goal:** `WkbReader::read_geometry` decodes all 3000-series WKB type codes: Z variants (1001–1007), M variants (2001–2007), ZM variants (3001–3007), plus recursive GeometryCollection (type 7).
  - **Design:** Extend dispatch match; add Z/M/ZM decoder helpers; recursive GeometryCollection with depth guard (max 64).
  - **Files:** geometry/wkb.rs (depth guard + has_z/has_m), geometry/types.rs (Geometry::has_z/has_m methods), geometry/wkb_extended.rs (wkb_bbox stride fix for M/ZM)
  - **Tests:** 6 tests covering PointZ, PointZM, flat collection, recursive collection, depth guard, MultiPolygonZM
