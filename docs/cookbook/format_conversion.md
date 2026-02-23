# Format Conversion Recipes

Common recipes for converting between geospatial data formats.

## Raster Formats

### GeoTIFF to COG

```rust
use oxigdal_core::Dataset;
use oxigdal_geotiff::{GeoTiffDriver, CogOptions};

async fn geotiff_to_cog(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let driver = GeoTiffDriver::new();
    let options = CogOptions {
        tile_size: 512,
        compression: "DEFLATE".to_string(),
        overview_levels: vec![2, 4, 8, 16],
        ..Default::default()
    };

    driver.create_cog(&dataset, output, options).await?;
    Ok(())
}
```

### HDF5 to GeoTIFF

```rust
async fn hdf5_to_geotiff(input: &str, subdataset: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let hdf_path = format!("HDF5:{}://{}", input, subdataset);
    let dataset = Dataset::open(&hdf_path).await?;

    dataset.save(output).await?;
    Ok(())
}
```

### NetCDF to Zarr

```rust
use oxigdal_zarr::ZarrDriver;

async fn netcdf_to_zarr(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let driver = ZarrDriver::new();
    driver.convert(&dataset, output).await?;

    Ok(())
}
```

## Vector Formats

### Shapefile to GeoJSON

```rust
use oxigdal_geojson::GeoJsonDriver;

async fn shapefile_to_geojson(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open(input).await?;
    let src_layer = src.layer(0)?;

    let driver = GeoJsonDriver::new();
    let mut dst = driver.create(output).await?;
    let mut dst_layer = dst.create_layer("features", src_layer.spatial_ref()?.into())?;

    // Copy schema
    let layer_def = src_layer.definition()?;
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        dst_layer.create_field(field.name(), field.field_type(), field.width())?;
    }

    // Copy features
    for feature in src_layer.features()? {
        dst_layer.add_feature(feature)?;
    }

    dst.flush().await?;
    Ok(())
}
```

### GeoJSON to GeoParquet

```rust
use oxigdal_geoparquet::GeoParquetDriver;

async fn geojson_to_geoparquet(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open(input).await?;
    let src_layer = src.layer(0)?;

    let driver = GeoParquetDriver::new();
    let mut dst = driver.create(output).await?;

    let srs = src_layer.spatial_ref()?;
    let mut dst_layer = dst.create_layer("data", Some(&srs))?;

    // Copy schema and data
    let layer_def = src_layer.definition()?;
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        dst_layer.create_field(field.name(), field.field_type(), field.width())?;
    }

    for feature in src_layer.features()? {
        dst_layer.add_feature(feature)?;
    }

    dst.flush().await?;
    Ok(())
}
```

### FlatGeobuf to Shapefile

```rust
use oxigdal_shapefile::ShapefileDriver;

async fn flatgeobuf_to_shapefile(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open(input).await?;
    let src_layer = src.layer(0)?;

    let driver = ShapefileDriver::new();
    let mut dst = driver.create(output).await?;

    let srs = src_layer.spatial_ref()?;
    let mut dst_layer = dst.create_layer("layer", Some(&srs))?;

    // Copy fields
    let layer_def = src_layer.definition()?;
    for i in 0..layer_def.field_count() {
        let field = layer_def.field(i)?;
        dst_layer.create_field(field.name(), field.field_type(), field.width())?;
    }

    // Copy features
    for feature in src_layer.features()? {
        dst_layer.add_feature(feature)?;
    }

    dst.flush().await?;
    Ok(())
}
```

## Batch Conversion

### Convert Multiple Files

```rust
use std::path::Path;

async fn batch_convert(
    input_dir: &str,
    output_dir: &str,
    input_ext: &str,
    output_ext: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(output_dir)?;

    let entries = std::fs::read_dir(input_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some(input_ext) {
            let filename = path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Invalid filename")?;

            let output_path = Path::new(output_dir)
                .join(format!("{}.{}", filename, output_ext));

            convert_file(path.to_str().unwrap(), output_path.to_str().unwrap()).await?;

            println!("Converted: {} -> {}", path.display(), output_path.display());
        }
    }

    Ok(())
}

async fn convert_file(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    dataset.save(output).await?;
    Ok(())
}
```

## Format with Options

### GeoTIFF with Compression

```rust
use oxigdal_geotiff::{GeoTiffDriver, GeoTiffOptions};

async fn create_compressed_geotiff(
    data: &[f32],
    width: u32,
    height: u32,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let driver = GeoTiffDriver::new();

    let options = GeoTiffOptions {
        compression: "DEFLATE".to_string(),
        compression_level: Some(9),
        tiled: true,
        tile_width: Some(256),
        tile_height: Some(256),
        ..Default::default()
    };

    let mut dataset = driver.create_with_options(output, width, height, 1, options).await?;
    dataset.band_mut(1)?.write_block(0, 0, width as usize, height as usize, data).await?;
    dataset.flush().await?;

    Ok(())
}
```

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
