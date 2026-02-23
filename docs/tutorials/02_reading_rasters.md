# Reading Raster Data in OxiGDAL

## Overview

This tutorial covers advanced techniques for reading raster data efficiently in OxiGDAL, including windowed reading, tiled processing, and handling different data types.

## Basic Raster Reading

### Opening a Raster Dataset

```rust
use oxigdal_core::Dataset;

async fn open_raster() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("satellite_image.tif").await?;

    println!("Raster size: {} x {}", dataset.width(), dataset.height());
    println!("Number of bands: {}", dataset.band_count());
    println!("Data type: {:?}", dataset.band(1)?.data_type());

    Ok(())
}
```

### Reading Metadata

```rust
use oxigdal_core::Dataset;

async fn read_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("dem.tif").await?;

    // Geotransform: [origin_x, pixel_width, rotation_x, origin_y, rotation_y, pixel_height]
    let geo_transform = dataset.geo_transform()?;
    println!("Origin: ({}, {})", geo_transform[0], geo_transform[3]);
    println!("Pixel size: ({}, {})", geo_transform[1], geo_transform[5]);

    // Spatial reference
    let srs = dataset.spatial_ref()?;
    println!("Projection: {}", srs.to_wkt()?);

    // Band statistics
    let band = dataset.band(1)?;
    if let Some(no_data) = band.no_data_value() {
        println!("NoData value: {}", no_data);
    }

    Ok(())
}
```

## Reading Data

### Reading Entire Band

```rust
use oxigdal_core::{Dataset, DataType};

async fn read_entire_band() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("temperature.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut buffer = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut buffer).await?;

    println!("Read {} pixels", buffer.len());
    Ok(buffer)
}
```

### Reading a Window

```rust
async fn read_window() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("large_raster.tif").await?;
    let band = dataset.band(1)?;

    // Read a 256x256 window starting at pixel (1000, 2000)
    let x_off = 1000;
    let y_off = 2000;
    let width = 256;
    let height = 256;

    let mut buffer = vec![0.0f32; width * height];
    band.read_block(x_off, y_off, width, height, &mut buffer).await?;

    Ok(buffer)
}
```

### Reading with Resampling

```rust
use oxigdal_core::{Dataset, ResampleAlg};

async fn read_with_resampling() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("highres.tif").await?;
    let band = dataset.band(1)?;

    // Read entire raster but resample to 512x512
    let target_width = 512;
    let target_height = 512;

    let mut buffer = vec![0.0f32; target_width * target_height];
    band.read_block_resampled(
        0,
        0,
        band.width() as usize,
        band.height() as usize,
        target_width,
        target_height,
        &mut buffer,
        ResampleAlg::Bilinear,
    ).await?;

    Ok(buffer)
}
```

## Tiled Processing

### Processing in Tiles

```rust
use oxigdal_core::Dataset;

async fn process_in_tiles() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("huge.tif").await?;
    let band = dataset.band(1)?;

    let tile_size = 512;
    let width = band.width() as usize;
    let height = band.height() as usize;

    for y in (0..height).step_by(tile_size) {
        for x in (0..width).step_by(tile_size) {
            let x_size = (tile_size).min(width - x);
            let y_size = (tile_size).min(height - y);

            let mut tile = vec![0.0f32; x_size * y_size];
            band.read_block(x, y, x_size, y_size, &mut tile).await?;

            // Process tile
            process_tile(&mut tile, x_size, y_size)?;

            println!("Processed tile at ({}, {})", x, y);
        }
    }

    Ok(())
}

fn process_tile(tile: &mut [f32], _width: usize, _height: usize) -> Result<(), Box<dyn std::error::Error>> {
    // Example: normalize values
    if let (Some(&min), Some(&max)) = (tile.iter().min_by(|a, b| a.partial_cmp(b).unwrap()),
                                       tile.iter().max_by(|a, b| a.partial_cmp(b).unwrap())) {
        let range = max - min;
        if range > 0.0 {
            for pixel in tile.iter_mut() {
                *pixel = (*pixel - min) / range;
            }
        }
    }
    Ok(())
}
```

### Parallel Tile Processing

```rust
use oxigdal_core::Dataset;
use rayon::prelude::*;

async fn parallel_tile_processing() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("input.tif").await?;
    let band = dataset.band(1)?;

    let tile_size = 256;
    let width = band.width() as usize;
    let height = band.height() as usize;

    // Generate tile coordinates
    let mut tiles = Vec::new();
    for y in (0..height).step_by(tile_size) {
        for x in (0..width).step_by(tile_size) {
            tiles.push((x, y));
        }
    }

    // Process tiles in parallel
    let results: Vec<_> = tiles.par_iter()
        .map(|(x, y)| {
            let x_size = (tile_size).min(width - x);
            let y_size = (tile_size).min(height - y);
            (*x, *y, x_size, y_size)
        })
        .collect();

    for (x, y, x_size, y_size) in results {
        println!("Tile ({}, {}) size: {}x{}", x, y, x_size, y_size);
    }

    Ok(())
}
```

## Multi-Band Reading

### Reading All Bands

```rust
use oxigdal_core::Dataset;

async fn read_all_bands() -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("rgb_image.tif").await?;

    let width = dataset.width() as usize;
    let height = dataset.height() as usize;
    let band_count = dataset.band_count();

    let mut bands_data = Vec::new();

    for band_idx in 1..=band_count {
        let band = dataset.band(band_idx)?;
        let mut buffer = vec![0.0f32; width * height];
        band.read_block(0, 0, width, height, &mut buffer).await?;
        bands_data.push(buffer);
    }

    println!("Read {} bands", bands_data.len());
    Ok(bands_data)
}
```

### Interleaved Band Reading

```rust
async fn read_interleaved() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("multispectral.tif").await?;

    let width = dataset.width() as usize;
    let height = dataset.height() as usize;
    let band_count = dataset.band_count();

    // Create interleaved buffer (RGBRGBRGB...)
    let mut buffer = vec![0.0f32; width * height * band_count];

    for band_idx in 1..=band_count {
        let band = dataset.band(band_idx)?;
        let mut band_buffer = vec![0.0f32; width * height];
        band.read_block(0, 0, width, height, &mut band_buffer).await?;

        // Interleave into main buffer
        for i in 0..band_buffer.len() {
            buffer[i * band_count + (band_idx - 1)] = band_buffer[i];
        }
    }

    Ok(buffer)
}
```

## Handling Different Data Types

### Type Conversion

```rust
use oxigdal_core::{Dataset, DataType};

async fn handle_data_types() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("mixed_types.tif").await?;
    let band = dataset.band(1)?;

    match band.data_type() {
        DataType::UInt8 => {
            let mut buffer = vec![0u8; band.width() as usize * band.height() as usize];
            band.read_block_as::<u8>(0, 0, band.width() as usize, band.height() as usize, &mut buffer).await?;
        }
        DataType::Int16 => {
            let mut buffer = vec![0i16; band.width() as usize * band.height() as usize];
            band.read_block_as::<i16>(0, 0, band.width() as usize, band.height() as usize, &mut buffer).await?;
        }
        DataType::Float32 => {
            let mut buffer = vec![0.0f32; band.width() as usize * band.height() as usize];
            band.read_block_as::<f32>(0, 0, band.width() as usize, band.height() as usize, &mut buffer).await?;
        }
        _ => {
            return Err("Unsupported data type".into());
        }
    }

    Ok(())
}
```

## Handling NoData Values

### Filtering NoData

```rust
use oxigdal_core::Dataset;

async fn handle_nodata() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let dataset = Dataset::open("with_nodata.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut buffer = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut buffer).await?;

    // Get NoData value
    let no_data = band.no_data_value().unwrap_or(f32::NAN);

    // Filter out NoData values
    let valid_data: Vec<f32> = buffer.iter()
        .filter(|&&v| (v - no_data).abs() > f32::EPSILON)
        .copied()
        .collect();

    println!("Valid pixels: {} / {}", valid_data.len(), buffer.len());

    Ok(valid_data)
}
```

## Memory-Efficient Reading

### Streaming Large Files

```rust
use oxigdal_core::Dataset;
use std::io::Write;

async fn stream_to_file() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("very_large.tif").await?;
    let band = dataset.band(1)?;

    let chunk_size = 1024;
    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut output = std::fs::File::create("output.bin")?;

    for y in (0..height).step_by(chunk_size) {
        let y_size = chunk_size.min(height - y);
        let mut buffer = vec![0.0f32; width * y_size];

        band.read_block(0, y, width, y_size, &mut buffer).await?;

        // Write to file
        let bytes: &[u8] = bytemuck::cast_slice(&buffer);
        output.write_all(bytes)?;

        println!("Processed {} / {} rows", y + y_size, height);
    }

    Ok(())
}
```

## Overview and Statistics

### Computing Statistics

```rust
use oxigdal_core::Dataset;

async fn compute_statistics() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("data.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut buffer = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut buffer).await?;

    let no_data = band.no_data_value();

    // Compute statistics
    let valid_pixels: Vec<f32> = buffer.iter()
        .filter(|&&v| no_data.map_or(true, |nd| (v - nd).abs() > f32::EPSILON))
        .copied()
        .collect();

    if valid_pixels.is_empty() {
        return Err("No valid pixels".into());
    }

    let sum: f32 = valid_pixels.iter().sum();
    let mean = sum / valid_pixels.len() as f32;

    let variance: f32 = valid_pixels.iter()
        .map(|&v| (v - mean).powi(2))
        .sum::<f32>() / valid_pixels.len() as f32;
    let std_dev = variance.sqrt();

    let min = valid_pixels.iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .copied()
        .unwrap_or(0.0);
    let max = valid_pixels.iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .copied()
        .unwrap_or(0.0);

    println!("Statistics:");
    println!("  Count: {}", valid_pixels.len());
    println!("  Min: {}", min);
    println!("  Max: {}", max);
    println!("  Mean: {}", mean);
    println!("  Std Dev: {}", std_dev);

    Ok(())
}
```

## Performance Tips

1. **Use appropriate tile sizes** - 256x256 or 512x512 typically work well
2. **Leverage async I/O** - Don't block on I/O operations
3. **Process in parallel** - Use rayon for CPU-bound operations
4. **Minimize memory allocation** - Reuse buffers when possible
5. **Read only what you need** - Use windowed reading for large files

## Complete Example: NDVI Calculation

```rust
use oxigdal_core::Dataset;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open multispectral image (assumes bands 4=NIR, 3=Red)
    let dataset = Dataset::open("landsat.tif").await?;

    let nir_band = dataset.band(4)?;
    let red_band = dataset.band(3)?;

    let width = nir_band.width() as usize;
    let height = nir_band.height() as usize;

    // Read bands
    let mut nir = vec![0.0f32; width * height];
    let mut red = vec![0.0f32; width * height];

    nir_band.read_block(0, 0, width, height, &mut nir).await?;
    red_band.read_block(0, 0, width, height, &mut red).await?;

    // Calculate NDVI: (NIR - Red) / (NIR + Red)
    let mut ndvi = vec![0.0f32; width * height];
    for i in 0..ndvi.len() {
        let sum = nir[i] + red[i];
        ndvi[i] = if sum > 0.0 {
            (nir[i] - red[i]) / sum
        } else {
            -1.0  // NoData
        };
    }

    // Create output dataset
    let mut output = Dataset::create("ndvi.tif", width as u32, height as u32, 1).await?;
    output.set_geo_transform(dataset.geo_transform()?)?;
    output.set_spatial_ref(&dataset.spatial_ref()?)?;

    let out_band = output.band_mut(1)?;
    out_band.set_no_data_value(-1.0)?;
    out_band.write_block(0, 0, width, height, &ndvi).await?;

    output.flush().await?;
    println!("NDVI calculation complete!");

    Ok(())
}
```

## Next Steps

- Learn about [Raster Operations](03_raster_operations.md)
- Explore [Cloud Data](04_cloud_data.md) access
- Study [Performance Tuning](09_performance_tuning.md)

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
