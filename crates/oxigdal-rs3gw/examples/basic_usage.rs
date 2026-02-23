//! Basic usage example for oxigdal-rs3gw
//!
//! This example demonstrates how to create a data source backed by rs3gw
//! using a local filesystem backend.
//!
//! Run with:
//! ```bash
//! cargo run --example basic_usage --features async
//! ```

use oxigdal_core::io::{ByteRange, DataSource};
use oxigdal_rs3gw::{OxigdalBackend, Rs3gwDataSource};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("=== OxiGDAL rs3gw Basic Usage ===\n");

    // Create a temporary directory for testing
    let temp_dir = std::env::temp_dir().join("oxigdal-rs3gw-example");
    std::fs::create_dir_all(&temp_dir)?;

    println!("Using storage root: {}", temp_dir.display());

    // Configure local filesystem backend
    let backend = OxigdalBackend::Local {
        root: temp_dir.clone(),
    };

    println!("Creating storage backend...");
    let storage = backend.create_storage().await?;

    // Create a test bucket
    println!("Creating test bucket...");
    storage.create_bucket("example-bucket").await?;

    // Create a test file
    println!("Writing test data...");
    let test_data = b"Hello from OxiGDAL + rs3gw! This is a geospatial data source.";
    storage
        .put_object(
            "example-bucket",
            "test.txt",
            bytes::Bytes::from(&test_data[..]),
            std::collections::HashMap::new(),
        )
        .await?;

    // Create a data source
    println!("\nCreating Rs3gwDataSource...");
    let source = Rs3gwDataSource::new(
        storage,
        "example-bucket".to_string(),
        "test.txt".to_string(),
    )
    .await?;

    // Query size
    let size = source.size()?;
    println!("Data source size: {} bytes", size);

    // Read the entire file
    println!("\nReading entire file...");
    let range = ByteRange::new(0, size);
    let data = source.read_range(range)?;
    println!("Read {} bytes", data.len());
    println!("Content: {}", String::from_utf8_lossy(&data));

    // Read a partial range
    println!("\nReading partial range (0-10)...");
    let partial = source.read_range(ByteRange::new(0, 10))?;
    println!("Partial content: {}", String::from_utf8_lossy(&partial));

    // Read multiple ranges
    println!("\nReading multiple ranges...");
    let ranges = vec![ByteRange::new(0, 5), ByteRange::new(6, 11)];
    let results = source.read_ranges(&ranges)?;
    println!("Range 1: {}", String::from_utf8_lossy(&results[0]));
    println!("Range 2: {}", String::from_utf8_lossy(&results[1]));

    println!("\n=== Example completed successfully ===");

    // Cleanup
    std::fs::remove_dir_all(&temp_dir)?;

    Ok(())
}
