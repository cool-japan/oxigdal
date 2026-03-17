# TODO: oxigdal-geojson

## High Priority
- [ ] Implement true streaming parser that reads features incrementally from a `Read` source without buffering the entire JSON into memory (current StreamingFeatureReader wraps an already-parsed Vec)
- [ ] Add MultiPointZ, MultiLineStringZ, MultiPolygonZ geometry variants for full 3D multi-geometry support
- [ ] Implement GeoJSON-seq / newline-delimited GeoJSON (RFC 8142) reader and writer for line-delimited streaming
- [ ] Add geometry simplification (Douglas-Peucker) as a writer transform to reduce output size
- [ ] Implement TopoJSON output support (shared arcs, topology-aware encoding) for bandwidth savings
- [ ] Add OR-logic and NOT-logic to FeatureFilter (currently only AND semantics for property filters)

## Medium Priority
- [ ] Implement geometry reprojection integration with oxigdal-proj (transform coordinates on read/write)
- [ ] Add WKT (Well-Known Text) geometry serialization and deserialization as alternative to GeoJSON
- [ ] Implement spatial indexing of feature collections (integration with oxigdal-index RTree)
- [ ] Add geometry area, length, and centroid computation on GeoJsonGeometry
- [ ] Implement geometry clipping to a bounding box (Cohen-Sutherland for lines, Sutherland-Hodgman for polygons)
- [ ] Add RegExp filter operator for string property matching
- [ ] Implement feature merging / dissolve by property key (union geometries sharing the same attribute value)
- [ ] Add coordinate precision truncation on parse (reduce memory for large files with excessive decimal places)
- [ ] Implement 6-dimensional bbox support `[minx, miny, minz, maxx, maxy, maxz]` for 3D collections

## Low Priority / Future
- [ ] Add GeoJSON-LD (Linked Data) context output for semantic web compatibility
- [ ] Implement property schema inference (detect field types across features for downstream use)
- [ ] Add geometry buffering (polygon offset) for GeoJSON geometries
- [ ] Implement feature sorting by property value or spatial key (geohash, Hilbert)
- [ ] Add parallel feature parsing for large collections using rayon (feature-gated)
- [ ] Implement GeoJSON diff: compare two FeatureCollections and emit added/removed/changed features
- [ ] Add CRS transformation on write (reproject from source CRS to WGS84 per RFC 7946)
- [ ] Implement geometry validity checking beyond current validator (e.g., polygon self-intersection, ring orientation)
- [ ] Add integration tests with real-world GeoJSON datasets (Natural Earth, OSM exports)
