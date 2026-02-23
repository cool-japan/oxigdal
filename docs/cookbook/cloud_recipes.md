# Cloud Storage Recipes

Common recipes for working with cloud-stored geospatial data.

## AWS S3

### Read from S3

```rust
use oxigdal_core::Dataset;
use oxigdal_cloud::s3::S3Options;

async fn read_from_s3() -> Result<(), Box<dyn std::error::Error>> {
    let options = S3Options::default();

    let dataset = Dataset::open_with_options(
        "s3://my-bucket/data/image.tif",
        options,
    ).await?;

    println!("Dataset: {} x {}", dataset.width(), dataset.height());
    Ok(())
}
```

### Write to S3

```rust
use oxigdal_cloud::s3::S3Client;

async fn write_to_s3(local_path: &str, bucket: &str, key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let s3_client = S3Client::default().await?;

    s3_client.upload_file(
        bucket,
        key,
        local_path,
        Default::default(),
    ).await?;

    println!("Uploaded to s3://{}/{}", bucket, key);
    Ok(())
}
```

### List S3 Objects

```rust
async fn list_s3_objects(bucket: &str, prefix: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let s3_client = S3Client::default().await?;

    let objects = s3_client.list_objects(bucket, prefix).await?;

    let keys: Vec<String> = objects.iter()
        .map(|obj| obj.key().to_string())
        .collect();

    println!("Found {} objects", keys.len());
    Ok(keys)
}
```

## Azure Blob Storage

### Read from Azure

```rust
use oxigdal_cloud::azure::AzureOptions;

async fn read_from_azure() -> Result<(), Box<dyn std::error::Error>> {
    let options = AzureOptions {
        account_name: std::env::var("AZURE_STORAGE_ACCOUNT")?,
        container_name: "geospatial".to_string(),
        ..Default::default()
    };

    let dataset = Dataset::open_with_options(
        "azure://geospatial/data/image.tif",
        options,
    ).await?;

    Ok(())
}
```

### Write to Azure

```rust
use oxigdal_cloud::azure::AzureClient;

async fn write_to_azure(local_path: &str, container: &str, blob_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = AzureClient::default().await?;

    client.upload_file(container, blob_name, local_path).await?;

    println!("Uploaded to Azure: {}/{}", container, blob_name);
    Ok(())
}
```

## Google Cloud Storage

### Read from GCS

```rust
use oxigdal_cloud::gcs::GcsOptions;

async fn read_from_gcs() -> Result<(), Box<dyn std::error::Error>> {
    let options = GcsOptions::default();

    let dataset = Dataset::open_with_options(
        "gs://my-bucket/data/image.tif",
        options,
    ).await?;

    Ok(())
}
```

### Write to GCS

```rust
use oxigdal_cloud::gcs::GcsClient;

async fn write_to_gcs(local_path: &str, bucket: &str, object_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = GcsClient::default().await?;

    client.upload_file(bucket, object_name, local_path).await?;

    println!("Uploaded to GCS: gs://{}/{}", bucket, object_name);
    Ok(())
}
```

## Cloud Optimized GeoTIFF (COG)

### Create COG

```rust
use oxigdal_geotiff::{GeoTiffDriver, CogOptions};

async fn create_cog(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(input).await?;

    let driver = GeoTiffDriver::new();
    let options = CogOptions {
        tile_size: 512,
        compression: "DEFLATE".to_string(),
        overview_levels: vec![2, 4, 8, 16],
        resampling: "NEAREST".to_string(),
        ..Default::default()
    };

    driver.create_cog(&dataset, output, options).await?;

    println!("Created COG: {}", output);
    Ok(())
}
```

### Read COG from Cloud

```rust
async fn read_cog_from_cloud(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dataset = Dataset::open(url).await?;

    // Read specific region without downloading entire file
    let band = dataset.band(1)?;
    let mut tile = vec![0.0f32; 512 * 512];

    band.read_block(1000, 2000, 512, 512, &mut tile).await?;

    println!("Read {} pixels from COG", tile.len());
    Ok(())
}
```

## STAC Catalog

### Search STAC

```rust
use oxigdal_stac::{StacClient, SearchParams};

async fn search_stac_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;

    let params = SearchParams {
        bbox: Some([-122.5, 37.5, -122.0, 38.0]),
        datetime: Some("2024-01-01/2024-12-31".to_string()),
        collections: vec!["sentinel-2-l2a".to_string()],
        max_items: 10,
        ..Default::default()
    };

    let results = client.search(params).await?;

    for item in results.items() {
        println!("Found: {} ({})", item.id(), item.datetime()?);
    }

    Ok(())
}
```

### Load STAC Asset

```rust
async fn load_stac_asset() -> Result<(), Box<dyn std::error::Error>> {
    let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;

    let item = client.get_item("sentinel-2-l2a", "S2B_36QWD_20240115_0_L2A").await?;

    if let Some(asset) = item.asset("red") {
        let dataset = Dataset::open(asset.href()).await?;
        println!("Loaded red band: {} x {}", dataset.width(), dataset.height());
    }

    Ok(())
}
```

## Batch Cloud Processing

### Process Multiple S3 Files

```rust
async fn process_s3_batch(bucket: &str, prefix: &str) -> Result<(), Box<dyn std::error::Error>> {
    let s3_client = S3Client::default().await?;

    let objects = s3_client.list_objects(bucket, prefix).await?;

    for object in objects {
        let input_path = format!("s3://{}/{}", bucket, object.key());
        let dataset = Dataset::open(&input_path).await?;

        // Process dataset
        let processed = process_dataset(&dataset).await?;

        // Upload result
        let output_key = format!("processed/{}", object.key());
        processed.save(&format!("s3://{}/{}", bucket, output_key)).await?;

        println!("Processed: {}", object.key());
    }

    Ok(())
}

async fn process_dataset(dataset: &Dataset) -> Result<Dataset, Box<dyn std::error::Error>> {
    // Processing logic
    Ok(dataset.clone())
}
```

## Caching

### Local Cache for Cloud Data

```rust
use oxigdal_cloud::cache::{LocalCache, CacheOptions};

async fn use_local_cache(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let cache_options = CacheOptions {
        cache_dir: "/tmp/oxigdal_cache".to_string(),
        max_size_mb: 1024,
        ttl_seconds: 3600,
        ..Default::default()
    };

    let cache = LocalCache::new(cache_options)?;

    // First access downloads
    let dataset1 = cache.get_or_fetch(url).await?;

    // Second access uses cache
    let dataset2 = cache.get_or_fetch(url).await?;

    println!("Cache hit on second access");
    Ok(())
}
```

## Parallel Downloads

### Download Multiple Files

```rust
use futures::future::join_all;

async fn parallel_download(urls: &[String]) -> Result<Vec<Dataset>, Box<dyn std::error::Error>> {
    let futures: Vec<_> = urls.iter()
        .map(|url| Dataset::open(url))
        .collect();

    let datasets: Vec<_> = join_all(futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    println!("Downloaded {} datasets", datasets.len());
    Ok(datasets)
}
```

---

Copyright (c) 2025 COOLJAPAN OU (Team Kitasan)
Licensed under Apache-2.0
