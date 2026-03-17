# TODO: oxigdal-cli

## High Priority
- [ ] Implement actual raster I/O in `translate` command (currently stub)
- [ ] Implement actual reprojection in `warp` command via oxigdal-proj
- [ ] Wire `convert` command to real format drivers (GeoTIFF, GeoJSON, Shapefile, etc.)
- [ ] Add `ogr2ogr`-equivalent vector conversion with attribute filtering
- [ ] Implement `calc` band math expressions with proper parser
- [ ] Add progress bar integration for long-running operations (indicatif 0.18)
- [ ] Implement `merge` command with proper overlap handling and nodata merging
- [ ] Add cloud URI support (s3://, gs://, az://) via oxigdal-rs3gw

## Medium Priority
- [ ] Add `tileindex` command for generating tile index shapefiles
- [ ] Add `polygonize` command (raster to vector conversion)
- [ ] Implement `buildvrt` with proper XML VRT generation and relative paths
- [ ] Add `clip` subcommand for clipping rasters/vectors by geometry or bbox
- [ ] Add `reproject` shorthand subcommand for CRS transformations
- [ ] Implement `--co` (creation options) flag for all output commands
- [ ] Add `stats` command for raster/vector statistics summary
- [ ] Add YAML/TOML config file support for batch processing
- [ ] Implement `diff` command for comparing two datasets

## Low Priority / Future
- [ ] Add interactive mode (REPL) for exploratory data analysis
- [ ] Implement pipeline mode (stdin/stdout chaining between commands)
- [ ] Add man page generation via clap_mangen
- [ ] Add Nushell and Fish completion generators
- [ ] Implement `benchmark` command for format read/write performance comparison
- [ ] Add `serve` subcommand to launch a local tile server (OGC Tiles)
- [ ] Add `--parallel` flag with configurable thread count for all commands
