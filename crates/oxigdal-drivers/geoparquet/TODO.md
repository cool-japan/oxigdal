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
