# OxiGDAL Examples

The example programs that used to live in this top-level `examples/` directory
have moved into the `oxigdal-examples` workspace crate, where they are
compiled and checked as part of the normal `cargo build` / `cargo clippy`
pipeline (top-level `examples/**/*.rs` files were never part of any Cargo
target and could silently rot).

**New location:** [`crates/oxigdal-examples/examples/`](../crates/oxigdal-examples/examples/)

## Running an example

```bash
# From the workspace root
cargo run -p oxigdal-examples --example <name>

# e.g.
cargo run -p oxigdal-examples --example cookbook_terrain_analysis
cargo run -p oxigdal-examples --example tutorial_01_quickstart
cargo run -p oxigdal-examples --example cog_pipeline --release
```

List every available example:

```bash
cargo run -p oxigdal-examples --example
```

## What's there

- `tutorial_01_quickstart.rs` .. `tutorial_10_mobile_integration.rs` — a
  progressive tutorial series (raster/vector basics, cloud data, temporal
  analysis, ML inference, web services, performance, GPU, mobile).
- `cookbook_*.rs` — end-to-end recipes (batch processing, change detection,
  cloud ETL, custom algorithms, data fusion, ML classification, quality
  assessment, satellite processing, terrain analysis, web tile server).
- `cog_pipeline.rs`, `ml_inference.rs`, `satellite_processing.rs`,
  `timeseries_analysis.rs`, `vector_postgis.rs` — deeper, single-purpose
  pipelines built on the real `oxigdal-sensors`, `oxigdal-ml`,
  `oxigdal-temporal`, and `oxigdal-postgis` crate APIs.
- `read_geotiff.rs`, `read_geojson.rs`, `vector_operations.rs`,
  `tile_processing.rs`, `image_resampling.rs`,
  `intermediate_ndvi_calculation.rs` — smaller, focused examples.

There are also 8 `cargo run --bin <name>` example binaries defined directly
in the crate (`read-geotiff`, `write-geotiff`, `create-cog`, `cog-tiles`,
`buffer-ops`, `list-tiff-structure`, `geotiff-with-overviews`,
`create-geoparquet-samples`) — see
[`crates/oxigdal-examples/README.md`](../crates/oxigdal-examples/README.md).

## Notes

- `vector_postgis` and the cloud-facing sections of `tutorial_04_cloud_data`
  demonstrate real client APIs but need a reachable PostGIS/cloud endpoint to
  actually round-trip data; they degrade gracefully (log a warning and
  continue) when no such endpoint is configured.
- `tutorial_09_gpu_acceleration` attempts real GPU detection via
  `oxigdal-gpu`'s `GpuContext`, falling back to the crate's own CPU kernels
  (`oxigdal_gpu::cpu_fallback::cpu`) when no compatible GPU is available.
