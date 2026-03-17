# TODO: oxigdal-gpkg

## High Priority
- [ ] Implement GeoPackage writer: create new .gpkg files with proper SQLite page layout, system tables, and feature insertion
- [ ] Add 3D geometry support (PointZ, LineStringZ, PolygonZ) in WKB parser and GpkgGeometry enum
- [ ] Implement tile pyramid reader: extract raster tiles from GeoPackage tile tables using TileMatrix metadata
- [ ] Add full SQLite B-tree page traversal for reading feature tables (currently only header/schema parsing)
- [ ] Implement GeoPackage extensions registry parsing (gpkg_extensions table)
- [ ] Add WAL (Write-Ahead Logging) mode support for concurrent read access

## Medium Priority
- [ ] Implement spatial index (RTree) reading from gpkg_rtree_* shadow tables for accelerated bbox queries
- [ ] Add GeoPackage Related Tables Extension (GPKG-RTE) support for many-to-many feature relationships
- [ ] Implement tile matrix set creation and insertion for raster tile writing
- [ ] Add WKB encoding for M coordinates (PointM, PointZM) and circular/curve geometry types
- [ ] Implement attribute filtering with SQL-like WHERE clause parsing on feature tables
- [ ] Add GeoPackage schema constraints validation (NOT NULL, UNIQUE, CHECK on user tables)
- [ ] Implement feature table pagination (read features in configurable page sizes for large tables)
- [ ] Add GeoPackage metadata table support (gpkg_metadata, gpkg_metadata_reference)
- [ ] Implement conversion to/from oxigdal-geojson FeatureCollection

## Low Priority / Future
- [ ] Add GeoPackage Tiled Gridded Coverage Data extension for elevation/DEM tiles
- [ ] Implement GeoPackage data-columns constraints extension for column descriptions and constraints
- [ ] Add support for multiple geometry columns per feature table
- [ ] Implement FlatGeoBuf export from GeoPackage feature tables
- [ ] Add CRS reprojection on read using oxigdal-proj (transform coordinates to target SRS)
- [ ] Implement incremental feature table updates (INSERT/UPDATE/DELETE without rewriting)
- [ ] Add GeoPackage file integrity check (validate all system tables, foreign keys, SRS references)
- [ ] Implement GeoPackage-to-MBTiles conversion (tile table extraction)
- [ ] Add trigger-based feature change tracking (GeoPackage change tracking extension)
- [ ] Implement concurrent read access with file locking for multi-threaded scenarios
