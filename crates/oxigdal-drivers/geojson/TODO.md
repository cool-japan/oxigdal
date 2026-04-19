# TODO: oxigdal-geojson

## High Priority
- [x] Implement streaming writer for FeatureCollection (write features one-at-a-time) — `GeoJsonWriter::write_features`, `start_feature_collection`, `write_feature_streaming`, `finish_feature_collection`
- [x] Add GeoJSON-seq (newline-delimited GeoJSON / GeoJSONL) support — `read_geojsonl`, `write_geojsonl`, `open`, `open_geojsonl`, `write_geojsonl_to_file`
- [x] Implement spatial filtering during streaming read (bbox predicate pushdown) — `features_in_bbox`, `geometry_bbox`, `feature_bbox_intersects`
- [ ] Add TopoJSON reading support (shared arc topology)
- [ ] Implement foreign member preservation during read-modify-write round-trip (struct fields already present; needs round-trip test suite)
- [x] Add coordinate precision control in writer (configurable decimal places) — `WriterConfig.coordinate_precision` is now fully wired: `apply_precision_to_geometry` rounds all `Position` values before serialization in `write_geometry`, `write_feature`, and `write_feature_collection`
- [x] Implement bounding box calculation and injection during write — `WriterConfig.write_bbox` / `compute_bbox`

## Medium Priority
- [ ] Add property type inference and schema extraction from FeatureCollection
- [ ] Implement GeoJSON to Shapefile/GeoParquet conversion helpers
- [ ] Add right-hand rule enforcement during write (RFC 7946 polygon orientation)
- [ ] Implement antimeridian-crossing geometry splitting
- [ ] Add CRS transformation on read/write (reproject to/from WGS84)
- [ ] Implement GeoJSON diff (compare two FeatureCollections, report changes)
- [ ] Add geometry simplification option during write (reduce file size)
- [ ] Implement FeatureCollection merge from multiple files

## Low Priority / Future
- [ ] Add GeoJSON-T (temporal) extension support
- [ ] Implement GeoJSON validation against RFC 7946 strict mode
- [ ] Add parallel feature parsing for large files
- [ ] Implement coordinate rounding to snap near-equal vertices
- [ ] Add GeoJSON tiling (split large collections into spatial tiles)
- [ ] Implement GeoJSON statistics (feature count, geometry types, bbox) without full parse
- [ ] Add support for nested property objects and arrays
- [ ] Implement GeoJSON to MVT (Mapbox Vector Tile) conversion
