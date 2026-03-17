# TODO: oxigdal-netcdf

## High Priority
- [ ] Update reader/writer to netcdf3 v0.6.0 API (FileReader/FileWriter/Dataset)
- [ ] Implement actual data reading via Pure Rust netcdf3 crate integration
- [ ] Add CF-1.8 coordinate variable auto-detection and axis interpretation
- [ ] Implement unlimited dimension record appending
- [ ] Add NetCDF-3 64-bit offset format support for large variables
- [ ] Implement variable slicing (read sub-regions with start/count/stride)
- [ ] Add missing value and _FillValue attribute handling

## Medium Priority
- [ ] Implement CF conventions grid_mapping parsing for CRS extraction
- [ ] Add time coordinate decoding (calendar-aware, CF time units)
- [ ] Implement multi-variable reading with shared dimension coordinates
- [ ] Add NetCDF-3 write support for creating new files from scratch
- [ ] Implement variable packing/unpacking (scale_factor, add_offset)
- [ ] Add coordinate bounds variable support (cell boundaries)
- [ ] Implement OPeNDAP-style constraint expressions for remote subsetting
- [ ] Add NetCDF to Zarr streaming conversion

## Low Priority / Future
- [ ] Implement Pure Rust NetCDF-4 reader (subset of HDF5 for NC4 files)
- [ ] Add CDL (Common Data Language) text format import/export
- [ ] Implement UGRID convention support for unstructured grids
- [ ] Add SGRID convention support for structured grids
- [ ] Implement NetCDF-4 group hierarchy traversal
- [ ] Add parallel variable reading for multi-core performance
- [ ] Implement NetCDF file repair for truncated/corrupted files
- [ ] Add NcML aggregation support for multi-file datasets
- [ ] Implement ACDD (Attribute Convention for Dataset Discovery) validation
