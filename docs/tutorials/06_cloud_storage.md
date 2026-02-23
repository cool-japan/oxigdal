# Cloud Storage Integration

## Overview

This tutorial covers accessing and processing geospatial data stored in cloud object storage including AWS S3, Azure Blob Storage, and Google Cloud Storage.

## AWS S3 Integration

### Basic S3 Access

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::S3Options;

async fn open_from_s3() -> Result<(), Box<dyn std::error::Error>> {
    let options = S3Options {
        region: "us-west-2".to_string(),
        access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok(),
        secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
        ..Default::default()
    };

    let s3_path = "s3://my-bucket/data/satellite.tif";
    let dataset = Dataset::open_with_options(s3_path, options).await?;

    println!("Opened dataset: {} x {}", dataset.width(), dataset.height());
    Ok(())
}
```

### Using AWS SDK Credentials

```rust
use oxigdal_cloud::s3::S3Client;
use aws_config::load_from_env;

async fn use_aws_credentials() -> Result<(), Box<dyn std::error::Error>> {
    // Load AWS credentials from environment
    let config = load_from_env().await;
    let s3_client = S3Client::from_config(&config).await?;

    let dataset = s3_client.open_dataset(
        "my-bucket",
        "path/to/data.tif",
    ).await?;

    println!("Dataset loaded from S3");
    Ok(())
}
```

### Reading COG from S3

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::S3Options;

async fn read_cog_from_s3() -> Result<(), Box<dyn std::error::Error>> {
    let options = S3Options::default();

    // COG (Cloud Optimized GeoTIFF) allows efficient random access
    let dataset = Dataset::open_with_options(
        "s3://satellite-imagery/landsat/LC08_L1TP_042034_20201231.tif",
        options,
    ).await?;

    // Read a specific region without downloading entire file
    let band = dataset.band(1)?;
    let tile_size = 512;

    let mut tile = vec![0.0f32; tile_size * tile_size];
    band.read_block(1000, 2000, tile_size, tile_size, &mut tile).await?;

    println!("Read {} pixels from COG", tile.len());
    Ok(())
}
```

## Azure Blob Storage

### Azure Blob Access

```rust
use oxigdal_cloud::azure::AzureOptions;
use oxigdal_core::Dataset;

async fn open_from_azure() -> Result<(), Box<dyn std::error::Error>> {
    let options = AzureOptions {
        account_name: std::env::var("AZURE_STORAGE_ACCOUNT")?,
        account_key: std::env::var("AZURE_STORAGE_KEY").ok(),
        container_name: "geospatial-data".to_string(),
        ..Default::default()
    };

    let blob_path = "azure://geospatial-data/satellite/image.tif";
    let dataset = Dataset::open_with_options(blob_path, options).await?;

    println!("Opened from Azure: {} x {}", dataset.width(), dataset.height());
    Ok(())
}
```

### Azure Data Lake

```rust
use oxigdal_cloud::azure::{AzureClient, DataLakeOptions};

async fn access_data_lake() -> Result<(), Box<dyn std::error::Error>> {
    let options = DataLakeOptions {
        account_name: std::env::var("AZURE_STORAGE_ACCOUNT")?,
        filesystem: "production".to_string(),
        use_managed_identity: true,
        ..Default::default()
    };

    let client = AzureClient::new(options).await?;
    let dataset = client.open_dataset("geospatial/dem.tif").await?;

    println!("Opened from Data Lake");
    Ok(())
}
```

## Google Cloud Storage

### GCS Access

```rust
use oxigdal_cloud::gcs::GcsOptions;
use oxigdal_core::Dataset;

async fn open_from_gcs() -> Result<(), Box<dyn std::error::Error>> {
    let options = GcsOptions {
        project_id: std::env::var("GCP_PROJECT_ID").ok(),
        credentials_path: std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok(),
        ..Default::default()
    };

    let gcs_path = "gs://earth-engine-public/satellite/imagery.tif";
    let dataset = Dataset::open_with_options(gcs_path, options).await?;

    println!("Opened from GCS: {} x {}", dataset.width(), dataset.height());
    Ok(())
}
```

## STAC (SpatioTemporal Asset Catalog)

### Searching STAC Catalog

```rust
use oxigdal_stac::{StacClient, SearchParams};

async fn search_stac() -> Result<(), Box<dyn std::error::Error>> {
    let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;

    let params = SearchParams {
        bbox: Some([-122.5, 37.7, -122.3, 37.9]),  // San Francisco
        datetime: Some("2024-01-01/2024-12-31".to_string()),
        collections: vec!["sentinel-2-l2a".to_string()],
        max_items: 10,
        ..Default::default()
    };

    let results = client.search(params).await?;

    for item in results.items() {
        println!("Item ID: {}", item.id());
        println!("  Datetime: {}", item.datetime()?);
        println!("  Cloud cover: {}", item.cloud_cover()?);

        // Access assets
        if let Some(asset) = item.asset("visual") {
            println!("  Visual asset: {}", asset.href());
        }
    }

    Ok(())
}
```

### Loading STAC Item

```rust
use oxigdal_stac::StacClient;
use oxigdal_core::Dataset;

async fn load_stac_item() -> Result<(), Box<dyn std::error::Error>> {
    let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;

    let item = client.get_item(
        "sentinel-2-l2a",
        "S2B_36QWD_20240115_0_L2A",
    ).await?;

    // Load the red band
    let red_asset = item.asset("red").ok_or("Red band not found")?;
    let dataset = Dataset::open(red_asset.href()).await?;

    println!("Loaded band: {} x {}", dataset.width(), dataset.height());
    Ok(())
}
```

## Cloud-Optimized Formats

### Creating COG (Cloud Optimized GeoTIFF)

```rust
use oxigdal_core::{Dataset, Driver};
use oxigdal_geotiff::{GeoTiffDriver, CogOptions};

async fn create_cog() -> Result<(), Box<dyn std::error::Error>> {
    // Open source dataset
    let src = Dataset::open("large_image.tif").await?;

    // COG options
    let options = CogOptions {
        tile_size: 512,
        compression: "DEFLATE".to_string(),
        overview_levels: vec![2, 4, 8, 16],
        resampling: "NEAREST".to_string(),
        ..Default::default()
    };

    // Create COG
    let driver = GeoTiffDriver::new();
    let cog = driver.create_cog(&src, "output.tif", options).await?;

    println!("Created COG with {} overview levels", cog.overview_count()?);
    Ok(())
}
```

### Writing to Cloud Storage

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::{S3Client, UploadOptions};

async fn upload_to_s3() -> Result<(), Box<dyn std::error::Error>> {
    // Create dataset locally first
    let dataset = Dataset::create("temp.tif", 1024, 1024, 3).await?;

    // ... populate with data ...

    dataset.flush().await?;

    // Upload to S3
    let s3_client = S3Client::default().await?;

    let options = UploadOptions {
        acl: Some("public-read".to_string()),
        storage_class: Some("STANDARD_IA".to_string()),
        ..Default::default()
    };

    s3_client.upload_file(
        "my-bucket",
        "processed/output.tif",
        "temp.tif",
        options,
    ).await?;

    println!("Uploaded to S3");
    Ok(())
}
```

## Streaming Operations

### Streaming Processing

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::S3Options;

async fn stream_process() -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open_with_options(
        "s3://large-bucket/huge-file.tif",
        S3Options::default(),
    ).await?;

    let band = dataset.band(1)?;
    let tile_size = 256;

    let width = band.width() as usize;
    let height = band.height() as usize;

    // Process in tiles without loading entire file
    for y in (0..height).step_by(tile_size) {
        for x in (0..width).step_by(tile_size) {
            let x_size = tile_size.min(width - x);
            let y_size = tile_size.min(height - y);

            let mut tile = vec![0.0f32; x_size * y_size];
            band.read_block(x, y, x_size, y_size, &mut tile).await?;

            // Process tile
            process_tile(&tile)?;
        }
    }

    println!("Streaming processing complete");
    Ok(())
}

fn process_tile(tile: &[f32]) -> Result<(), Box<dyn std::error::Error>> {
    // Tile processing logic
    Ok(())
}
```

## Distributed Cloud Processing

### AWS Lambda Integration

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::aws::LambdaClient;

async fn lambda_processing() -> Result<(), Box<dyn std::error::Error>> {
    let lambda_client = LambdaClient::default().await?;

    let payload = serde_json::json!({
        "bucket": "satellite-data",
        "key": "scene1.tif",
        "operation": "ndvi",
    });

    let result = lambda_client.invoke(
        "geospatial-processor",
        &payload,
    ).await?;

    println!("Lambda result: {:?}", result);
    Ok(())
}
```

### Cloud Function for Batch Processing

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::S3Client;

async fn batch_cloud_processing() -> Result<(), Box<dyn std::error::Error>> {
    let s3_client = S3Client::default().await?;

    // List all files in bucket
    let objects = s3_client.list_objects("input-bucket", "satellite/").await?;

    for object in objects {
        println!("Processing: {}", object.key());

        // Open from S3
        let dataset = Dataset::open(&format!("s3://input-bucket/{}", object.key())).await?;

        // Process
        let processed = process_dataset(&dataset).await?;

        // Save to output bucket
        let output_key = format!("processed/{}", object.key());
        processed.save(&format!("s3://output-bucket/{}", output_key)).await?;

        println!("Completed: {}", output_key);
    }

    Ok(())
}

async fn process_dataset(dataset: &Dataset) -> Result<Dataset, Box<dyn std::error::Error>> {
    // Processing logic
    Ok(dataset.clone())
}
```

## Caching Strategies

### Local Cache for Cloud Data

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::cache::{CacheOptions, LocalCache};

async fn use_cache() -> Result<(), Box<dyn std::error::Error>> {
    let cache_options = CacheOptions {
        cache_dir: "/tmp/oxigdal_cache".to_string(),
        max_size_mb: 1024,
        ttl_seconds: 3600,
        ..Default::default()
    };

    let cache = LocalCache::new(cache_options)?;

    // First access - downloads from cloud
    let dataset1 = cache.get_or_fetch(
        "s3://my-bucket/data.tif"
    ).await?;

    // Second access - uses cached version
    let dataset2 = cache.get_or_fetch(
        "s3://my-bucket/data.tif"
    ).await?;

    println!("Cache hit on second access");
    Ok(())
}
```

## Complete Example: Cloud Mosaic Pipeline

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::{S3Client, S3Options};
use oxigdal_algorithms::mosaic::MosaicOptions;
use oxigdal_stac::{StacClient, SearchParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Search for Sentinel-2 scenes via STAC
    let stac_client = StacClient::new("https://earth-search.aws.element84.com/v1")?;

    let params = SearchParams {
        bbox: Some([-123.0, 37.0, -122.0, 38.0]),
        datetime: Some("2024-06-01/2024-06-30".to_string()),
        collections: vec!["sentinel-2-l2a".to_string()],
        query: vec![("eo:cloud_cover".to_string(), "<10".to_string())],
        max_items: 5,
        ..Default::default()
    };

    let results = stac_client.search(params).await?;
    println!("Found {} scenes", results.items().len());

    // 2. Load datasets from S3
    let mut datasets = Vec::new();

    for item in results.items() {
        if let Some(asset) = item.asset("visual") {
            println!("Loading: {}", asset.href());

            let dataset = Dataset::open_with_options(
                asset.href(),
                S3Options::default(),
            ).await?;

            datasets.push(dataset);
        }
    }

    // 3. Create mosaic
    let options = MosaicOptions {
        blend_method: BlendMethod::Feather,
        ..Default::default()
    };

    let mosaic = Dataset::mosaic(&datasets, options).await?;
    println!("Created mosaic: {} x {}", mosaic.width(), mosaic.height());

    // 4. Save as COG to S3
    let cog_options = CogOptions {
        tile_size: 512,
        compression: "DEFLATE".to_string(),
        overview_levels: vec![2, 4, 8],
        ..Default::default()
    };

    let cog = mosaic.to_cog(cog_options).await?;

    // Save locally first, then upload
    cog.save("mosaic.tif").await?;

    let s3_client = S3Client::default().await?;
    s3_client.upload_file(
        "output-bucket",
        "mosaics/2024-06-mosaic.tif",
        "mosaic.tif",
        Default::default(),
    ).await?;

    println!("Mosaic uploaded to S3!");

    Ok(())
}
```

## Performance Tips

1. **Use COG format** - Enables efficient random access
2. **Implement caching** - Reduce cloud API calls
3. **Process in tiles** - Minimize data transfer
4. **Parallel downloads** - Use async operations
5. **Right-size instances** - Match compute to workload

## Cost Optimization

```rust
use oxigdal_cloud::cost::{CostEstimator, StorageClass};

async fn estimate_costs() -> Result<(), Box<dyn std::error::Error>> {
    let estimator = CostEstimator::new("us-west-2");

    let file_size_gb = 10.0;
    let read_operations = 1000;

    // S3 Standard
    let standard_cost = estimator.estimate_storage(
        file_size_gb,
        StorageClass::Standard,
    )?;

    // S3 Infrequent Access
    let ia_cost = estimator.estimate_storage(
        file_size_gb,
        StorageClass::InfrequentAccess,
    )?;

    let transfer_cost = estimator.estimate_egress(file_size_gb)?;
    let api_cost = estimator.estimate_api_calls(read_operations)?;

    println!("Monthly storage (Standard): ${:.2}", standard_cost);
    println!("Monthly storage (IA): ${:.2}", ia_cost);
    println!("Data transfer: ${:.2}", transfer_cost);
    println!("API calls: ${:.2}", api_cost);

    Ok(())
}
```

## Next Steps

- Learn about [ML Inference](07_ml_inference.md)
- Explore [Distributed Processing](08_distributed_processing.md)
- Study [Performance Tuning](09_performance_tuning.md)

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
