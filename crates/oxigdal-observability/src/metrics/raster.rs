//! Raster operation metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};

/// Metrics for raster operations.
pub struct RasterMetrics {
    // Read operations
    /// Counter for raster read operations.
    pub read_count: Counter<u64>,
    /// Histogram of raster read durations.
    pub read_duration: Histogram<f64>,
    /// Total bytes read from rasters.
    pub read_bytes: Counter<u64>,

    // Write operations
    /// Counter for raster write operations.
    pub write_count: Counter<u64>,
    /// Histogram of raster write durations.
    pub write_duration: Histogram<f64>,
    /// Total bytes written to rasters.
    pub write_bytes: Counter<u64>,

    // Processing operations
    /// Counter for reprojection operations.
    pub reproject_count: Counter<u64>,
    /// Histogram of reprojection durations.
    pub reproject_duration: Histogram<f64>,
    /// Counter for resample operations.
    pub resample_count: Counter<u64>,
    /// Histogram of resample durations.
    pub resample_duration: Histogram<f64>,
    /// Counter for warp operations.
    pub warp_count: Counter<u64>,
    /// Histogram of warp durations.
    pub warp_duration: Histogram<f64>,

    // Compression operations
    /// Counter for compression operations.
    pub compress_count: Counter<u64>,
    /// Histogram of compression durations.
    pub compress_duration: Histogram<f64>,
    /// Histogram of compression ratios achieved.
    pub compress_ratio: Histogram<f64>,
    /// Counter for decompression operations.
    pub decompress_count: Counter<u64>,
    /// Histogram of decompression durations.
    pub decompress_duration: Histogram<f64>,

    // Band operations
    /// Counter for band read operations.
    pub band_read_count: Counter<u64>,
    /// Histogram of band read durations.
    pub band_read_duration: Histogram<f64>,
    /// Counter for band write operations.
    pub band_write_count: Counter<u64>,
    /// Histogram of band write durations.
    pub band_write_duration: Histogram<f64>,

    // Tile operations
    /// Counter for tiles generated.
    pub tile_count: Counter<u64>,
    /// Histogram of tile generation durations.
    pub tile_generation_duration: Histogram<f64>,
    /// Counter for tile cache hits.
    pub tile_cache_hits: Counter<u64>,
    /// Counter for tile cache misses.
    pub tile_cache_misses: Counter<u64>,

    // Overview operations
    /// Counter for overviews generated.
    pub overview_count: Counter<u64>,
    /// Histogram of overview generation durations.
    pub overview_generation_duration: Histogram<f64>,

    // Statistics
    /// Current number of active raster datasets.
    pub active_rasters: UpDownCounter<i64>,
    /// Histogram of raster widths in pixels.
    pub raster_width: Histogram<f64>,
    /// Histogram of raster heights in pixels.
    pub raster_height: Histogram<f64>,
    /// Histogram of raster band counts.
    pub raster_bands: Histogram<f64>,
    /// Histogram of raster sizes in bytes.
    pub raster_size_bytes: Histogram<f64>,
}

impl RasterMetrics {
    /// Create new raster metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // Read operations
            read_count: meter
                .u64_counter("oxigdal.raster.read.count")
                .with_description("Number of raster read operations")
                .build(),
            read_duration: meter
                .f64_histogram("oxigdal.raster.read.duration")
                .with_description("Duration of raster read operations in milliseconds")
                .build(),
            read_bytes: meter
                .u64_counter("oxigdal.raster.read.bytes")
                .with_description("Bytes read from raster")
                .build(),

            // Write operations
            write_count: meter
                .u64_counter("oxigdal.raster.write.count")
                .with_description("Number of raster write operations")
                .build(),
            write_duration: meter
                .f64_histogram("oxigdal.raster.write.duration")
                .with_description("Duration of raster write operations in milliseconds")
                .build(),
            write_bytes: meter
                .u64_counter("oxigdal.raster.write.bytes")
                .with_description("Bytes written to raster")
                .build(),

            // Processing operations
            reproject_count: meter
                .u64_counter("oxigdal.raster.reproject.count")
                .with_description("Number of raster reprojection operations")
                .build(),
            reproject_duration: meter
                .f64_histogram("oxigdal.raster.reproject.duration")
                .with_description("Duration of raster reprojection in milliseconds")
                .build(),
            resample_count: meter
                .u64_counter("oxigdal.raster.resample.count")
                .with_description("Number of raster resample operations")
                .build(),
            resample_duration: meter
                .f64_histogram("oxigdal.raster.resample.duration")
                .with_description("Duration of raster resample in milliseconds")
                .build(),
            warp_count: meter
                .u64_counter("oxigdal.raster.warp.count")
                .with_description("Number of raster warp operations")
                .build(),
            warp_duration: meter
                .f64_histogram("oxigdal.raster.warp.duration")
                .with_description("Duration of raster warp in milliseconds")
                .build(),

            // Compression operations
            compress_count: meter
                .u64_counter("oxigdal.raster.compress.count")
                .with_description("Number of raster compression operations")
                .build(),
            compress_duration: meter
                .f64_histogram("oxigdal.raster.compress.duration")
                .with_description("Duration of raster compression in milliseconds")
                .build(),
            compress_ratio: meter
                .f64_histogram("oxigdal.raster.compress.ratio")
                .with_description("Compression ratio achieved")
                .build(),
            decompress_count: meter
                .u64_counter("oxigdal.raster.decompress.count")
                .with_description("Number of raster decompression operations")
                .build(),
            decompress_duration: meter
                .f64_histogram("oxigdal.raster.decompress.duration")
                .with_description("Duration of raster decompression in milliseconds")
                .build(),

            // Band operations
            band_read_count: meter
                .u64_counter("oxigdal.raster.band.read.count")
                .with_description("Number of band read operations")
                .build(),
            band_read_duration: meter
                .f64_histogram("oxigdal.raster.band.read.duration")
                .with_description("Duration of band read in milliseconds")
                .build(),
            band_write_count: meter
                .u64_counter("oxigdal.raster.band.write.count")
                .with_description("Number of band write operations")
                .build(),
            band_write_duration: meter
                .f64_histogram("oxigdal.raster.band.write.duration")
                .with_description("Duration of band write in milliseconds")
                .build(),

            // Tile operations
            tile_count: meter
                .u64_counter("oxigdal.raster.tile.count")
                .with_description("Number of tiles generated")
                .build(),
            tile_generation_duration: meter
                .f64_histogram("oxigdal.raster.tile.generation.duration")
                .with_description("Duration of tile generation in milliseconds")
                .build(),
            tile_cache_hits: meter
                .u64_counter("oxigdal.raster.tile.cache.hits")
                .with_description("Number of tile cache hits")
                .build(),
            tile_cache_misses: meter
                .u64_counter("oxigdal.raster.tile.cache.misses")
                .with_description("Number of tile cache misses")
                .build(),

            // Overview operations
            overview_count: meter
                .u64_counter("oxigdal.raster.overview.count")
                .with_description("Number of overviews generated")
                .build(),
            overview_generation_duration: meter
                .f64_histogram("oxigdal.raster.overview.generation.duration")
                .with_description("Duration of overview generation in milliseconds")
                .build(),

            // Statistics
            active_rasters: meter
                .i64_up_down_counter("oxigdal.raster.active")
                .with_description("Number of active raster datasets")
                .build(),
            raster_width: meter
                .f64_histogram("oxigdal.raster.width")
                .with_description("Raster width in pixels")
                .build(),
            raster_height: meter
                .f64_histogram("oxigdal.raster.height")
                .with_description("Raster height in pixels")
                .build(),
            raster_bands: meter
                .f64_histogram("oxigdal.raster.bands")
                .with_description("Number of raster bands")
                .build(),
            raster_size_bytes: meter
                .f64_histogram("oxigdal.raster.size.bytes")
                .with_description("Raster size in bytes")
                .build(),
        })
    }

    /// Record raster read operation.
    pub fn record_read(&self, duration_ms: f64, bytes: u64, format: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("format", format.to_string()),
            KeyValue::new("success", success),
        ];

        self.read_count.add(1, &attrs);
        self.read_duration.record(duration_ms, &attrs);
        if success {
            self.read_bytes.add(bytes, &attrs);
        }
    }

    /// Record raster write operation.
    pub fn record_write(&self, duration_ms: f64, bytes: u64, format: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("format", format.to_string()),
            KeyValue::new("success", success),
        ];

        self.write_count.add(1, &attrs);
        self.write_duration.record(duration_ms, &attrs);
        if success {
            self.write_bytes.add(bytes, &attrs);
        }
    }

    /// Record reprojection operation.
    pub fn record_reproject(&self, duration_ms: f64, from_srs: &str, to_srs: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("from_srs", from_srs.to_string()),
            KeyValue::new("to_srs", to_srs.to_string()),
            KeyValue::new("success", success),
        ];

        self.reproject_count.add(1, &attrs);
        self.reproject_duration.record(duration_ms, &attrs);
    }

    /// Record compression operation.
    pub fn record_compress(
        &self,
        duration_ms: f64,
        original_size: u64,
        compressed_size: u64,
        algorithm: &str,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("algorithm", algorithm.to_string()),
            KeyValue::new("success", success),
        ];

        self.compress_count.add(1, &attrs);
        self.compress_duration.record(duration_ms, &attrs);

        if success && original_size > 0 {
            let ratio = compressed_size as f64 / original_size as f64;
            self.compress_ratio.record(ratio, &attrs);
        }
    }

    /// Record tile cache hit.
    pub fn record_tile_cache_hit(&self, zoom: u32) {
        let attrs = vec![KeyValue::new("zoom", zoom as i64)];
        self.tile_cache_hits.add(1, &attrs);
    }

    /// Record tile cache miss.
    pub fn record_tile_cache_miss(&self, zoom: u32) {
        let attrs = vec![KeyValue::new("zoom", zoom as i64)];
        self.tile_cache_misses.add(1, &attrs);
    }

    /// Increment active rasters.
    pub fn inc_active_rasters(&self) {
        self.active_rasters.add(1, &[]);
    }

    /// Decrement active rasters.
    pub fn dec_active_rasters(&self) {
        self.active_rasters.add(-1, &[]);
    }

    /// Record raster dimensions.
    pub fn record_dimensions(&self, width: u32, height: u32, bands: u32) {
        let attrs = vec![KeyValue::new("operation", "open")];
        self.raster_width.record(width as f64, &attrs);
        self.raster_height.record(height as f64, &attrs);
        self.raster_bands.record(bands as f64, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_raster_metrics_creation() {
        let meter = global::meter("test");
        let metrics = RasterMetrics::new(meter);
        assert!(metrics.is_ok());
    }
}
