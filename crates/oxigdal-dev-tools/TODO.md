# TODO: oxigdal-dev-tools

## High Priority
- [ ] Implement file format inspector for GeoTIFF (read IFD tags, overview structure)
- [ ] Add test data generator for all supported formats (GeoTIFF, GeoJSON, Shapefile)
- [ ] Implement profiler with flamegraph-compatible output
- [ ] Add validator for COG (Cloud-Optimized GeoTIFF) compliance
- [ ] Implement benchmarker with statistical analysis (mean, stddev, percentiles)
- [ ] Add debugger with raster band visualization (ASCII/terminal rendering)

## Medium Priority
- [ ] Implement format comparison tool (diff two GeoTIFFs pixel-by-pixel)
- [ ] Add synthetic DEM generator (fractal terrain, Perlin noise)
- [ ] Implement CRS diagnostic tool (validate EPSG codes, show projections)
- [ ] Add file size estimator for format conversion planning
- [ ] Implement test fixture management (download/cache standard test datasets)
- [ ] Add memory profiling helpers (track peak allocation per operation)
- [ ] Implement regression test harness (compare outputs against golden files)
- [ ] Add random vector feature generator (points, lines, polygons with attributes)
- [ ] Implement STAC catalog generator for test collections

## Low Priority / Future
- [ ] Add interactive data explorer (TUI with ratatui)
- [ ] Implement documentation generator (extract examples from code, run them)
- [ ] Add performance regression detection (compare benchmarks across commits)
- [ ] Implement fuzzing harness generator for format parsers
- [ ] Add dependency graph visualizer for the workspace
- [ ] Implement CI helper (generate test matrix, coverage reports)
