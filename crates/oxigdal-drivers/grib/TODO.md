# TODO: oxigdal-grib

## High Priority
- [ ] Implement GRIB2 complex packing (Template 5.2, 5.3) for modern data
- [ ] Add GRIB2 JPEG2000 compressed data section decoding (Template 5.40)
- [ ] Implement GRIB2 PNG compressed data section decoding (Template 5.41)
- [ ] Add GRIB writing support (at least GRIB2 simple packing)
- [ ] Implement WMO GRIB2 parameter table 4.2 complete coverage
- [ ] Add GRIB2 Section 2 (Local Use) parsing for ECMWF/NCEP extensions
- [ ] Implement GRIB message scanning for multi-message files (inventory)

## Medium Priority
- [ ] Add GRIB2 template 3.x support for all grid definitions (rotated, stretched)
- [ ] Implement GRIB2 ensemble/probability product templates (4.1, 4.2, 4.11)
- [ ] Add time range processing (accumulation, average, max/min over interval)
- [ ] Implement GRIB index file (.idx) generation and reading for fast access
- [ ] Add GRIB to NetCDF/Zarr conversion tool
- [ ] Implement GRIB2 bitmap section handling for sparse grids
- [ ] Add originating center/subcenter metadata tables (WMO Table C-11)
- [ ] Implement GRIB2 statistical processing templates (4.8, 4.9, 4.10)

## Low Priority / Future
- [ ] Add GRIB2 spectral data template support (Template 5.50, 5.51)
- [ ] Implement GRIB2 CCSDS/AEC compression (Template 5.42)
- [ ] Add GRIB1 to GRIB2 conversion tool
- [ ] Implement GRIB message editing (modify metadata without rewriting data)
- [ ] Add parallel multi-message decoding for large GRIB files
- [ ] Implement GRIB2 derived parameter computation (e.g., wind speed from U/V)
- [ ] Add GRIB inventory caching for repeated access to large files
- [ ] Implement ecCodes-compatible GRIB key access for interoperability
