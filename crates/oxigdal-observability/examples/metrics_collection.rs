//! Example of collecting custom geospatial metrics.

use opentelemetry::global;
use oxigdal_observability::metrics::GeoMetrics;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get a meter
    let meter = global::meter("oxigdal-example");

    // Create geo metrics
    let metrics = GeoMetrics::new(meter)?;

    // Record raster operations
    metrics.raster.record_read(125.5, 1048576, "GeoTIFF", true);
    metrics.raster.record_write(89.3, 524288, "GeoTIFF", true);
    metrics.raster.inc_active_rasters();

    // Record cache operations
    metrics.cache.record_hit("tile_cache", 8192);
    metrics.cache.record_miss("tile_cache");
    metrics.cache.record_hit_ratio(8, 10, "tile_cache");

    // Record query operations
    metrics.query.record_query(45.2, "spatial", 150, true);

    println!("Metrics collected successfully");

    Ok(())
}
