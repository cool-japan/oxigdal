//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

/// Edge case and stress tests
#[cfg(test)]
mod edge_case_tests {
    use crate::*;
    /// Test tile coordinate boundary conditions
    mod tile_coord_edge_cases {
        use super::*;
        #[test]
        fn test_max_zoom_level() {
            let coord = TileCoord::new(20, 1_000_000, 1_000_000);
            assert_eq!(coord.level, 20);
            assert_eq!(coord.key(), "20/1000000/1000000");
        }
        #[test]
        fn test_zero_coordinates() {
            let coord = TileCoord::new(0, 0, 0);
            assert_eq!(coord.key(), "0/0/0");
            assert!(coord.parent().is_none());
        }
        #[test]
        fn test_large_coordinates() {
            let coord = TileCoord::new(15, 32767, 32767);
            let parent = coord.parent().expect("Should have parent");
            assert_eq!(parent.x, 16383);
            assert_eq!(parent.y, 16383);
        }
        #[test]
        fn test_coordinate_overflow_protection() {
            let coord = TileCoord::new(30, u32::MAX / 2, u32::MAX / 2);
            let children = coord.children();
            assert_eq!(children.len(), 4);
        }
        #[test]
        fn test_neighbor_edge_tiles() {
            let edge = TileCoord::new(5, 0, 0);
            let neighbors = edge.neighbors();
            let valid: Vec<_> = neighbors.iter().filter_map(|&n| n).collect();
            assert!(valid.len() < 8);
        }
        #[test]
        fn test_key_parsing_edge_cases() {
            assert!(TileCoord::from_key("").is_none());
            assert!(TileCoord::from_key("/").is_none());
            assert!(TileCoord::from_key("//").is_none());
            assert!(TileCoord::from_key("1/2/").is_none());
            assert!(TileCoord::from_key("-1/0/0").is_none());
        }
        #[test]
        fn test_deep_hierarchy() {
            let mut coord = TileCoord::new(10, 512, 512);
            let mut depth = 0;
            while let Some(parent) = coord.parent() {
                coord = parent;
                depth += 1;
            }
            assert_eq!(depth, 10);
            assert_eq!(coord, TileCoord::new(0, 0, 0));
        }
    }
    /// Test cache boundary conditions
    mod cache_edge_cases {
        use super::*;
        #[test]
        fn test_zero_capacity_cache() {
            let mut cache = TileCache::new(0);
            let coord = TileCoord::new(0, 0, 0);
            let result = cache.put(coord, vec![1, 2, 3], 0.0);
            assert!(result.is_err());
        }
        #[test]
        fn test_single_byte_capacity() {
            let mut cache = TileCache::new(1);
            let coord = TileCoord::new(0, 0, 0);
            let _result = cache.put(coord, vec![1], 0.0);
        }
        #[test]
        fn test_rapid_eviction() {
            let mut cache = TileCache::new(100);
            for i in 0..1000 {
                let coord = TileCoord::new(0, i, 0);
                let _ = cache.put(coord, vec![1, 2, 3, 4, 5], i as f64);
            }
            let stats = cache.stats();
            assert!(stats.entry_count < 100);
        }
        #[test]
        fn test_concurrent_access_pattern() {
            let mut cache = TileCache::new(1000);
            let coord = TileCoord::new(0, 0, 0);
            cache.put(coord, vec![1, 2, 3], 0.0).expect("Put");
            for i in 0..1000 {
                cache.get(&coord, i as f64);
            }
            let stats = cache.stats();
            assert_eq!(stats.hit_count, 1000);
        }
        #[test]
        fn test_fragmentation_scenario() {
            let mut cache = TileCache::new(1000);
            for i in 0..10 {
                let coord = TileCoord::new(0, i, 0);
                let size = (i + 1) * 10;
                let data = vec![0u8; size as usize];
                let _ = cache.put(coord, data, i as f64);
            }
            cache.clear();
            assert_eq!(cache.stats().current_size, 0);
        }
        #[test]
        fn test_empty_tile_data() {
            let mut cache = TileCache::new(1000);
            let coord = TileCoord::new(0, 0, 0);
            cache.put(coord, vec![], 0.0).expect("Put empty");
            let retrieved = cache.get(&coord, 1.0);
            assert!(retrieved.is_some());
            assert_eq!(retrieved.expect("Retrieved").len(), 0);
        }
        #[test]
        fn test_large_tile_rejection() {
            let mut cache = TileCache::new(100);
            let coord = TileCoord::new(0, 0, 0);
            let huge_tile = vec![0u8; 1000];
            let result = cache.put(coord, huge_tile, 0.0);
            assert!(result.is_err());
        }
        #[test]
        fn test_duplicate_puts() {
            let mut cache = TileCache::new(1000);
            let coord = TileCoord::new(0, 0, 0);
            cache.put(coord, vec![1, 2, 3], 0.0).expect("First put");
            cache.put(coord, vec![4, 5, 6], 1.0).expect("Second put");
            let data = cache.get(&coord, 2.0).expect("Should exist");
            assert_eq!(data, vec![4, 5, 6]);
        }
    }
    /// Test pyramid edge cases
    mod pyramid_edge_cases {
        use super::*;
        #[test]
        fn test_non_power_of_two_dimensions() {
            let pyramid = TilePyramid::new(1920, 1080, 256, 256);
            let (tiles_x, tiles_y) = pyramid.tiles_at_level(0).expect("Level 0");
            assert_eq!(tiles_x, 8);
            assert_eq!(tiles_y, 5);
        }
        #[test]
        fn test_single_tile_pyramid() {
            let pyramid = TilePyramid::new(256, 256, 256, 256);
            let total = pyramid.total_tiles();
            assert_eq!(total, 1);
        }
        #[test]
        fn test_very_large_pyramid() {
            let pyramid = TilePyramid::new(65536, 65536, 256, 256);
            let (tiles_x, tiles_y) = pyramid.tiles_at_level(0).expect("Level 0");
            assert_eq!(tiles_x, 256);
            assert_eq!(tiles_y, 256);
        }
        #[test]
        fn test_asymmetric_tiles() {
            let pyramid = TilePyramid::new(4096, 2048, 512, 256);
            let (tiles_x, tiles_y) = pyramid.tiles_at_level(0).expect("Level 0");
            assert_eq!(tiles_x, 8);
            assert_eq!(tiles_y, 8);
        }
        #[test]
        fn test_invalid_level_access() {
            let pyramid = TilePyramid::new(1024, 1024, 256, 256);
            assert!(pyramid.tiles_at_level(100).is_none());
        }
        #[test]
        fn test_boundary_tile_coordinates() {
            let pyramid = TilePyramid::new(1024, 1024, 256, 256);
            let (tiles_x, tiles_y) = pyramid.tiles_at_level(0).expect("Level 0");
            assert!(pyramid.is_valid_coord(&TileCoord::new(0, 0, 0)));
            assert!(pyramid.is_valid_coord(&TileCoord::new(0, tiles_x - 1, tiles_y - 1)));
            assert!(!pyramid.is_valid_coord(&TileCoord::new(0, tiles_x, tiles_y)));
        }
    }
    /// Test image processing edge cases
    mod image_processing_edge_cases {
        use super::*;
        #[test]
        fn test_empty_image_data() {
            let data = vec![];
            let hist = Histogram::from_rgba(&data, 0, 0);
            assert!(hist.is_err());
        }
        #[test]
        fn test_single_pixel() {
            let mut data = vec![128, 128, 128, 255];
            ImageProcessor::adjust_brightness(&mut data, 100);
            assert_eq!(data[0], 228);
        }
        #[test]
        fn test_extreme_brightness() {
            let mut data = vec![100, 100, 100, 255];
            ImageProcessor::adjust_brightness(&mut data, 200);
            assert_eq!(data[0], 255);
        }
        #[test]
        fn test_negative_brightness() {
            let mut data = vec![100, 100, 100, 255];
            ImageProcessor::adjust_brightness(&mut data, -150);
            assert_eq!(data[0], 0);
        }
        #[test]
        fn test_contrast_extremes() {
            let mut data = vec![128, 128, 128, 255];
            ImageProcessor::adjust_contrast(&mut data, 10.0);
        }
        #[test]
        fn test_zero_contrast() {
            let mut data = vec![100, 150, 200, 255];
            ImageProcessor::adjust_contrast(&mut data, 0.0);
            assert_eq!(data[0], data[1]);
            assert_eq!(data[1], data[2]);
        }
        #[test]
        fn test_invalid_image_dimensions() {
            let data = vec![255, 0, 0, 255, 0, 255, 0];
            let hist = Histogram::from_rgba(&data, 2, 1);
            assert!(hist.is_err());
        }
        #[test]
        fn test_color_conversion_roundtrip() {
            let original = Rgb::new(123, 45, 67);
            let hsv = original.to_hsv();
            let back_to_rgb = hsv.to_rgb();
            assert!((back_to_rgb.r as i32 - original.r as i32).abs() <= 2);
            assert!((back_to_rgb.g as i32 - original.g as i32).abs() <= 2);
            assert!((back_to_rgb.b as i32 - original.b as i32).abs() <= 2);
        }
        #[test]
        fn test_grayscale_edge_values() {
            assert_eq!(Rgb::new(0, 0, 0).to_gray(), 0);
            assert_eq!(Rgb::new(255, 255, 255).to_gray(), 255);
        }
        #[test]
        fn test_all_black_histogram() {
            let data = [0u8, 0, 0, 255].repeat(100);
            let hist = Histogram::from_rgba(&data, 10, 10).expect("Histogram");
            assert_eq!(hist.min_value(), 0);
            assert_eq!(hist.max_value(), 0);
        }
        #[test]
        fn test_all_white_histogram() {
            let data = [255u8, 255, 255, 255].repeat(100);
            let hist = Histogram::from_rgba(&data, 10, 10).expect("Histogram");
            assert_eq!(hist.min_value(), 255);
            assert_eq!(hist.max_value(), 255);
        }
    }
    /// Test profiler edge cases
    mod profiler_edge_cases {
        use super::*;
        #[test]
        fn test_zero_duration_records() {
            let mut profiler = Profiler::new();
            profiler.record("instant", 0.0);
            profiler.record("instant", 0.0);
            let stats = profiler.counter_stats("instant").expect("Stats");
            assert_eq!(stats.average_ms, 0.0);
        }
        #[test]
        fn test_very_long_operation() {
            let mut profiler = Profiler::new();
            profiler.start_timer("long", 0.0);
            profiler.stop_timer("long", 10000.0);
            let stats = profiler.counter_stats("long").expect("Stats");
            assert_eq!(stats.total_time_ms, 10000.0);
        }
        #[test]
        fn test_nested_timers_same_name() {
            let mut profiler = Profiler::new();
            profiler.start_timer("op", 0.0);
            profiler.start_timer("op", 5.0);
            profiler.stop_timer("op", 10.0);
            let stats = profiler.counter_stats("op");
            assert!(stats.is_some());
        }
        #[test]
        fn test_stop_without_start() {
            let mut profiler = Profiler::new();
            profiler.stop_timer("never_started", 10.0);
            let summary = profiler.summary();
            assert!(summary.counters.is_empty());
        }
        #[test]
        fn test_many_counters() {
            let mut profiler = Profiler::new();
            for i in 0..1000 {
                let name = format!("op_{}", i);
                profiler.record(&name, i as f64);
            }
            let summary = profiler.summary();
            assert_eq!(summary.counters.len(), 1000);
        }
        #[test]
        fn test_percentile_calculation() {
            let mut profiler = Profiler::new();
            for i in 0..100 {
                profiler.record("test", i as f64);
            }
            let stats = profiler.counter_stats("test").expect("Stats");
            assert!(stats.p50_ms > 40.0 && stats.p50_ms < 60.0);
            assert!(stats.p95_ms > 90.0 && stats.p95_ms < 100.0);
        }
        #[test]
        fn test_memory_monitor_decreasing() {
            let mut monitor = MemoryMonitor::new();
            monitor.record(MemorySnapshot::new(0.0, 1000));
            monitor.record(MemorySnapshot::new(1.0, 500));
            let stats = monitor.stats();
            assert_eq!(stats.current_heap_used, 500);
            assert_eq!(stats.peak_heap_used, 1000);
        }
        #[test]
        fn test_memory_leak_detection() {
            let mut monitor = MemoryMonitor::new();
            for i in 0..100 {
                monitor.record(MemorySnapshot::new(i as f64, 1000 + i * 10));
            }
            let stats = monitor.stats();
            assert!(stats.peak_heap_used as f64 > stats.average_heap_used);
        }
    }
    /// Test viewport edge cases
    mod viewport_edge_cases {
        use super::*;
        #[test]
        fn test_zero_dimension_viewport() {
            let state = ViewportState::new(0, 0);
            assert_eq!(state.canvas_width, 0);
            assert_eq!(state.canvas_height, 0);
        }
        #[test]
        fn test_extreme_zoom() {
            let mut state = ViewportState::new(800, 600);
            state.zoom(1000.0, 400.0, 300.0);
            assert_eq!(state.transform.sx, 1000.0);
        }
        #[test]
        fn test_extreme_pan() {
            let mut state = ViewportState::new(800, 600);
            state.pan(1_000_000.0, 1_000_000.0);
            assert_eq!(state.transform.tx, 1_000_000.0);
        }
        #[test]
        fn test_negative_zoom() {
            let mut state = ViewportState::new(800, 600);
            state.zoom(-1.0, 400.0, 300.0);
            assert!(state.transform.sx > 0.0);
        }
        #[test]
        fn test_history_capacity_limit() {
            let mut history = ViewportHistory::new(5);
            for i in 0..10 {
                let mut state = ViewportState::new(800, 600);
                state.pan(i as f64 * 10.0, 0.0);
                history.push(state);
            }
            assert!(history.current_index() <= 5);
        }
        #[test]
        fn test_undo_redo_boundary() {
            let mut history = ViewportHistory::new(5);
            let state = ViewportState::new(800, 600);
            history.push(state);
            while history.can_undo() {
                history.undo();
            }
            history.undo();
            while history.can_redo() {
                history.redo();
            }
            history.redo();
        }
        #[test]
        fn test_transform_composition() {
            let t1 = ViewportTransform::translate(10.0, 20.0);
            let t2 = ViewportTransform::scale(2.0, 2.0);
            let composed = t1.compose(&t2);
            let (x, y) = composed.transform_point(5.0, 5.0);
            assert!((x - 20.0).abs() < 0.001);
            assert!((y - 30.0).abs() < 0.001);
        }
        #[test]
        fn test_inverse_transform_accuracy() {
            let transform = ViewportTransform::new(2.0, 0.0, 0.0, 2.0, 10.0, 20.0);
            let (x, y) = transform.transform_point(5.0, 10.0);
            let (ix, iy) = transform.inverse_transform_point(x, y);
            assert!((ix - 5.0).abs() < 0.001);
            assert!((iy - 10.0).abs() < 0.001);
        }
    }
    /// Test buffer edge cases
    mod buffer_edge_cases {
        use super::*;
        #[test]
        fn test_zero_size_buffer() {
            let result = CanvasBuffer::new(0, 0);
            assert!(result.is_err());
        }
        #[test]
        fn test_extremely_large_buffer() {
            let _result = CanvasBuffer::new(100000, 100000);
        }
        #[test]
        fn test_composite_out_of_bounds() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Buffer");
            let tile = vec![255u8; 256 * 256 * 4];
            let _result = buffer.composite_tile(&tile, 256, 256, 200, 200, 1.0);
        }
        #[test]
        fn test_alpha_blending_extremes() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Buffer");
            buffer.clear_with_color(0, 0, 0, 255);
            let tile = vec![255u8; 256 * 256 * 4];
            buffer
                .composite_tile(&tile, 256, 256, 0, 0, 0.0)
                .expect("Alpha 0");
            buffer
                .composite_tile(&tile, 256, 256, 0, 0, 0.5)
                .expect("Alpha 0.5");
            buffer
                .composite_tile(&tile, 256, 256, 0, 0, 1.0)
                .expect("Alpha 1");
        }
        #[test]
        fn test_buffer_resize_to_zero() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Buffer");
            let result = buffer.resize(0, 0);
            assert!(result.is_err());
        }
        #[test]
        fn test_multiple_resizes() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Buffer");
            buffer.resize(512, 512).expect("Resize 1");
            assert_eq!(buffer.dimensions(), (512, 512));
            buffer.resize(128, 128).expect("Resize 2");
            assert_eq!(buffer.dimensions(), (128, 128));
            buffer.resize(1024, 1024).expect("Resize 3");
            assert_eq!(buffer.dimensions(), (1024, 1024));
        }
        #[test]
        fn test_buffer_clear_preserves_size() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Buffer");
            let (w, h) = buffer.dimensions();
            buffer.clear();
            assert_eq!(buffer.dimensions(), (w, h));
        }
    }
    /// Test frame rate tracking edge cases
    mod frame_rate_edge_cases {
        use super::*;
        #[test]
        fn test_zero_target_fps() {
            let _tracker = FrameRateTracker::new(0.0);
        }
        #[test]
        fn test_very_high_fps() {
            let mut tracker = FrameRateTracker::new(240.0);
            for i in 0..480 {
                tracker.record_frame((i as f64) * (1000.0 / 240.0));
            }
            let stats = tracker.stats();
            assert!(stats.current_fps > 200.0);
        }
        #[test]
        fn test_inconsistent_frame_times() {
            let mut tracker = FrameRateTracker::new(60.0);
            tracker.record_frame(0.0);
            tracker.record_frame(16.67);
            tracker.record_frame(50.0);
            tracker.record_frame(66.67);
            let _stats = tracker.stats();
        }
        #[test]
        fn test_backwards_time() {
            let mut tracker = FrameRateTracker::new(60.0);
            tracker.record_frame(100.0);
            tracker.record_frame(50.0);
            let _stats = tracker.stats();
        }
    }
    /// Test compression edge cases
    mod compression_edge_cases {
        use super::*;
        #[test]
        fn test_compress_empty_data() {
            let data = vec![];
            let compressed = RleCompressor::compress(&data);
            let decompressed = RleCompressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_compress_single_byte() {
            let data = vec![42];
            let compressed = RleCompressor::compress(&data);
            let decompressed = RleCompressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_compress_all_same() {
            let data = vec![123; 1000];
            let compressed = RleCompressor::compress(&data);
            assert!(compressed.len() < 50);
            let decompressed = RleCompressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_compress_all_different() {
            let data: Vec<u8> = (0..=255).collect();
            let compressed = RleCompressor::compress(&data);
            let decompressed = RleCompressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_delta_compression_constant() {
            let data = vec![100; 100];
            let compressed = DeltaCompressor::compress(&data);
            let decompressed = DeltaCompressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_delta_compression_linear() {
            let data: Vec<u8> = (0..100).collect();
            let compressed = DeltaCompressor::compress(&data);
            let decompressed = DeltaCompressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_huffman_single_symbol() {
            let data = vec![42; 100];
            let compressed = HuffmanCompressor::compress(&data);
            // Note: Huffman decompression is not yet implemented
            // TODO: Implement full Huffman codec or use existing library
            assert!(!compressed.is_empty(), "Compression should produce output");
        }
        #[test]
        fn test_lz77_no_matches() {
            let data: Vec<u8> = (0..=255).cycle().take(1000).collect();
            let compressed = Lz77Compressor::compress(&data);
            let decompressed = Lz77Compressor::decompress(&compressed).expect("Decompress");
            assert_eq!(decompressed, data);
        }
        #[test]
        fn test_compression_algorithm_selection() {
            let mut selector = CompressionSelector::new();
            let random_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
            let best = selector.select_best(&random_data).expect("Select");
            assert!(matches!(
                best,
                CompressionAlgorithm::Rle
                    | CompressionAlgorithm::Delta
                    | CompressionAlgorithm::Huffman
                    | CompressionAlgorithm::Lz77
            ));
        }
    }
    /// Test streaming edge cases
    mod streaming_edge_cases {
        use super::*;
        #[test]
        fn test_quality_adapter_zero_bandwidth() {
            let mut adapter = QualityAdapter::new();
            // Need 3 consistent updates to overcome hysteresis
            adapter.update_bandwidth(0.0, 0.0);
            adapter.update_bandwidth(0.0, 1.0);
            adapter.update_bandwidth(0.0, 2.0);
            assert_eq!(adapter.current_quality(), StreamingQuality::Low);
        }
        #[test]
        fn test_quality_adapter_infinite_bandwidth() {
            let mut adapter = QualityAdapter::new();
            // Need 3 consistent updates to overcome hysteresis
            adapter.update_bandwidth(f64::MAX, 0.0);
            adapter.update_bandwidth(f64::MAX, 1.0);
            adapter.update_bandwidth(f64::MAX, 2.0);
            assert_eq!(adapter.current_quality(), StreamingQuality::High);
        }
        #[test]
        fn test_progressive_loader_empty_tiles() {
            let loader = ProgressiveLoader::new();
            let tiles = vec![];
            let ordered = loader.prioritize_tiles(&tiles);
            assert!(ordered.is_empty());
        }
        #[test]
        fn test_bandwidth_estimator_single_sample() {
            let mut estimator = BandwidthEstimator::new();
            estimator.record_download(1000, 0.1);
            let bandwidth = estimator.estimate();
            assert!(bandwidth > 0.0);
        }
        #[test]
        fn test_bandwidth_estimator_zero_duration() {
            let mut estimator = BandwidthEstimator::new();
            estimator.record_download(1000, 0.0);
            let _bandwidth = estimator.estimate();
        }
        #[test]
        fn test_prefetch_scheduler_empty_viewport() {
            let scheduler = PrefetchScheduler::new(10);
            let viewport = Viewport::new(0.0, 0.0, 0, 0, 0);
            let _tiles = scheduler.schedule_prefetch(&viewport, 0);
        }
        #[test]
        fn test_prefetch_scheduler_huge_viewport() {
            let scheduler = PrefetchScheduler::new(10);
            let viewport = Viewport::new(500_000.0, 500_000.0, 0, 1_000_000, 1_000_000);
            let tiles = scheduler.schedule_prefetch(&viewport, 0);
            assert!(tiles.len() <= 1000);
        }
        #[test]
        fn test_adaptive_quality_oscillation() {
            let mut adapter = QualityAdapter::new();
            for i in 0..100 {
                let bw = if i % 2 == 0 { 5_000_000.0 } else { 100_000.0 };
                adapter.update_bandwidth(bw, i as f64);
            }
            let _quality = adapter.current_quality();
        }
    }
    /// Test TypeScript bindings edge cases
    mod bindings_edge_cases {
        use super::*;
        #[test]
        fn test_nested_types() {
            let nested = TsType::Array(Box::new(TsType::Array(Box::new(TsType::Number))));
            assert_eq!(nested.to_ts_string(), "number[][]");
        }
        #[test]
        fn test_complex_interface() {
            let interface = TsInterface::new("Complex")
                .field(
                    "nested",
                    TsType::Array(Box::new(TsType::Reference("Inner".to_string()))),
                )
                .field("union", TsType::Union(vec![TsType::String, TsType::Number]))
                .field("optional", TsType::Void);
            let decl = interface.to_ts_declaration();
            assert!(decl.contains("nested"));
            assert!(decl.contains("optional"));
        }
        #[test]
        fn test_function_with_many_parameters() {
            let mut func = TsFunction::new("complex", TsType::Void);
            for i in 0..20 {
                func = func.parameter(TsParameter::new(format!("param{}", i), TsType::Number));
            }
            let decl = func.to_ts_declaration();
            assert!(decl.contains("param0"));
            assert!(decl.contains("param19"));
        }
        #[test]
        fn test_empty_interface() {
            let interface = TsInterface::new("Empty");
            let decl = interface.to_ts_declaration();
            assert!(decl.contains("interface Empty"));
        }
        #[test]
        fn test_promise_type() {
            let promise = TsType::Promise(Box::new(TsType::String));
            assert_eq!(promise.to_ts_string(), "Promise<string>");
        }
        #[test]
        fn test_tuple_type() {
            let tuple = TsType::Tuple(vec![TsType::String, TsType::Number, TsType::Boolean]);
            assert_eq!(tuple.to_ts_string(), "[string, number, boolean]");
        }
    }
}
