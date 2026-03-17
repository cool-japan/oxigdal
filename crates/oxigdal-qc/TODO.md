# TODO: oxigdal-qc

## High Priority
- [ ] Implement topology validation for vector data (self-intersections, gaps, overlaps)
- [ ] Add cloud-optimized GeoTIFF (COG) compliance checker
- [ ] Implement raster NoData consistency validation across bands
- [ ] Add STAC item/collection schema validation
- [ ] Implement automatic CRS validation (EPSG code vs WKT consistency)
- [ ] Add batch QC mode for processing entire directories

## Medium Priority
- [ ] Implement raster accuracy assessment (confusion matrix, kappa coefficient)
- [ ] Add vector attribution completeness checker with schema enforcement
- [ ] Implement spatial extent validation (reasonable bounds per CRS)
- [ ] Add GeoPackage compliance validation (OGC GeoPackage spec)
- [ ] Implement raster radiometric range validation per sensor type
- [ ] Add duplicate feature detection in vector datasets
- [ ] Implement HTML report generation with embedded charts
- [ ] Add TOML-based rule configuration file loading
- [ ] Implement fix preview mode (show proposed changes before applying)

## Low Priority / Future
- [ ] Add cross-dataset consistency validation (overlapping tiles, seamlines)
- [ ] Implement temporal consistency checking for time series datasets
- [ ] Add point cloud (LAS/COPC) quality validation
- [ ] Implement metadata completeness scoring per standard (ISO, FGDC, INSPIRE)
- [ ] Add CI/CD integration (GitHub Actions, GitLab CI output formats)
- [ ] Implement custom rule scripting via embedded expression language
