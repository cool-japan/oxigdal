# Raster Processing Recipes

Common recipes for raster data processing with OxiGDAL.

## Reading Rasters

### Read a Single Band

```rust
use oxigdal_core::Dataset;

async fn read_band() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("elevation.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut data = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut data).await?;

    Ok(data)
}
```

### Read Specific Window

```rust
async fn read_window() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("large.tif").await?;
    let band = dataset.band(1)?;

    let mut window = vec![0.0f32; 512 * 512];
    band.read_block(1000, 2000, 512, 512, &mut window).await?;

    Ok(window)
}
```

## Creating Rasters

### Create GeoTIFF from Array

```rust
use oxigdal_core::{Dataset, DataType};

async fn create_from_array(data: &[f32], width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut dataset = Dataset::create("output.tif", width, height, 1).await?;

    let band = dataset.band_mut(1)?;
    band.write_block(0, 0, width as usize, height as usize, data).await?;

    dataset.flush().await?;
    Ok(())
}
```

### Create Multi-band Image

```rust
async fn create_rgb(r: &[u8], g: &[u8], b: &[u8], width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut dataset = Dataset::create_with_type("rgb.tif", width, height, 3, DataType::UInt8).await?;

    dataset.band_mut(1)?.write_block_as::<u8>(0, 0, width as usize, height as usize, r).await?;
    dataset.band_mut(2)?.write_block_as::<u8>(0, 0, width as usize, height as usize, g).await?;
    dataset.band_mut(3)?.write_block_as::<u8>(0, 0, width as usize, height as usize, b).await?;

    dataset.flush().await?;
    Ok(())
}
```

## Reprojection

### Reproject to WGS84

```rust
use oxigdal_proj::SpatialRef;

async fn reproject_to_wgs84(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let wgs84 = SpatialRef::from_epsg(4326)?;

    let reprojected = dataset.reproject(&wgs84, None).await?;
    reprojected.save(output).await?;

    Ok(())
}
```

### Reproject to Web Mercator

```rust
async fn reproject_to_web_mercator(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let web_mercator = SpatialRef::from_epsg(3857)?;

    let reprojected = dataset.reproject(&web_mercator, None).await?;
    reprojected.save("web_mercator.tif").await?;

    Ok(())
}
```

## Resampling

### Downsample by Factor

```rust
use oxigdal_core::ResampleAlg;

async fn downsample(input: &str, factor: u32) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let new_width = dataset.width() / factor;
    let new_height = dataset.height() / factor;

    let resampled = dataset.resample(new_width, new_height, ResampleAlg::Average).await?;
    resampled.save("downsampled.tif").await?;

    Ok(())
}
```

### Upsample with Interpolation

```rust
async fn upsample(input: &str, factor: u32) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let new_width = dataset.width() * factor;
    let new_height = dataset.height() * factor;

    let resampled = dataset.resample(new_width, new_height, ResampleAlg::Cubic).await?;
    resampled.save("upsampled.tif").await?;

    Ok(())
}
```

## Clipping

### Clip by Bounding Box

```rust
use oxigdal_core::BoundingBox;

async fn clip_bbox(input: &str, bbox: BoundingBox) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let clipped = dataset.clip_by_bbox(&bbox).await?;
    clipped.save("clipped.tif").await?;

    Ok(())
}
```

## Band Math

### Calculate NDVI

```rust
async fn calculate_ndvi(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let nir_band = dataset.band(4)?;
    let red_band = dataset.band(3)?;

    let width = nir_band.width() as usize;
    let height = nir_band.height() as usize;

    let mut nir = vec![0.0f32; width * height];
    let mut red = vec![0.0f32; width * height];

    nir_band.read_block(0, 0, width, height, &mut nir).await?;
    red_band.read_block(0, 0, width, height, &mut red).await?;

    let ndvi: Vec<f32> = nir.iter()
        .zip(red.iter())
        .map(|(n, r)| {
            let sum = n + r;
            if sum.abs() > f32::EPSILON {
                (n - r) / sum
            } else {
                -1.0
            }
        })
        .collect();

    let mut output = Dataset::create("ndvi.tif", width as u32, height as u32, 1).await?;
    output.set_geo_transform(dataset.geo_transform()?)?;
    output.set_spatial_ref(&dataset.spatial_ref()?)?;
    output.band_mut(1)?.set_no_data_value(-1.0)?;
    output.band_mut(1)?.write_block(0, 0, width, height, &ndvi).await?;
    output.flush().await?;

    Ok(())
}
```

### Calculate EVI

```rust
async fn calculate_evi(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let nir = read_band_data(&dataset, 4).await?;
    let red = read_band_data(&dataset, 3).await?;
    let blue = read_band_data(&dataset, 1).await?;

    // EVI = 2.5 * ((NIR - Red) / (NIR + 6 * Red - 7.5 * Blue + 1))
    let evi: Vec<f32> = nir.iter()
        .zip(red.iter().zip(blue.iter()))
        .map(|(n, (r, b))| {
            let denom = n + 6.0 * r - 7.5 * b + 1.0;
            if denom.abs() > f32::EPSILON {
                2.5 * ((n - r) / denom)
            } else {
                -1.0
            }
        })
        .collect();

    save_band_data(&dataset, "evi.tif", &evi).await?;
    Ok(())
}

async fn read_band_data(dataset: &Dataset, band_idx: usize) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let band = dataset.band(band_idx)?;
    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut data = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut data).await?;

    Ok(data)
}

async fn save_band_data(dataset: &Dataset, path: &str, data: &[f32]) -> Result<(), Box<dyn std::error::Error>> {
    let width = dataset.width();
    let height = dataset.height();

    let mut output = Dataset::create(path, width, height, 1).await?;
    output.set_geo_transform(dataset.geo_transform()?)?;
    output.set_spatial_ref(&dataset.spatial_ref()?)?;
    output.band_mut(1)?.write_block(0, 0, width as usize, height as usize, data).await?;
    output.flush().await?;

    Ok(())
}
```

## Statistics

### Compute Band Statistics

```rust
async fn compute_statistics(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut data = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut data).await?;

    let no_data = band.no_data_value();
    let valid: Vec<f32> = data.iter()
        .filter(|&&v| no_data.map_or(true, |nd| (v - nd).abs() > f32::EPSILON))
        .copied()
        .collect();

    if valid.is_empty() {
        return Err("No valid data".into());
    }

    let sum: f32 = valid.iter().sum();
    let mean = sum / valid.len() as f32;

    let variance: f32 = valid.iter()
        .map(|&v| (v - mean).powi(2))
        .sum::<f32>() / valid.len() as f32;

    let std_dev = variance.sqrt();

    let min = valid.iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .copied()
        .unwrap_or(0.0);

    let max = valid.iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .copied()
        .unwrap_or(0.0);

    println!("Statistics:");
    println!("  Count: {}", valid.len());
    println!("  Min: {:.2}", min);
    println!("  Max: {:.2}", max);
    println!("  Mean: {:.2}", mean);
    println!("  Std Dev: {:.2}", std_dev);

    Ok(())
}
```

## Format Conversion

### Convert to COG

```rust
use oxigdal_geotiff::{GeoTiffDriver, CogOptions};

async fn convert_to_cog(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
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

### Convert Between Formats

```rust
async fn convert_format(input: &str, output: &str, format: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    match format {
        "GTiff" => dataset.save(output).await?,
        "PNG" => dataset.save_as_png(output).await?,
        "JPEG" => dataset.save_as_jpeg(output).await?,
        _ => return Err("Unsupported format".into()),
    }

    Ok(())
}
```

## Tiling

### Create Tiles

```rust
async fn create_tiles(input: &str, tile_size: usize, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    std::fs::create_dir_all(output_dir)?;

    let mut tile_idx = 0;

    for y in (0..height).step_by(tile_size) {
        for x in (0..width).step_by(tile_size) {
            let x_size = tile_size.min(width - x);
            let y_size = tile_size.min(height - y);

            let mut tile_data = vec![0.0f32; x_size * y_size];
            band.read_block(x, y, x_size, y_size, &mut tile_data).await?;

            let output_path = format!("{}/tile_{}.tif", output_dir, tile_idx);

            let mut tile_dataset = Dataset::create(&output_path, x_size as u32, y_size as u32, 1).await?;
            tile_dataset.band_mut(1)?.write_block(0, 0, x_size, y_size, &tile_data).await?;
            tile_dataset.flush().await?;

            tile_idx += 1;
        }
    }

    println!("Created {} tiles", tile_idx);
    Ok(())
}
```

## Masking

### Apply NoData Mask

```rust
async fn apply_mask(input: &str, mask: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;
    let mask_dataset = Dataset::open(mask).await?;

    let width = dataset.width() as usize;
    let height = dataset.height() as usize;

    let mut data = vec![0.0f32; width * height];
    let mut mask_data = vec![0u8; width * height];

    dataset.band(1)?.read_block(0, 0, width, height, &mut data).await?;
    mask_dataset.band(1)?.read_block_as::<u8>(0, 0, width, height, &mut mask_data).await?;

    // Apply mask
    for (pixel, &mask_val) in data.iter_mut().zip(mask_data.iter()) {
        if mask_val == 0 {
            *pixel = -9999.0;  // NoData value
        }
    }

    let mut output_dataset = Dataset::create(output, width as u32, height as u32, 1).await?;
    output_dataset.set_geo_transform(dataset.geo_transform()?)?;
    output_dataset.set_spatial_ref(&dataset.spatial_ref()?)?;
    output_dataset.band_mut(1)?.set_no_data_value(-9999.0)?;
    output_dataset.band_mut(1)?.write_block(0, 0, width, height, &data).await?;
    output_dataset.flush().await?;

    Ok(())
}
```

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
