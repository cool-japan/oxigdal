# TODO: oxigdal-zarr

## High Priority
- [ ] Implement Zarr v3 sharding codec for efficient chunk-of-chunks storage
- [ ] Add cloud storage integration (S3Store, GCSStore) with async I/O
- [ ] Implement chunk-level parallel read/write using rayon
- [ ] Add Blosc codec support (shuffle + compression, widely used in Zarr v2)
- [ ] Implement consolidated metadata reading and writing for faster opens
- [ ] Add dimension coordinate variable support for labeled axes
- [ ] Implement slice reading with stride/step support (e.g., every Nth element)

## Medium Priority
- [ ] Add Zarr v3 codec pipeline (bytes-to-bytes, array-to-bytes, array-to-array)
- [ ] Implement LZ4 frame format codec (distinct from LZ4 block)
- [ ] Add chunk cache with configurable size and eviction policy
- [ ] Implement group hierarchy traversal and metadata aggregation
- [ ] Add Zarr v3 data type extensions (datetime, string, structured)
- [ ] Implement fill value handling for sparse arrays
- [ ] Add HTTP range-request store for remote Zarr datasets
- [ ] Implement Zarr to NetCDF/HDF5 conversion tool

## Low Priority / Future
- [ ] Add Zarr v3 extensions: variable chunking, codecs registry
- [ ] Implement fsspec-compatible storage abstraction
- [ ] Add Zarr directory consolidation (combine many small files)
- [ ] Implement Zarr virtual store (reference multiple remote chunks)
- [ ] Add Kerchunk-compatible reference file generation
- [ ] Implement Zarr-based append operations for time-series data
- [ ] Add XARRAY-compatible attributes and encoding conventions
- [ ] Implement Zarr checksum verification for data integrity
- [ ] Add Zarr diff tool (compare two arrays chunk-by-chunk)
