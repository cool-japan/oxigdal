# ML Project Template for OxiGDAL

A project template for building geospatial machine learning pipelines powered by OxiGDAL.

## What This Template Provides

- OxiGDAL core, algorithms, ML, and GeoTIFF driver dependencies
- ONNX Runtime integration via [ort](https://docs.rs/ort) for model inference
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
    // let dataset = oxigdal_geotiff::read("satellite_imagery.tif")?;

    // 2. Preprocess and extract features
    // let features = extract_features(&dataset)?;

    // 3. Run inference with ONNX model
    // let session = ort::Session::builder()?.commit_from_file("model.onnx")?;
    // let predictions = session.run(inputs)?;

    // 4. Export results as GeoTIFF
    // oxigdal_geotiff::write("predictions.tif", &output)?;

    Ok(())
}
```

## Extending the Template

- Add classification, regression, or segmentation models
- Integrate additional OxiGDAL drivers for multi-format input
- Use `ndarray` for tensor operations alongside SciRS2
- Add data augmentation and validation stages
- Export predictions to GeoParquet, GeoJSON, or other formats

## License

Apache-2.0

Part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) project by COOLJAPAN OU.
