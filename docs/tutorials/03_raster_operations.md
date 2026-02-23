# Raster Operations in OxiGDAL

## Overview

This tutorial covers common raster operations including reprojection, resampling, clipping, mosaicking, and raster algebra.

## Reprojection

### Basic Reprojection

```rust
use oxigdal_core::Dataset;
use oxigdal_proj::{SpatialRef, Transformer};

async fn reproject_raster() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("utm.tif").await?;
    let src_srs = src.spatial_ref()?;

    // Target: WGS84 Geographic
    let dst_srs = SpatialRef::from_epsg(4326)?;

    let transformer = Transformer::new(&src_srs, &dst_srs)?;
    let dst = transformer.transform_dataset(&src).await?;

    dst.save("wgs84.tif").await?;
    Ok(())
}
```

### Reprojection with Custom Parameters

```rust
use oxigdal_core::{Dataset, ResampleAlg};
use oxigdal_proj::{SpatialRef, ReprojectOptions};

async fn reproject_custom() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("input.tif").await?;

    let src_srs = SpatialRef::from_proj4(
        "+proj=utm +zone=33 +datum=WGS84"
    )?;

    let dst_srs = SpatialRef::from_proj4(
        "+proj=merc +datum=WGS84"
    )?;

    let options = ReprojectOptions {
        resampling: ResampleAlg::Cubic,
        error_threshold: 0.125,
        max_error: 0.0,
    };

    let dst = src.reproject(&dst_srs, Some(options)).await?;
    dst.save("mercator.tif").await?;

    Ok(())
}
```

## Resampling

### Upsampling

```rust
use oxigdal_core::{Dataset, ResampleAlg};

async fn upsample_raster() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("lowres.tif").await?;

    let new_width = src.width() * 2;
    let new_height = src.height() * 2;

    let dst = src.resample(new_width, new_height, ResampleAlg::Bilinear).await?;
    dst.save("highres.tif").await?;

    Ok(())
}
```

### Downsampling

```rust
async fn downsample_raster() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("highres.tif").await?;

    let new_width = src.width() / 2;
    let new_height = src.height() / 2;

    // Use average for downsampling to avoid aliasing
    let dst = src.resample(new_width, new_height, ResampleAlg::Average).await?;
    dst.save("lowres.tif").await?;

    Ok(())
}
```

## Clipping

### Clip by Bounding Box

```rust
use oxigdal_core::{Dataset, BoundingBox};

async fn clip_by_bbox() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("large.tif").await?;

    let bbox = BoundingBox {
        min_x: 100.0,
        min_y: 200.0,
        max_x: 500.0,
        max_y: 600.0,
    };

    let clipped = src.clip_by_bbox(&bbox).await?;
    clipped.save("clipped.tif").await?;

    Ok(())
}
```

### Clip by Polygon

```rust
use oxigdal_core::Dataset;
use geo_types::Polygon;

async fn clip_by_polygon() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("input.tif").await?;

    // Define polygon (e.g., study area boundary)
    let exterior = vec![
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 100.0),
        (0.0, 100.0),
        (0.0, 0.0),
    ];
    let polygon = Polygon::new(exterior.into(), vec![]);

    let clipped = src.clip_by_geometry(&polygon).await?;
    clipped.save("clipped.tif").await?;

    Ok(())
}
```

## Mosaicking

### Simple Mosaic

```rust
use oxigdal_core::Dataset;
use oxigdal_algorithms::mosaic::MosaicOptions;

async fn create_mosaic() -> Result<(), Box<dyn std::error::Error>> {
    let inputs = vec![
        Dataset::open("tile1.tif").await?,
        Dataset::open("tile2.tif").await?,
        Dataset::open("tile3.tif").await?,
        Dataset::open("tile4.tif").await?,
    ];

    let options = MosaicOptions::default();
    let mosaic = Dataset::mosaic(&inputs, options).await?;

    mosaic.save("mosaic.tif").await?;
    Ok(())
}
```

### Blended Mosaic

```rust
use oxigdal_algorithms::mosaic::{MosaicOptions, BlendMethod};

async fn create_blended_mosaic() -> Result<(), Box<dyn std::error::Error>> {
    let inputs = vec![
        Dataset::open("scene1.tif").await?,
        Dataset::open("scene2.tif").await?,
    ];

    let options = MosaicOptions {
        blend_method: BlendMethod::Feather,
        blend_width: 50,
        ..Default::default()
    };

    let mosaic = Dataset::mosaic(&inputs, options).await?;
    mosaic.save("blended.tif").await?;

    Ok(())
}
```

## Raster Algebra

### Band Math

```rust
use oxigdal_core::Dataset;

async fn band_math() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("multiband.tif").await?;

    let band1 = dataset.band(1)?;
    let band2 = dataset.band(2)?;

    let width = band1.width() as usize;
    let height = band1.height() as usize;

    let mut b1_data = vec![0.0f32; width * height];
    let mut b2_data = vec![0.0f32; width * height];

    band1.read_block(0, 0, width, height, &mut b1_data).await?;
    band2.read_block(0, 0, width, height, &mut b2_data).await?;

    // Calculate: output = (band1 + band2) / 2
    let result: Vec<f32> = b1_data.iter()
        .zip(b2_data.iter())
        .map(|(a, b)| (a + b) / 2.0)
        .collect();

    // Save result
    let mut output = Dataset::create("average.tif", width as u32, height as u32, 1).await?;
    output.band_mut(1)?.write_block(0, 0, width, height, &result).await?;
    output.flush().await?;

    Ok(())
}
```

### NDVI Calculation

```rust
async fn calculate_ndvi() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("sentinel2.tif").await?;

    let nir_band = dataset.band(8)?;  // Band 8 is NIR
    let red_band = dataset.band(4)?;  // Band 4 is Red

    let width = nir_band.width() as usize;
    let height = nir_band.height() as usize;

    let mut nir = vec![0.0f32; width * height];
    let mut red = vec![0.0f32; width * height];

    nir_band.read_block(0, 0, width, height, &mut nir).await?;
    red_band.read_block(0, 0, width, height, &mut red).await?;

    // NDVI = (NIR - Red) / (NIR + Red)
    let ndvi: Vec<f32> = nir.iter()
        .zip(red.iter())
        .map(|(n, r)| {
            let sum = n + r;
            if sum.abs() > f32::EPSILON {
                (n - r) / sum
            } else {
                -1.0  // NoData
            }
        })
        .collect();

    // Save NDVI
    let mut output = Dataset::create("ndvi.tif", width as u32, height as u32, 1).await?;
    output.set_geo_transform(dataset.geo_transform()?)?;
    output.set_spatial_ref(&dataset.spatial_ref()?)?;

    let out_band = output.band_mut(1)?;
    out_band.set_no_data_value(-1.0)?;
    out_band.write_block(0, 0, width, height, &ndvi).await?;

    output.flush().await?;
    Ok(())
}
```

## Image Filtering

### Convolution Filter

```rust
use oxigdal_core::Dataset;

async fn apply_convolution() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("input.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut input = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut input).await?;

    // Gaussian blur kernel (3x3)
    let kernel = [
        1.0, 2.0, 1.0,
        2.0, 4.0, 2.0,
        1.0, 2.0, 1.0,
    ];
    let kernel_sum: f32 = kernel.iter().sum();

    let mut output = vec![0.0f32; width * height];

    for y in 1..height-1 {
        for x in 1..width-1 {
            let mut sum = 0.0;

            for ky in 0..3 {
                for kx in 0..3 {
                    let pixel_y = y + ky - 1;
                    let pixel_x = x + kx - 1;
                    let pixel_idx = pixel_y * width + pixel_x;
                    let kernel_idx = ky * 3 + kx;

                    sum += input[pixel_idx] * kernel[kernel_idx];
                }
            }

            output[y * width + x] = sum / kernel_sum;
        }
    }

    // Save filtered image
    let mut out_dataset = Dataset::create("filtered.tif", width as u32, height as u32, 1).await?;
    out_dataset.band_mut(1)?.write_block(0, 0, width, height, &output).await?;
    out_dataset.flush().await?;

    Ok(())
}
```

### Sobel Edge Detection

```rust
async fn sobel_edge_detection() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("input.tif").await?;
    let band = dataset.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut input = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut input).await?;

    // Sobel kernels
    let sobel_x = [-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0];
    let sobel_y = [-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0];

    let mut output = vec![0.0f32; width * height];

    for y in 1..height-1 {
        for x in 1..width-1 {
            let mut gx = 0.0;
            let mut gy = 0.0;

            for ky in 0..3 {
                for kx in 0..3 {
                    let pixel_y = y + ky - 1;
                    let pixel_x = x + kx - 1;
                    let pixel_idx = pixel_y * width + pixel_x;
                    let kernel_idx = ky * 3 + kx;

                    gx += input[pixel_idx] * sobel_x[kernel_idx];
                    gy += input[pixel_idx] * sobel_y[kernel_idx];
                }
            }

            output[y * width + x] = (gx * gx + gy * gy).sqrt();
        }
    }

    // Save edge map
    let mut out_dataset = Dataset::create("edges.tif", width as u32, height as u32, 1).await?;
    out_dataset.band_mut(1)?.write_block(0, 0, width, height, &output).await?;
    out_dataset.flush().await?;

    Ok(())
}
```

## Raster Conversion

### Data Type Conversion

```rust
use oxigdal_core::{Dataset, DataType};

async fn convert_data_type() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("float_data.tif").await?;
    let band = src.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    // Read as float
    let mut float_data = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut float_data).await?;

    // Convert to UInt8 (0-255)
    let uint8_data: Vec<u8> = float_data.iter()
        .map(|&v| ((v * 255.0).max(0.0).min(255.0)) as u8)
        .collect();

    // Create output with new data type
    let mut output = Dataset::create_with_type(
        "uint8_data.tif",
        width as u32,
        height as u32,
        1,
        DataType::UInt8,
    ).await?;

    output.band_mut(1)?.write_block_as::<u8>(0, 0, width, height, &uint8_data).await?;
    output.flush().await?;

    Ok(())
}
```

## Hillshade Generation

```rust
use oxigdal_core::Dataset;

async fn generate_hillshade() -> Result<(), Box<dyn std::error::Error>> {
    let dem = Dataset::open("elevation.tif").await?;
    let band = dem.band(1)?;

    let width = band.width() as usize;
    let height = band.height() as usize;

    let mut elevation = vec![0.0f32; width * height];
    band.read_block(0, 0, width, height, &mut elevation).await?;

    // Hillshade parameters
    let azimuth = 315.0_f32.to_radians();  // Sun direction
    let altitude = 45.0_f32.to_radians();  // Sun angle
    let z_factor = 1.0;

    let geo_transform = dem.geo_transform()?;
    let cell_size = geo_transform[1] as f32;

    let mut hillshade = vec![0.0f32; width * height];

    for y in 1..height-1 {
        for x in 1..width-1 {
            // Calculate slope and aspect using 3x3 window
            let z1 = elevation[(y-1)*width + (x-1)];
            let z2 = elevation[(y-1)*width + x];
            let z3 = elevation[(y-1)*width + (x+1)];
            let z4 = elevation[y*width + (x-1)];
            let z6 = elevation[y*width + (x+1)];
            let z7 = elevation[(y+1)*width + (x-1)];
            let z8 = elevation[(y+1)*width + x];
            let z9 = elevation[(y+1)*width + (x+1)];

            let dz_dx = ((z3 + 2.0*z6 + z9) - (z1 + 2.0*z4 + z7)) / (8.0 * cell_size);
            let dz_dy = ((z7 + 2.0*z8 + z9) - (z1 + 2.0*z2 + z3)) / (8.0 * cell_size);

            let slope = (dz_dx*dz_dx + dz_dy*dz_dy).sqrt().atan();
            let aspect = dz_dy.atan2(-dz_dx);

            // Calculate hillshade
            let shade = ((altitude.cos() * slope.cos()) +
                        (altitude.sin() * slope.sin() * (azimuth - std::f32::consts::PI/2.0 - aspect).cos()))
                        .max(0.0) * 255.0;

            hillshade[y*width + x] = shade;
        }
    }

    // Save hillshade
    let mut output = Dataset::create("hillshade.tif", width as u32, height as u32, 1).await?;
    output.set_geo_transform(dem.geo_transform()?)?;
    output.set_spatial_ref(&dem.spatial_ref()?)?;
    output.band_mut(1)?.write_block(0, 0, width, height, &hillshade).await?;
    output.flush().await?;

    Ok(())
}
```

## Complete Example: Multi-Step Processing

```rust
use oxigdal_core::{Dataset, ResampleAlg};
use oxigdal_proj::SpatialRef;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open input dataset
    let input = Dataset::open("raw_satellite.tif").await?;
    println!("Opened: {} x {}", input.width(), input.height());

    // 2. Reproject to Web Mercator
    let web_mercator = SpatialRef::from_epsg(3857)?;
    let reprojected = input.reproject(&web_mercator, None).await?;
    println!("Reprojected to EPSG:3857");

    // 3. Resample to target resolution
    let target_width = 2048;
    let target_height = 2048;
    let resampled = reprojected.resample(
        target_width,
        target_height,
        ResampleAlg::Lanczos,
    ).await?;
    println!("Resampled to {} x {}", target_width, target_height);

    // 4. Save result
    resampled.save("processed.tif").await?;
    println!("Processing complete!");

    Ok(())
}
```

## Performance Optimization

1. **Minimize reprojection** - Reproject once, not per-pixel
2. **Choose appropriate resampling** - Lanczos for upsampling, Average for downsampling
3. **Process in tiles** - For large datasets
4. **Reuse buffers** - Avoid repeated allocation
5. **Use appropriate data types** - UInt8 for display, Float32 for analysis

## Next Steps

- Learn about [Vector Operations](04_vector_data.md)
- Explore [Projections](05_projections.md) in detail
- Study [ML Inference](07_ml_inference.md) for advanced analysis

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
