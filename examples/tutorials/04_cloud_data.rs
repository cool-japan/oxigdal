//! Tutorial 04: Cloud Data Access
//!
//! This tutorial demonstrates accessing geospatial data from cloud storage:
//! - Reading from AWS S3
//! - Reading from Azure Blob Storage
//! - Reading from Google Cloud Storage
//! - HTTP/HTTPS data sources
//! - Caching and optimization strategies
//! - STAC (SpatioTemporal Asset Catalog) integration
//!
//! Run with:
//! ```bash
//! cargo run --example 04_cloud_data
//! ```
//!
//! Note: Some operations require cloud credentials to be configured

use oxigdal_cloud::auth::{AuthConfig, CloudProvider};
use oxigdal_cloud::backends::{HttpBackend, S3Backend};
use oxigdal_cloud::cache::{CacheConfig, CloudCache};
use oxigdal_cloud::prefetch::PrefetchStrategy;
use oxigdal_cloud::retry::RetryConfig;
use oxigdal_core::io::DataSource;
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_stac::{StacCatalog, StacItem};
use std::env;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 04: Cloud Data Access ===\n");

    // Step 1: HTTP/HTTPS Data Sources
    println!("Step 1: HTTP/HTTPS Data Sources");
    println!("--------------------------------");

    // Example: Reading a public COG (Cloud-Optimized GeoTIFF) from HTTP
    let http_url = "https://example.com/data/sample.tif";
    println!("HTTP URL: {}", http_url);
    println!("Note: Using a real public COG URL for demonstration");

    // Configure HTTP backend with retry
    let retry_config = RetryConfig {
        max_retries: 3,
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(5),
        backoff_multiplier: 2.0,
    };

    let http_backend = HttpBackend::new(retry_config);

    println!("\nHTTP backend configuration:");
    println!("  Max retries: {}", retry_config.max_retries);
    println!("  Initial backoff: {:?}", retry_config.initial_backoff);

    // For demonstration, we'll use a local file as example
    // In production, replace with actual HTTP URL
    let temp_dir = env::temp_dir();
    let local_test_file = temp_dir.join("cloud_example.tif");

    if local_test_file.exists() {
        println!("\nReading local test file (simulating HTTP source)...");
        read_and_print_metadata(&local_test_file)?;
    } else {
        println!("\nNote: To use HTTP sources, provide a URL to a COG file");
        println!("Example: oxigdal_cloud::http::open(\"https://...\")?;");
    }

    // Step 2: AWS S3 Integration
    println!("\n\nStep 2: AWS S3 Integration");
    println!("---------------------------");

    println!("S3 configuration:");
    println!("  Bucket: my-geospatial-data");
    println!("  Key: path/to/data.tif");
    println!("  Region: us-west-2");

    // Configure S3 backend
    let s3_config = AuthConfig {
        provider: CloudProvider::AWS,
        region: Some("us-west-2".to_string()),
        access_key: env::var("AWS_ACCESS_KEY_ID").ok(),
        secret_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
        token: env::var("AWS_SESSION_TOKEN").ok(),
        endpoint: None,
    };

    println!("\nAuthentication:");
    println!("  Provider: AWS");
    println!("  Region: {}", s3_config.region.as_ref().ok_or("No region")?);
    println!("  Credentials: {}", if s3_config.access_key.is_some() {
        "Configured from environment"
    } else {
        "Not configured (using default credentials)"
    });

    // Example S3 path
    let s3_path = "s3://my-bucket/path/to/data.tif";
    println!("\nS3 Path: {}", s3_path);
    println!("Note: Requires valid AWS credentials to access");

    // In production:
    // let s3_backend = S3Backend::new(s3_config).await?;
    // let s3_source = s3_backend.open("my-bucket", "path/to/data.tif").await?;
    // let reader = GeoTiffReader::open(s3_source)?;

    // Step 3: Azure Blob Storage
    println!("\n\nStep 3: Azure Blob Storage");
    println!("---------------------------");

    println!("Azure configuration:");
    println!("  Storage account: myaccount");
    println!("  Container: geospatial");
    println!("  Blob: path/to/data.tif");

    let azure_config = AuthConfig {
        provider: CloudProvider::Azure,
        region: None,
        access_key: env::var("AZURE_STORAGE_ACCOUNT").ok(),
        secret_key: env::var("AZURE_STORAGE_KEY").ok(),
        token: None,
        endpoint: Some("https://myaccount.blob.core.windows.net".to_string()),
    };

    println!("\nAuthentication:");
    println!("  Provider: Azure");
    println!("  Endpoint: {}", azure_config.endpoint.as_ref().ok_or("No endpoint")?);
    println!("  Credentials: {}", if azure_config.access_key.is_some() {
        "Configured from environment"
    } else {
        "Not configured"
    });

    // In production:
    // let azure_backend = AzureBlobBackend::new(azure_config).await?;
    // let azure_source = azure_backend.open("geospatial", "path/to/data.tif").await?;

    // Step 4: Google Cloud Storage
    println!("\n\nStep 4: Google Cloud Storage");
    println!("-----------------------------");

    println!("GCS configuration:");
    println!("  Bucket: my-gcs-bucket");
    println!("  Object: path/to/data.tif");
    println!("  Project: my-project");

    let gcs_config = AuthConfig {
        provider: CloudProvider::GCS,
        region: None,
        access_key: None,
        secret_key: None,
        token: env::var("GOOGLE_APPLICATION_CREDENTIALS").ok(),
        endpoint: None,
    };

    println!("\nAuthentication:");
    println!("  Provider: GCS");
    println!("  Credentials: {}", if gcs_config.token.is_some() {
        "Service account from environment"
    } else {
        "Default credentials"
    });

    // In production:
    // let gcs_backend = GCSBackend::new(gcs_config).await?;
    // let gcs_source = gcs_backend.open("my-gcs-bucket", "path/to/data.tif").await?;

    // Step 5: Caching Strategies
    println!("\n\nStep 5: Caching Strategies");
    println!("---------------------------");

    let cache_config = CacheConfig {
        cache_dir: temp_dir.join("oxigdal_cache"),
        max_cache_size_mb: 1024, // 1 GB
        ttl_seconds: 3600,       // 1 hour
        compression_enabled: true,
    };

    println!("Cache configuration:");
    println!("  Cache directory: {:?}", cache_config.cache_dir);
    println!("  Max size: {} MB", cache_config.max_cache_size_mb);
    println!("  TTL: {} seconds", cache_config.ttl_seconds);
    println!("  Compression: {}", cache_config.compression_enabled);

    let cache = CloudCache::new(cache_config)?;

    println!("\nCache operations:");
    println!("  - Automatic caching of downloaded tiles");
    println!("  - LRU eviction when cache is full");
    println!("  - Compressed storage for efficiency");
    println!("  - TTL-based expiration");

    // Cache statistics
    let cache_stats = cache.stats();
    println!("\nCache statistics:");
    println!("  Entries: {}", cache_stats.entry_count);
    println!("  Size: {:.2} MB", cache_stats.size_bytes as f64 / 1_048_576.0);
    println!("  Hit rate: {:.2}%", cache_stats.hit_rate * 100.0);

    // Step 6: Prefetching Strategies
    println!("\n\nStep 6: Prefetching Strategies");
    println!("-------------------------------");

    println!("Prefetch strategies for optimal performance:");

    // Strategy 1: Sequential prefetch
    println!("\n1. Sequential Prefetch:");
    println!("   - Good for: Linear processing workflows");
    println!("   - Prefetches next N tiles in sequence");
    println!("   - Example: Processing satellite imagery rows");

    let sequential = PrefetchStrategy::Sequential { lookahead: 5 };
    println!("   Configuration: {:?}", sequential);

    // Strategy 2: Spatial prefetch
    println!("\n2. Spatial Prefetch:");
    println!("   - Good for: Map tile servers, viewport rendering");
    println!("   - Prefetches neighboring tiles");
    println!("   - Example: Web map applications");

    let spatial = PrefetchStrategy::Spatial {
        radius: 2,
        priority_order: vec![(0, 1), (1, 0), (0, -1), (-1, 0)],
    };
    println!("   Configuration: {:?}", spatial);

    // Strategy 3: Pyramid prefetch
    println!("\n3. Pyramid Prefetch:");
    println!("   - Good for: Multi-resolution displays");
    println!("   - Prefetches overview levels");
    println!("   - Example: Zoom-to-detail interfaces");

    let pyramid = PrefetchStrategy::Pyramid {
        levels: vec![0, 1, 2],
    };
    println!("   Configuration: {:?}", pyramid);

    // Step 7: STAC (SpatioTemporal Asset Catalog) Integration
    println!("\n\nStep 7: STAC Integration");
    println!("------------------------");

    println!("STAC enables discovery and access to geospatial assets");

    // Example STAC catalog URL
    let stac_url = "https://example.com/stac/catalog.json";
    println!("\nSTAC Catalog URL: {}", stac_url);

    // Create a mock STAC item for demonstration
    let stac_item = StacItem {
        id: "LC08_L1TP_001001_20200101_20200101_01_T1".to_string(),
        bbox: vec![-180.0, -90.0, 180.0, 90.0],
        geometry: serde_json::json!({
            "type": "Polygon",
            "coordinates": [[
                [-180.0, -90.0],
                [180.0, -90.0],
                [180.0, 90.0],
                [-180.0, 90.0],
                [-180.0, -90.0]
            ]]
        }),
        properties: serde_json::json!({
            "datetime": "2020-01-01T00:00:00Z",
            "platform": "Landsat-8",
            "instruments": ["OLI", "TIRS"]
        }),
        assets: serde_json::json!({
            "B4": {
                "href": "s3://landsat-data/LC08/.../B4.TIF",
                "type": "image/tiff; application=geotiff; profile=cloud-optimized",
                "title": "Red Band"
            },
            "B8": {
                "href": "s3://landsat-data/LC08/.../B8.TIF",
                "type": "image/tiff; application=geotiff; profile=cloud-optimized",
                "title": "Panchromatic Band"
            }
        }),
    };

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

    // In production:
    // let catalog = StacCatalog::from_url(stac_url).await?;
    // let items = catalog.search()
    //     .collection("landsat-8-l1")
    //     .datetime("2020-01-01", "2020-12-31")
    //     .bbox(-180.0, -90.0, 180.0, 90.0)
    //     .query("eo:cloud_cover", "<10")
    //     .execute()
    //     .await?;

    // Step 8: Best Practices
    println!("\n\nStep 8: Best Practices for Cloud Access");
    println!("----------------------------------------");

    println!("\n1. Use Cloud-Optimized Formats:");
    println!("   - COG (Cloud-Optimized GeoTIFF) for rasters");
    println!("   - FlatGeobuf for vectors");
    println!("   - Zarr for multi-dimensional arrays");

    println!("\n2. Enable Caching:");
    println!("   - Reduces redundant downloads");
    println!("   - Improves performance for repeated access");
    println!("   - Configure appropriate cache size and TTL");

    println!("\n3. Use Prefetching:");
    println!("   - Choose strategy based on access pattern");
    println!("   - Balance between memory and performance");
    println!("   - Monitor prefetch hit rates");

    println!("\n4. Implement Retry Logic:");
    println!("   - Handle transient network failures");
    println!("   - Use exponential backoff");
    println!("   - Set reasonable timeout values");

    println!("\n5. Optimize Read Patterns:");
    println!("   - Request only needed tiles/regions");
    println!("   - Use overviews for low-resolution views");
    println!("   - Batch requests when possible");

    println!("\n6. Monitor Costs:");
    println!("   - Track data egress");
    println!("   - Use same-region compute and storage");
    println!("   - Implement request caching");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. HTTP/HTTPS data sources");
    println!("  2. AWS S3 integration");
    println!("  3. Azure Blob Storage");
    println!("  4. Google Cloud Storage");
    println!("  5. Caching strategies");
    println!("  6. Prefetching optimization");
    println!("  7. STAC catalog integration");
    println!("  8. Best practices for cloud access");

    println!("\nKey Points:");
    println!("  - Cloud-optimized formats enable efficient partial reads");
    println!("  - Caching and prefetching dramatically improve performance");
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
