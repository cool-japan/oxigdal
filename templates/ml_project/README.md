# ML Project Template for OxiGeo

A project template for building geospatial machine learning pipelines powered by OxiGeo.

## What This Template Provides

- OxiGeo core, algorithms, ML, and GeoTIFF driver dependencies
- Pure-Rust ONNX Runtime integration via [oxionnx](https://docs.rs/oxionnx) for model inference (no C++ ONNX Runtime / `ort` dependency)
- Scientific computing with [SciRS2-Core](https://docs.rs/scirs2-core)
- Async runtime (Tokio) for parallel data loading and processing
- Structured error handling with `anyhow` and `thiserror`
- Scaffolded ML pipeline: data loading, training, inference, and export

## Getting Started

1. Copy this template directory to your workspace
2. Update `Cargo.toml` with your project name, authors, and any additional dependencies
3. Implement your ML pipeline stages in `src/main.rs`
4. Run:

```sh
cargo run --release
```

## Example Pipeline

```rust
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load geospatial training data
    // let dataset = oxigeo_geotiff::read("satellite_imagery.tif")?;

    // 2. Preprocess and extract features
    // let features = extract_features(&dataset)?;

    // 3. Run inference with ONNX model (pure-Rust oxionnx runtime)
    // let session = oxionnx::SessionBuilder::new().commit_from_file("model.onnx")?;
    // let predictions = session.run(inputs)?;

    // 4. Export results as GeoTIFF
    // oxigeo_geotiff::write("predictions.tif", &output)?;

    Ok(())
}
```

## Extending the Template

- Add classification, regression, or segmentation models
- Integrate additional OxiGeo drivers for multi-format input
- Use `ndarray` for tensor operations alongside SciRS2
- Add data augmentation and validation stages
- Export predictions to GeoParquet, GeoJSON, or other formats

## License

Apache-2.0

Part of the [OxiGeo](https://github.com/cool-japan/oxigeo) project by COOLJAPAN OU.
