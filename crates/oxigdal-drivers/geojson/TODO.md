# TODO: oxigdal-geojson

## High Priority
- [ ] Implement streaming writer for FeatureCollection (write features one-at-a-time)
- [ ] Add GeoJSON-seq (newline-delimited GeoJSON / GeoJSONL) support
- [ ] Implement spatial filtering during streaming read (bbox predicate pushdown)
- [ ] Add TopoJSON reading support (shared arc topology)
- [ ] Implement foreign member preservation during read-modify-write round-trip
- [ ] Add coordinate precision control in writer (configurable decimal places)
- [ ] Implement bounding box calculation and injection during write

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
