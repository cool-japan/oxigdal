# TODO: oxigdal-hdf5

## High Priority
- [ ] Implement chunked dataset reading (B-tree traversal for chunk lookup)
- [ ] Add GZIP decompression for chunked datasets (Pure Rust via flate2)
- [ ] Implement Superblock Version 2/3 parsing for modern HDF5 files
- [ ] Add compound datatype support (structs with named fields)
- [ ] Implement variable-length string reading
- [ ] Add HDF5 dimension scale convention support
- [ ] Implement partial/hyperslab reading (read sub-region of dataset)

## Medium Priority
- [ ] Add Shuffle filter support (byte-shuffle before compression)
- [ ] Implement Fletcher32 checksum verification
- [ ] Add HDF5 soft/hard link traversal
- [ ] Implement dataset creation property list (fill value, allocation time)
- [ ] Add HDF-EOS metadata parsing for satellite data products
- [ ] Implement virtual dataset (VDS) reading for multi-file aggregation
- [ ] Add 64-bit object addressing for files >2GB
- [ ] Implement object reference and region reference types

## Low Priority / Future
- [ ] Add SZIP decompression (Pure Rust implementation or feature-gated)
- [ ] Implement external dataset link support
- [ ] Add parallel I/O for multi-threaded chunk reading
- [ ] Implement HDF5 SWMR (Single Writer Multiple Reader) support
- [ ] Add HDF5 file repair/recovery tool for corrupted files
- [ ] Implement HDF5 to Zarr conversion for cloud-native migration
- [ ] Add support for custom filter plugins
- [ ] Implement HDF5 file diff (compare two files structure and data)
- [ ] Add NetCDF-4 aware reading mode (interpret CF conventions)
