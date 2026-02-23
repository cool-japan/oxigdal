//! Tests for custom geospatial metrics.

use opentelemetry::global;
use oxigdal_observability::metrics::GeoMetrics;

#[test]
fn test_geo_metrics_creation() {
    let meter = global::meter("test");
    let result = GeoMetrics::new(meter);
    assert!(result.is_ok());
}

#[test]
fn test_raster_metrics() {
    let meter = global::meter("test");
    let metrics = GeoMetrics::new(meter).expect("Failed to create metrics");

    metrics.raster.record_read(100.0, 1024, "GeoTIFF", true);
    metrics.raster.record_write(200.0, 2048, "GeoTIFF", true);
    metrics.raster.inc_active_rasters();
    metrics.raster.dec_active_rasters();
}

#[test]
fn test_cache_metrics() {
    let meter = global::meter("test");
    let metrics = GeoMetrics::new(meter).expect("Failed to create metrics");

    metrics.cache.record_hit("tile", 1024);
    metrics.cache.record_miss("tile");
    metrics.cache.record_hit_ratio(8, 10, "tile");
}
