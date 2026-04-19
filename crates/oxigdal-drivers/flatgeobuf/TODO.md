# TODO: oxigdal-flatgeobuf

## High Priority
- [x] FlatGeobuf writer with Packed Hilbert R-tree index (planned 2026-04-18)
  - **Goal:** Writer produces valid `.fgb` files: 8-byte magic, header, optional Packed Hilbert R-tree index, feature FlatBuffers in bbox-sorted order.
  - **Design:** Two-pass: collect bboxes → sort on Hilbert curve over global bbox → write header+index+features.
  - **Files:** writer.rs (enhanced), index.rs (Hilbert sort fix)
  - **Tests:** 6 tests covering magic, empty file, Hilbert sorted features, R-tree node size, roundtrip with index seek, Z flag
- [ ] Implement R-tree spatial index querying for bbox-filtered reads
- [ ] Add async HTTP range-request reading for cloud-hosted FlatGeobuf files
- [ ] Implement feature-level random access using spatial index offsets
- [ ] Add CRS reprojection during read/write
- [ ] Implement streaming write for large feature collections
- [ ] Add support for all FlatGeobuf geometry types (CircularString, CompoundCurve, etc.)

## Medium Priority
- [ ] Implement FlatGeobuf to GeoJSON/Shapefile conversion helpers
- [ ] Add attribute filtering during read (push down column selection)
- [ ] Implement partial column reading (skip unused attribute columns)
- [ ] Add FlatGeobuf file validation and integrity checking
- [ ] Implement geometry simplification on-the-fly during read
- [ ] Add support for FlatGeobuf 3.x features (Z/M coordinates, nested properties)
- [ ] Implement feature count and bbox extraction without full scan

## Low Priority / Future
- [ ] Add FlatGeobuf index rebuilding tool
- [ ] Implement FlatGeobuf merge (combine multiple files preserving spatial index)
- [ ] Add parallel feature encoding for writer
- [ ] Implement FlatGeobuf diff (detect changes between two files)
- [ ] Add memory-mapped reading for local file performance
- [ ] Implement FlatGeobuf to PMTiles/MBTiles conversion pipeline
- [ ] Add configurable geometry encoding precision
- [ ] Implement FlatGeobuf statistics without loading features
