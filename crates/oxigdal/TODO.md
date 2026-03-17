# TODO: oxigdal (umbrella crate)

## High Priority
- [ ] Implement actual raster band reading in Dataset (currently returns stub metadata)
- [ ] Wire Dataset::open() to real driver crates (parse GeoTIFF headers, read GeoJSON)
- [ ] Add magic-byte format detection (not just file extension)
- [ ] Implement Dataset::create() for writing new datasets
- [ ] Add raster band iterator (read band data as typed arrays)
- [ ] Implement vector layer iterator (read features with geometry + attributes)

## Medium Priority
- [ ] Add Dataset::reproject() convenience method via oxigdal-proj
- [ ] Implement Dataset::clip() for subsetting by bounding box
- [ ] Add Dataset::convert() for format translation (GeoTIFF to GeoJSON, etc.)
- [ ] Implement cloud URI support in Dataset::open() (s3://, gs://, az://)
- [ ] Add async variants of open/read/write operations
- [ ] Implement Dataset::info() with actual metadata parsing (not just stubs)
- [ ] Add virtual raster (VRT) creation from multiple datasets
- [ ] Implement feature-flag documentation with docsrs cfg annotations
- [ ] Add Dataset::statistics() for quick raster min/max/mean/stddev

## Low Priority / Future
- [ ] Add GDAL compatibility shim (GDALOpen, GDALClose function aliases)
- [ ] Implement Python bindings via PyO3 (oxigdal-python subcrate)
- [ ] Add WASM bindings for browser use (oxigdal-wasm already exists, integrate)
- [ ] Implement streaming read for datasets larger than memory
- [ ] Add dataset comparison (semantic diff between two datasets)
- [ ] Implement plugin system for user-defined format drivers
- [ ] Add comprehensive migration guide from GDAL C/Python to OxiGDAL
