# TODO: oxigdal-shapefile

## High Priority
- [ ] Implement full PolyLine/Polygon/MultiPoint geometry conversion to OxiGDAL core types
- [ ] Add `.prj` file reading and writing for CRS (projection) support
- [ ] Implement `.cpg` code page file reading for proper character encoding
- [ ] Add spatial filtering during read (bounding box query using .shx index)
- [ ] Implement streaming record iterator for large shapefiles (avoid loading all into memory)
- [ ] Add MultiPatch (3D surface) geometry support
- [ ] Implement attribute filtering during read (SQL-like WHERE clause)

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
