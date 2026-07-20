//! Tutorial 04: Cloud Data Access
//!
//! This tutorial demonstrates accessing geospatial data from cloud storage:
//! - Reading from AWS S3 / Azure Blob Storage / Google Cloud Storage
//! - HTTP/HTTPS data sources
//! - Caching and retry strategies
//! - STAC (SpatioTemporal Asset Catalog) integration
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_04_cloud_data
//! ```
//!
//! Note: Some operations require cloud credentials to be configured

use oxigdal_cloud::auth::Credentials;
use oxigdal_cloud::cache::CacheConfig;
use oxigdal_cloud::retry::RetryConfig;
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_stac::ItemBuilder;
use std::env;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 04: Cloud Data Access ===\n");

    // Step 1: HTTP/HTTPS Data Sources
    println!("Step 1: HTTP/HTTPS Data Sources");
    println!("--------------------------------");

    let http_url = "https://example.com/data/sample.tif";
    println!("HTTP URL: {}", http_url);

    // Configure HTTP backend with retry
    let retry_config = RetryConfig::new()
        .with_max_retries(3)
        .with_initial_backoff(Duration::from_millis(100))
        .with_max_backoff(Duration::from_secs(5))
        .with_backoff_multiplier(2.0);

    println!("\nHTTP backend configuration:");
    println!("  Max retries: {}", retry_config.max_retries);
    println!("  Initial backoff: {:?}", retry_config.initial_backoff);
    println!("  Note: enable oxigdal-cloud's `http` feature for `HttpBackend`");

    // For demonstration, we'll use a local file as example
    let temp_dir = env::temp_dir();
    let local_test_file = temp_dir.join("cloud_example.tif");

    if local_test_file.exists() {
        println!("\nReading local test file (simulating HTTP source)...");
        read_and_print_metadata(&local_test_file)?;
    } else {
        println!(
            "\nNote: To use HTTP sources, fetch bytes with `HttpBackend` (feature `http`) and"
        );
        println!("wrap them in a `DataSource` implementation before opening.");
    }

    // Step 2: AWS S3 Integration
    println!("\n\nStep 2: AWS S3 Integration");
    println!("---------------------------");

    println!("S3 configuration:");
    println!("  Bucket: my-geospatial-data");
    println!("  Key: path/to/data.tif");
    println!("  Region: us-west-2");
    println!("  Note: enable oxigdal-cloud's `s3` feature for `S3Backend`");

    let s3_credentials = match (
        env::var("AWS_ACCESS_KEY_ID").ok(),
        env::var("AWS_SECRET_ACCESS_KEY").ok(),
    ) {
        (Some(access), Some(secret)) => Some(Credentials::access_key(access, secret)),
        _ => None,
    };

    println!("\nAuthentication:");
    println!(
        "  Credentials: {}",
        if s3_credentials.is_some() {
            "Configured from environment"
        } else {
            "Not configured (using default credentials)"
        }
    );

    let s3_path = "s3://my-bucket/path/to/data.tif";
    println!("\nS3 Path: {}", s3_path);
    println!("Note: Requires valid AWS credentials to access");

    // Step 3: Azure Blob Storage
    println!("\n\nStep 3: Azure Blob Storage");
    println!("---------------------------");

    println!("Azure configuration:");
    println!("  Storage account: myaccount");
    println!("  Container: geospatial");
    println!("  Blob: path/to/data.tif");

    let azure_credentials = env::var("AZURE_STORAGE_SAS_TOKEN")
        .ok()
        .map(Credentials::sas_token);

    println!("\nAuthentication:");
    println!(
        "  Credentials: {}",
        if azure_credentials.is_some() {
            "Configured from environment (SAS token)"
        } else {
            "Not configured"
        }
    );

    // Step 4: Google Cloud Storage
    println!("\n\nStep 4: Google Cloud Storage");
    println!("-----------------------------");

    println!("GCS configuration:");
    println!("  Bucket: my-gcs-bucket");
    println!("  Object: path/to/data.tif");

    let gcs_credentials = env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .ok()
        .and_then(|path| Credentials::service_account_from_file(path).ok());

    println!("\nAuthentication:");
    println!(
        "  Credentials: {}",
        if gcs_credentials.is_some() {
            "Service account from environment"
        } else {
            "Default credentials"
        }
    );

    // Step 5: Caching Strategies
    println!("\n\nStep 5: Caching Strategies");
    println!("---------------------------");

    let cache_config = CacheConfig::new()
        .with_cache_dir(temp_dir.join("oxigdal_cache"))
        .with_max_memory_size(256 * 1024 * 1024)
        .with_max_disk_size(1024 * 1024 * 1024)
        .with_default_ttl(Duration::from_secs(3600))
        .with_compress(true);

    println!("Cache configuration:");
    println!("  Cache directory: {:?}", cache_config.cache_dir);
    println!("  Max memory size: {} bytes", cache_config.max_memory_size);
    println!("  Max disk size: {} bytes", cache_config.max_disk_size);
    println!("  Default TTL: {:?}", cache_config.default_ttl);
    println!("  Compression: {}", cache_config.compress);

    println!("\nCache operations:");
    println!("  - Automatic caching of downloaded tiles");
    println!("  - Configurable eviction strategy when the cache is full");
    println!("  - Compressed storage for efficiency");
    println!("  - TTL-based expiration");

    // Step 6: STAC (SpatioTemporal Asset Catalog) Integration
    println!("\n\nStep 6: STAC Integration");
    println!("------------------------");

    println!("STAC enables discovery and access to geospatial assets");

    let stac_url = "https://example.com/stac/catalog.json";
    println!("\nSTAC Catalog URL: {}", stac_url);

    // Create a mock STAC item for demonstration
    let geometry = oxigdal_stac::geojson::Geometry::new_polygon(vec![vec![
        vec![-180.0, -90.0],
        vec![180.0, -90.0],
        vec![180.0, 90.0],
        vec![-180.0, 90.0],
        vec![-180.0, -90.0],
    ]]);

    let stac_item = ItemBuilder::new("LC08_L1TP_001001_20200101_20200101_01_T1")
        .geometry(geometry)
        .bbox(-180.0, -90.0, 180.0, 90.0)
        .property("platform", serde_json::json!("Landsat-8"))
        .property("instruments", serde_json::json!(["OLI", "TIRS"]))
        .simple_asset("B4", "s3://landsat-data/LC08/.../B4.TIF")
        .simple_asset("B8", "s3://landsat-data/LC08/.../B8.TIF")
        .collection("landsat-8-l1")
        .build()?;

    println!("\nSTAC Item:");
    println!("  ID: {}", stac_item.id);
    println!("  Bounds: {:?}", stac_item.bbox);
    println!("  Assets: B4 (Red), B8 (Panchromatic)");

    // Query STAC catalog
    println!("\nSTAC Query Example:");
    println!("  Collection: landsat-8-l1");
    println!("  Date range: 2020-01-01 to 2020-12-31");
    println!("  Bounds: [-180, -90, 180, 90]");
    println!("  Cloud cover: < 10%");

    // Step 7: Best Practices
    println!("\n\nStep 7: Best Practices for Cloud Access");
    println!("----------------------------------------");

    println!("\n1. Use Cloud-Optimized Formats:");
    println!("   - COG (Cloud-Optimized GeoTIFF) for rasters");
    println!("   - FlatGeobuf for vectors");
    println!("   - Zarr for multi-dimensional arrays");

    println!("\n2. Enable Caching:");
    println!("   - Reduces redundant downloads");
    println!("   - Improves performance for repeated access");
    println!("   - Configure appropriate cache size and TTL");

    println!("\n3. Implement Retry Logic:");
    println!("   - Handle transient network failures");
    println!("   - Use exponential backoff");
    println!("   - Set reasonable timeout values");

    println!("\n4. Optimize Read Patterns:");
    println!("   - Request only needed tiles/regions");
    println!("   - Use overviews for low-resolution views");
    println!("   - Batch requests when possible");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. HTTP/HTTPS data sources");
    println!("  2. AWS S3 integration");
    println!("  3. Azure Blob Storage");
    println!("  4. Google Cloud Storage");
    println!("  5. Caching strategies");
    println!("  6. STAC catalog integration");
    println!("  7. Best practices for cloud access");

    println!("\nKey Points:");
    println!("  - Cloud-optimized formats enable efficient partial reads");
    println!("  - Caching dramatically improves repeated-access performance");
    println!("  - STAC provides standardized asset discovery");
    println!("  - Proper authentication and retry logic are essential");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 05 for temporal analysis");

    Ok(())
}

/// Helper function to read and print raster metadata
fn read_and_print_metadata(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use oxigdal_core::io::FileDataSource;

    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source)?;

    println!("Metadata:");
    println!("  Size: {}x{}", reader.width(), reader.height());
    println!("  Bands: {}", reader.band_count());
    println!("  Data type: {:?}", reader.data_type());

    if let Some(epsg) = reader.epsg_code() {
        println!("  EPSG: {}", epsg);
    }

    Ok(())
}
