//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

/// Integration tests
#[cfg(test)]
mod integration_tests {
    use crate::*;
    /// End-to-end tile loading and caching
    #[test]
    fn test_tile_loading_workflow() {
        // Tile size: 256x256x4 = 262,144 bytes, so cache needs to be at least that large
        let mut cache = TileCache::new(1_000_000);
        let pyramid = TilePyramid::new(1024, 1024, 256, 256);
        let coord1 = TileCoord::new(0, 0, 0);
        let coord2 = TileCoord::new(0, 1, 0);
        let tile_data = vec![0u8; 256 * 256 * 4];
        cache
            .put(coord1, tile_data.clone(), 0.0)
            .expect("Put tile 1");
        cache
            .put(coord2, tile_data.clone(), 1.0)
            .expect("Put tile 2");
        assert!(cache.contains(&coord1));
        assert!(cache.contains(&coord2));
        assert!(pyramid.is_valid_coord(&coord1));
    }
    /// Profiling and performance tracking
    #[test]
    fn test_profiling_workflow() {
        let mut profiler = Profiler::new();
        profiler.start_timer("load_tile", 0.0);
        profiler.stop_timer("load_tile", 10.0);
        profiler.start_timer("render_tile", 10.0);
        profiler.stop_timer("render_tile", 15.0);
        let summary = profiler.summary();
        assert_eq!(summary.counters.len(), 2);
    }
    /// Viewport management and history
    #[test]
    fn test_viewport_workflow() {
        let mut history = ViewportHistory::new(10);
        let mut state = ViewportState::new(800, 600);
        history.push(state.clone());
        state.pan(100.0, 50.0);
        history.push(state.clone());
        state.zoom(1.5, 400.0, 300.0);
        history.push(state.clone());
        assert!(history.can_undo());
        history.undo();
        assert!(history.can_redo());
    }
    /// Rendering pipeline integration
    #[test]
    fn test_rendering_pipeline() {
        let mut buffer = CanvasBuffer::new(512, 512).expect("Create buffer");
        buffer.clear_with_color(0, 0, 0, 255);
        let data = vec![255u8; 256 * 256 * 4];
        let _result = buffer.composite_tile(&data, 256, 256, 0, 0, 1.0);
        assert!(buffer.is_dirty());
    }
    /// Color processing pipeline
    #[test]
    fn test_color_pipeline() {
        let mut data = vec![128, 64, 192, 255];
        ImageProcessor::adjust_brightness(&mut data, 20);
        ImageProcessor::adjust_contrast(&mut data, 1.2);
        ImageProcessor::adjust_saturation(&mut data, 1.1);
        assert_ne!(data[0], 128);
    }
    /// Compression round-trip
    #[test]
    fn test_compression_roundtrip() {
        let original = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let compressed = RleCompressor::compress(&original);
        let decompressed = RleCompressor::decompress(&compressed).expect("Decompress");
        assert_eq!(original, decompressed);
    }
    /// Streaming quality adaptation
    #[test]
    fn test_streaming_quality() {
        // Test separate adapters to avoid averaging issues
        let mut adapter_high = QualityAdapter::new();
        // 15 Mbps = 15_000_000 bps > 10 Mbps threshold for High quality
        for i in 0..10 {
            adapter_high.update_bandwidth(15_000_000.0, i as f64);
        }
        assert_eq!(adapter_high.current_quality(), StreamingQuality::High);

        let mut adapter_low = QualityAdapter::new();
        // 1 Mbps = 1_000_000 bps < 2 Mbps threshold for Low quality
        for i in 0..10 {
            adapter_low.update_bandwidth(1_000_000.0, i as f64);
        }
        assert_eq!(adapter_low.current_quality(), StreamingQuality::Low);
    }
    /// Prefetch scheduling
    #[test]
    fn test_prefetch_scheduling() {
        let scheduler = PrefetchScheduler::new(10);
        let viewport = Viewport::new(50.0, 50.0, 0, 100, 100);
        let tiles = scheduler.schedule_prefetch(&viewport, 0);
        assert!(!tiles.is_empty());
    }
    /// Bandwidth estimation
    #[test]
    fn test_bandwidth_estimation() {
        let mut estimator = BandwidthEstimator::new();
        // Record downloads: bytes and time in milliseconds
        // 10KB in 100ms = 100KB/s, 20KB in 200ms = 100KB/s
        estimator.record_download(10000, 100.0);
        estimator.record_download(20000, 200.0);
        let bandwidth = estimator.estimate();
        // Average: 30KB in 300ms = 100KB/s = 100,000 bytes/sec
        assert!(bandwidth > 90_000.0 && bandwidth < 110_000.0);
    }
}
