//! Vector Analysis with PostGIS Example
//!
//! This example demonstrates spatial vector analysis using the real
//! `oxigeo-postgis` crate:
//! - Building a connection pool (`ConnectionConfig` / `ConnectionPool`)
//! - Building spatial SQL with `SpatialQuery` (works without a live database)
//! - Writing/reading features via `PostGisWriter` / `PostGisReader`
//! - Graceful fallback when no PostGIS database is reachable
//!
//! Run with:
//! ```bash
//! cargo run --example vector_postgis
//! ```
//!
//! Set `PGHOST`/`PGPORT`/`PGUSER`/`PGPASSWORD`/`PGDATABASE` to point at a real
//! PostGIS instance to exercise the read/write path; otherwise the example
//! demonstrates query building and reports that no database was reachable.

use oxigeo_core::vector::{Feature, FieldValue, Geometry, Point};
use oxigeo_postgis::{
    ConnectionConfig, ConnectionPool, PostGisReader, PostGisWriter, SpatialQuery,
};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("vector_postgis=info")
        .init();

    info!("Starting Vector Analysis with PostGIS");

    // Step 1: Build connection configuration
    info!("Step 1: Building connection configuration");

    let config = ConnectionConfig::new(
        std::env::var("PGDATABASE").unwrap_or_else(|_| "gis_analysis".to_string()),
    )
    .host(std::env::var("PGHOST").unwrap_or_else(|_| "localhost".to_string()))
    .user(std::env::var("PGUSER").unwrap_or_else(|_| "postgres".to_string()))
    .password(std::env::var("PGPASSWORD").unwrap_or_else(|_| "postgres".to_string()));

    info!("  Connection string: {}", config.to_connection_string());

    let pool = ConnectionPool::new(config.clone())?;
    info!("  Connection pool created (lazy; not yet connected)");
    info!("  Pool status: {:?}", pool.status());

    // Step 2: Build spatial SQL with the query builder (no DB required)
    info!("\nStep 2: Building spatial queries");

    let residential_query = SpatialQuery::new("pois")?
        .select(&["pois.*"])
        .where_clause("category = 'residential'")
        .order_by("name", true);

    info!(
        "  Residential POIs SQL:\n    {}",
        residential_query.build_sql()?
    );

    let bbox = oxigeo_core::types::BoundingBox::new(-10.0, 40.0, -9.0, 41.0)?;
    let bbox_query = SpatialQuery::new("roads")?.where_bbox(&bbox)?;

    info!("  Roads-in-bbox SQL:\n    {}", bbox_query.build_sql()?);

    // Step 3: Attempt a real round-trip (requires a live PostGIS database)
    info!("\nStep 3: Attempting a live database round-trip");

    match run_roundtrip(&config).await {
        Ok(count) => info!("  Round-trip succeeded: read back {} feature(s)", count),
        Err(e) => {
            warn!(
                "  No PostGIS database reachable, skipping round-trip: {}",
                e
            );
            info!("  (Set PGHOST/PGUSER/PGPASSWORD/PGDATABASE to run against a real instance)");
        }
    }

    // Step 4: Summary
    info!("\n=== Analysis Summary ===");
    info!("  Connection target: {}", config.to_connection_string());
    info!("  Spatial queries built: 2");
    info!("");
    info!("Vector PostGIS example completed successfully!");

    Ok(())
}

/// Insert a couple of synthetic point features into a scratch table and read them back.
async fn run_roundtrip(config: &ConnectionConfig) -> Result<usize, Box<dyn std::error::Error>> {
    let write_pool = ConnectionPool::new(config.clone())?;
    let mut writer = PostGisWriter::new(write_pool, "oxigeo_examples_pois")
        .srid(4326)
        .create_table(true);

    writer.ensure_table().await?;

    for (idx, (lon, lat, name)) in [(-9.5, 40.5, "City Hall"), (-9.4, 40.6, "Market Square")]
        .iter()
        .enumerate()
    {
        let mut feature = Feature::new(Geometry::Point(Point::new(*lon, *lat)));
        feature
            .properties
            .insert("name".to_string(), FieldValue::String((*name).to_string()));
        feature
            .properties
            .insert("rank".to_string(), FieldValue::Integer(idx as i64));

        writer.insert(&feature).await?;
    }

    let read_pool = ConnectionPool::new(config.clone())?;
    let mut reader = PostGisReader::new(read_pool, "oxigeo_examples_pois");
    let features = reader.read_all().await?;

    Ok(features.len())
}
