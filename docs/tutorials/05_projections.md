# Coordinate Reference Systems and Projections

## Overview

This tutorial covers working with coordinate reference systems (CRS), spatial reference systems (SRS), and performing coordinate transformations in OxiGDAL.

## Understanding CRS

### Common Coordinate Systems

```rust
use oxigdal_proj::SpatialRef;

async fn common_crs() -> Result<(), Box<dyn std::error::Error>> {
    // WGS84 Geographic (lat/lon)
    let wgs84 = SpatialRef::from_epsg(4326)?;
    println!("WGS84: {}", wgs84.to_pretty_wkt()?);

    // Web Mercator (used by web maps)
    let web_mercator = SpatialRef::from_epsg(3857)?;
    println!("Web Mercator: {}", web_mercator.to_pretty_wkt()?);

    // UTM Zone 33N
    let utm_33n = SpatialRef::from_epsg(32633)?;
    println!("UTM 33N: {}", utm_33n.to_pretty_wkt()?);

    Ok(())
}
```

### Creating SRS from PROJ4

```rust
use oxigdal_proj::SpatialRef;

async fn create_from_proj4() -> Result<(), Box<dyn std::error::Error>> {
    let proj4_str = "+proj=lcc +lat_1=49 +lat_2=44 +lat_0=46.5 +lon_0=3 +x_0=700000 +y_0=6600000 +ellps=GRS80 +units=m +no_defs";

    let srs = SpatialRef::from_proj4(proj4_str)?;
    println!("Created SRS: {}", srs.to_wkt()?);

    Ok(())
}
```

### Creating SRS from WKT

```rust
use oxigdal_proj::SpatialRef;

async fn create_from_wkt() -> Result<(), Box<dyn std::error::Error>> {
    let wkt = r#"
        GEOGCS["WGS 84",
            DATUM["WGS_1984",
                SPHEROID["WGS 84",6378137,298.257223563]],
            PRIMEM["Greenwich",0],
            UNIT["degree",0.0174532925199433]]
    "#;

    let srs = SpatialRef::from_wkt(wkt)?;
    println!("Authority: {:?}", srs.authority_code()?);

    Ok(())
}
```

## Coordinate Transformations

### Point Transformation

```rust
use oxigdal_proj::{SpatialRef, Transformer};

async fn transform_point() -> Result<(), Box<dyn std::error::Error>> {
    let src_srs = SpatialRef::from_epsg(4326)?;  // WGS84
    let dst_srs = SpatialRef::from_epsg(3857)?;  // Web Mercator

    let transformer = Transformer::new(&src_srs, &dst_srs)?;

    // London coordinates
    let lon = -0.1276;
    let lat = 51.5074;

    let (x, y) = transformer.transform_point(lon, lat)?;
    println!("WGS84: ({}, {}) -> Web Mercator: ({}, {})", lon, lat, x, y);

    Ok(())
}
```

### Batch Transformation

```rust
use oxigdal_proj::{SpatialRef, Transformer};

async fn transform_batch() -> Result<(), Box<dyn std::error::Error>> {
    let src_srs = SpatialRef::from_epsg(4326)?;
    let dst_srs = SpatialRef::from_epsg(32633)?;  // UTM 33N

    let transformer = Transformer::new(&src_srs, &dst_srs)?;

    let coordinates = vec![
        (10.0, 53.5),
        (11.0, 53.5),
        (12.0, 53.5),
    ];

    let transformed = transformer.transform_coords(&coordinates)?;

    for (i, (x, y)) in transformed.iter().enumerate() {
        println!("Point {}: ({:.2}, {:.2})", i, x, y);
    }

    Ok(())
}
```

## Reprojecting Datasets

### Raster Reprojection

```rust
use oxigdal_core::{Dataset, ResampleAlg};
use oxigdal_proj::{SpatialRef, ReprojectOptions};

async fn reproject_raster() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("input_utm.tif").await?;

    // Target: WGS84
    let dst_srs = SpatialRef::from_epsg(4326)?;

    let options = ReprojectOptions {
        resampling: ResampleAlg::Bilinear,
        error_threshold: 0.125,
        max_error: 0.0,
    };

    let dst = src.reproject(&dst_srs, Some(options)).await?;
    dst.save("output_wgs84.tif").await?;

    println!("Reprojection complete!");
    Ok(())
}
```

### Vector Reprojection

```rust
use oxigdal_core::Dataset;
use oxigdal_proj::{SpatialRef, Transformer};

async fn reproject_vector() -> Result<(), Box<dyn std::error::Error>> {
    let src = Dataset::open("input.geojson").await?;
    let src_layer = src.layer(0)?;

    // Source and target SRS
    let src_srs = src_layer.spatial_ref()?;
    let dst_srs = SpatialRef::from_epsg(3857)?;

    let transformer = Transformer::new(&src_srs, &dst_srs)?;

    // Create output
    let mut dst = Dataset::create_vector("output.geojson").await?;
    let mut dst_layer = dst.create_layer("reprojected", Some(&dst_srs))?;

    // Copy features with transformed geometries
    for feature in src_layer.features()? {
        let mut out_feature = dst_layer.create_feature()?;

        let geometry = feature.geometry()?;
        let transformed_geom = transformer.transform_geometry(&geometry)?;

        out_feature.set_geometry(transformed_geom)?;
        dst_layer.add_feature(out_feature)?;
    }

    dst.flush().await?;
    Ok(())
}
```

## Datum Transformations

### High-Accuracy Transformations

```rust
use oxigdal_proj::{SpatialRef, Transformer, TransformOptions};

async fn datum_transformation() -> Result<(), Box<dyn std::error::Error>> {
    // NAD83 to WGS84 (requires datum transformation)
    let nad83 = SpatialRef::from_epsg(4269)?;
    let wgs84 = SpatialRef::from_epsg(4326)?;

    let options = TransformOptions {
        always_xy: true,
        ..Default::default()
    };

    let transformer = Transformer::with_options(&nad83, &wgs84, options)?;

    let (x, y) = transformer.transform_point(-122.4194, 37.7749)?;
    println!("Transformed: ({}, {})", x, y);

    Ok(())
}
```

## Working with UTM

### Determining UTM Zone

```rust
fn get_utm_zone(lon: f64, lat: f64) -> i32 {
    let zone = ((lon + 180.0) / 6.0).floor() as i32 + 1;

    // Handle special cases
    if lat >= 56.0 && lat < 64.0 && lon >= 3.0 && lon < 12.0 {
        return 32;  // Norway special case
    }

    if lat >= 72.0 && lat < 84.0 {
        if lon >= 0.0 && lon < 9.0 {
            return 31;  // Svalbard special case 1
        }
        if lon >= 9.0 && lon < 21.0 {
            return 33;  // Svalbard special case 2
        }
        if lon >= 21.0 && lon < 33.0 {
            return 35;  // Svalbard special case 3
        }
        if lon >= 33.0 && lon < 42.0 {
            return 37;  // Svalbard special case 4
        }
    }

    zone
}

fn get_utm_epsg(lon: f64, lat: f64) -> i32 {
    let zone = get_utm_zone(lon, lat);
    let hemisphere = if lat >= 0.0 { 32600 } else { 32700 };
    hemisphere + zone
}
```

### Auto UTM Projection

```rust
use oxigdal_core::Dataset;
use oxigdal_proj::SpatialRef;

async fn auto_utm_projection() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("wgs84_data.tif").await?;

    // Get center point
    let geo_transform = dataset.geo_transform()?;
    let center_lon = geo_transform[0] + (dataset.width() as f64 * geo_transform[1] / 2.0);
    let center_lat = geo_transform[3] + (dataset.height() as f64 * geo_transform[5] / 2.0);

    // Determine appropriate UTM zone
    let utm_epsg = get_utm_epsg(center_lon, center_lat);
    let utm_srs = SpatialRef::from_epsg(utm_epsg)?;

    println!("Auto-selected UTM zone EPSG:{}", utm_epsg);

    // Reproject to UTM
    let reprojected = dataset.reproject(&utm_srs, None).await?;
    reprojected.save("utm_projection.tif").await?;

    Ok(())
}
```

## Geotransform Operations

### Understanding Geotransform

```rust
use oxigdal_core::Dataset;

async fn analyze_geotransform() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("georeferenced.tif").await?;
    let gt = dataset.geo_transform()?;

    // Geotransform: [origin_x, pixel_width, rotation_x, origin_y, rotation_y, pixel_height]
    println!("Geotransform parameters:");
    println!("  Origin X (top-left): {}", gt[0]);
    println!("  Pixel width: {}", gt[1]);
    println!("  Rotation X: {}", gt[2]);
    println!("  Origin Y (top-left): {}", gt[3]);
    println!("  Rotation Y: {}", gt[4]);
    println!("  Pixel height: {}", gt[5]);

    // Calculate extent
    let width = dataset.width() as f64;
    let height = dataset.height() as f64;

    let min_x = gt[0];
    let max_x = gt[0] + width * gt[1];
    let max_y = gt[3];
    let min_y = gt[3] + height * gt[5];

    println!("Extent: ({}, {}) to ({}, {})", min_x, min_y, max_x, max_y);

    Ok(())
}
```

### Pixel to World Coordinates

```rust
fn pixel_to_world(gt: &[f64; 6], pixel_x: usize, pixel_y: usize) -> (f64, f64) {
    let x = gt[0] + pixel_x as f64 * gt[1] + pixel_y as f64 * gt[2];
    let y = gt[3] + pixel_x as f64 * gt[4] + pixel_y as f64 * gt[5];
    (x, y)
}

fn world_to_pixel(gt: &[f64; 6], world_x: f64, world_y: f64) -> (isize, isize) {
    let det = gt[1] * gt[5] - gt[2] * gt[4];

    let pixel_x = ((world_x - gt[0]) * gt[5] - (world_y - gt[3]) * gt[2]) / det;
    let pixel_y = ((world_y - gt[3]) * gt[1] - (world_x - gt[0]) * gt[4]) / det;

    (pixel_x as isize, pixel_y as isize)
}
```

## Projection Information

### Getting Projection Details

```rust
use oxigdal_core::Dataset;

async fn get_projection_info() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open("data.tif").await?;
    let srs = dataset.spatial_ref()?;

    // Authority code (e.g., EPSG)
    if let Some(code) = srs.authority_code()? {
        println!("EPSG Code: {}", code);
    }

    // Projection name
    println!("Projection: {}", srs.projection_name()?);

    // Linear units
    let (unit_name, unit_value) = srs.linear_units()?;
    println!("Linear units: {} ({})", unit_name, unit_value);

    // Angular units (for geographic CRS)
    if srs.is_geographic()? {
        let (unit_name, unit_value) = srs.angular_units()?;
        println!("Angular units: {} ({})", unit_name, unit_value);
    }

    // Check CRS type
    if srs.is_geographic()? {
        println!("Type: Geographic");
    } else if srs.is_projected()? {
        println!("Type: Projected");
    }

    Ok(())
}
```

## Complete Example: Multi-Source Reprojection

```rust
use oxigdal_core::Dataset;
use oxigdal_proj::{SpatialRef, Transformer};
use oxigdal_algorithms::mosaic::MosaicOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load datasets with different projections
    let dataset1 = Dataset::open("utm_zone_32.tif").await?;
    let dataset2 = Dataset::open("utm_zone_33.tif").await?;
    let dataset3 = Dataset::open("wgs84.tif").await?;

    println!("Dataset 1 SRS: {}", dataset1.spatial_ref()?.authority_code()?.unwrap_or(-1));
    println!("Dataset 2 SRS: {}", dataset2.spatial_ref()?.authority_code()?.unwrap_or(-1));
    println!("Dataset 3 SRS: {}", dataset3.spatial_ref()?.authority_code()?.unwrap_or(-1));

    // 2. Choose common projection (Web Mercator)
    let target_srs = SpatialRef::from_epsg(3857)?;

    // 3. Reproject all datasets
    let reprojected1 = dataset1.reproject(&target_srs, None).await?;
    let reprojected2 = dataset2.reproject(&target_srs, None).await?;
    let reprojected3 = dataset3.reproject(&target_srs, None).await?;

    println!("All datasets reprojected to EPSG:3857");

    // 4. Mosaic reprojected datasets
    let datasets = vec![reprojected1, reprojected2, reprojected3];
    let options = MosaicOptions::default();
    let mosaic = Dataset::mosaic(&datasets, options).await?;

    // 5. Save result
    mosaic.save("merged_web_mercator.tif").await?;

    println!("Mosaic complete!");
    Ok(())
}
```

## Performance Considerations

1. **Cache transformers** - Reuse transformer objects for batch operations
2. **Use appropriate accuracy** - Balance between speed and precision
3. **Minimize reprojections** - Reproject once, not repeatedly
4. **Consider native projection** - Process in native CRS when possible

## Next Steps

- Learn about [Cloud Storage](06_cloud_storage.md)
- Explore [ML Inference](07_ml_inference.md)
- Study [Distributed Processing](08_distributed_processing.md)

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
