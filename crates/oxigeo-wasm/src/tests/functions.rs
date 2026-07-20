//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::*;

#[cfg(test)]
mod unit_tests {
    use super::*;
    /// Tests for TileCoord functionality
    mod tile_coord_tests {
        use super::*;
        #[test]
        fn test_tile_coord_creation() {
            let coord = TileCoord::new(5, 10, 20);
            assert_eq!(coord.level, 5);
            assert_eq!(coord.x, 10);
            assert_eq!(coord.y, 20);
        }
        #[test]
        fn test_tile_coord_key() {
            let coord = TileCoord::new(5, 10, 20);
            assert_eq!(coord.key(), "5/10/20");
        }
        #[test]
        fn test_tile_coord_parent() {
            let coord = TileCoord::new(5, 10, 20);
            let parent = coord.parent().expect("Should have parent");
            assert_eq!(parent.level, 4);
            assert_eq!(parent.x, 5);
            assert_eq!(parent.y, 10);
            let root = TileCoord::new(0, 0, 0);
            assert!(root.parent().is_none());
        }
        #[test]
        fn test_tile_coord_children() {
            let coord = TileCoord::new(5, 10, 20);
            let children = coord.children();
            assert_eq!(children.len(), 4);
            assert_eq!(children[0], TileCoord::new(6, 20, 40));
            assert_eq!(children[1], TileCoord::new(6, 21, 40));
            assert_eq!(children[2], TileCoord::new(6, 20, 41));
            assert_eq!(children[3], TileCoord::new(6, 21, 41));
        }
        #[test]
        fn test_tile_coord_from_key() {
            let coord = TileCoord::new(5, 10, 20);
            let key = coord.key();
            let parsed = TileCoord::from_key(&key).expect("Should parse");
            assert_eq!(parsed, coord);
            assert!(TileCoord::from_key("invalid").is_none());
            assert!(TileCoord::from_key("1/2").is_none());
            assert!(TileCoord::from_key("a/b/c").is_none());
        }
        #[test]
        fn test_tile_coord_neighbors() {
            let coord = TileCoord::new(5, 10, 10);
            let neighbors = coord.neighbors();
            let valid_neighbors: Vec<_> = neighbors.iter().filter_map(|&n| n).collect();
            assert_eq!(valid_neighbors.len(), 8);
            let corner = TileCoord::new(0, 0, 0);
            let corner_neighbors = corner.neighbors();
            let valid_corner: Vec<_> = corner_neighbors.iter().filter_map(|&n| n).collect();
            assert_eq!(valid_corner.len(), 3);
        }
    }
    /// Tests for TileCache functionality
    mod tile_cache_tests {
        use super::*;
        #[test]
        fn test_cache_creation() {
            let cache = TileCache::new(1024);
            let stats = cache.stats();
            assert_eq!(stats.current_size, 0);
            assert_eq!(stats.max_size, 1024);
            assert_eq!(stats.entry_count, 0);
        }
        #[test]
        fn test_cache_put_get() {
            let mut cache = TileCache::new(1024);
            let coord = TileCoord::new(0, 0, 0);
            let data = vec![1, 2, 3, 4, 5];
            cache
                .put(coord, data.clone(), 0.0)
                .expect("Put should succeed");
            let retrieved = cache.get(&coord, 0.0).expect("Should find tile");
            assert_eq!(retrieved, data);
            let stats = cache.stats();
            assert_eq!(stats.hit_count, 1);
            assert_eq!(stats.miss_count, 0);
        }
        #[test]
        fn test_cache_miss() {
            let mut cache = TileCache::new(1024);
            let coord = TileCoord::new(0, 0, 0);
            let result = cache.get(&coord, 0.0);
            assert!(result.is_none());
            let stats = cache.stats();
            assert_eq!(stats.hit_count, 0);
            assert_eq!(stats.miss_count, 1);
        }
        #[test]
        fn test_cache_eviction() {
            // Cache size 14 bytes - can fit 2 tiles of 5 bytes (10 total) but not 3 (15 total)
            let mut cache = TileCache::new(14);
            let coord1 = TileCoord::new(0, 0, 0);
            let coord2 = TileCoord::new(0, 1, 0);
            let coord3 = TileCoord::new(0, 2, 0);
            cache.put(coord1, vec![1, 2, 3, 4, 5], 0.0).expect("Put 1");
            cache.put(coord2, vec![6, 7, 8, 9, 10], 1.0).expect("Put 2");
            cache
                .put(coord3, vec![11, 12, 13, 14, 15], 2.0)
                .expect("Put 3");
            // First tile should be evicted (LRU), keeping tiles 2 and 3
            assert!(!cache.contains(&coord1));
            assert!(cache.contains(&coord2));
            assert!(cache.contains(&coord3));
        }
        #[test]
        fn test_cache_hit_rate() {
            let mut cache = TileCache::new(1024);
            let coord = TileCoord::new(0, 0, 0);
            cache.put(coord, vec![1, 2, 3], 0.0).expect("Put");
            cache.get(&coord, 1.0);
            cache.get(&coord, 2.0);
            cache.get(&TileCoord::new(0, 1, 0), 3.0);
            let hit_rate = cache.hit_rate();
            assert!((hit_rate - 0.666).abs() < 0.01);
        }
        #[test]
        fn test_cache_clear() {
            let mut cache = TileCache::new(1024);
            let coord = TileCoord::new(0, 0, 0);
            cache.put(coord, vec![1, 2, 3], 0.0).expect("Put");
            assert_eq!(cache.stats().entry_count, 1);
            cache.clear();
            assert_eq!(cache.stats().entry_count, 0);
            assert_eq!(cache.stats().current_size, 0);
        }
    }
    /// Tests for TilePyramid functionality
    mod tile_pyramid_tests {
        use super::*;
        #[test]
        fn test_pyramid_creation() {
            let pyramid = TilePyramid::new(4096, 2048, 256, 256);
            assert_eq!(pyramid.width, 4096);
            assert_eq!(pyramid.height, 2048);
            assert_eq!(pyramid.tile_width, 256);
            assert_eq!(pyramid.tile_height, 256);
        }
        #[test]
        fn test_pyramid_tiles_at_level() {
            let pyramid = TilePyramid::new(4096, 2048, 256, 256);
            let (tiles_x, tiles_y) = pyramid.tiles_at_level(0).expect("Level 0");
            assert_eq!(tiles_x, 16);
            assert_eq!(tiles_y, 8);
            let (tiles_x1, tiles_y1) = pyramid.tiles_at_level(1).expect("Level 1");
            assert_eq!(tiles_x1, 8);
            assert_eq!(tiles_y1, 4);
        }
        #[test]
        fn test_pyramid_total_tiles() {
            let pyramid = TilePyramid::new(1024, 1024, 256, 256);
            let total = pyramid.total_tiles();
            assert!(total > 0);
            assert_eq!(total, 21);
        }
        #[test]
        fn test_pyramid_valid_coord() {
            let pyramid = TilePyramid::new(1024, 1024, 256, 256);
            assert!(pyramid.is_valid_coord(&TileCoord::new(0, 0, 0)));
            assert!(pyramid.is_valid_coord(&TileCoord::new(0, 3, 3)));
            assert!(!pyramid.is_valid_coord(&TileCoord::new(0, 4, 4)));
            assert!(!pyramid.is_valid_coord(&TileCoord::new(10, 0, 0)));
        }
    }
    /// Tests for ImageProcessor functionality
    mod image_processor_tests {
        use super::*;
        #[test]
        fn test_rgb_to_grayscale() {
            let rgb = Rgb::new(128, 128, 128);
            assert_eq!(rgb.to_gray(), 128);
            let black = Rgb::new(0, 0, 0);
            assert_eq!(black.to_gray(), 0);
            let white = Rgb::new(255, 255, 255);
            assert_eq!(white.to_gray(), 255);
        }
        #[test]
        fn test_rgb_to_hsv_to_rgb() {
            let original = Rgb::new(255, 0, 0);
            let hsv = original.to_hsv();
            let converted = hsv.to_rgb();
            assert_eq!(converted.r, original.r);
            assert!(converted.g < 5);
            assert!(converted.b < 5);
        }
        #[test]
        fn test_rgb_to_ycbcr_to_rgb() {
            let original = Rgb::new(128, 64, 192);
            let ycbcr = original.to_ycbcr();
            let converted = ycbcr.to_rgb();
            assert!((converted.r as i32 - original.r as i32).abs() <= 2);
            assert!((converted.g as i32 - original.g as i32).abs() <= 2);
            assert!((converted.b as i32 - original.b as i32).abs() <= 2);
        }
        #[test]
        fn test_brightness_adjustment() {
            let mut data = vec![100, 100, 100, 255];
            ImageProcessor::adjust_brightness(&mut data, 50);
            assert_eq!(data[0], 150);
            assert_eq!(data[1], 150);
            assert_eq!(data[2], 150);
            assert_eq!(data[3], 255);
        }
        #[test]
        fn test_brightness_clamping() {
            let mut data = vec![200, 200, 200, 255];
            ImageProcessor::adjust_brightness(&mut data, 100);
            assert_eq!(data[0], 255);
            assert_eq!(data[1], 255);
            assert_eq!(data[2], 255);
        }
        #[test]
        fn test_invert() {
            let mut data = vec![0, 128, 255, 255];
            ImageProcessor::invert(&mut data);
            assert_eq!(data[0], 255);
            assert_eq!(data[1], 127);
            assert_eq!(data[2], 0);
            assert_eq!(data[3], 255);
        }
        #[test]
        fn test_grayscale_conversion() {
            let mut data = vec![255, 0, 0, 255];
            ImageProcessor::to_grayscale(&mut data);
            assert_eq!(data[0], data[1]);
            assert_eq!(data[1], data[2]);
            assert_eq!(data[3], 255);
        }
    }
    /// Tests for Histogram functionality
    mod histogram_tests {
        use super::*;
        #[test]
        fn test_histogram_creation() {
            let data = vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 255,
            ];
            let hist = Histogram::from_rgba(&data, 2, 2).expect("Histogram");
            assert_eq!(hist.red[255], 1);
            assert_eq!(hist.green[255], 1);
            assert_eq!(hist.blue[255], 1);
            assert_eq!(hist.red[128], 1);
        }
        #[test]
        fn test_histogram_min_max() {
            let data = vec![
                50, 50, 50, 255, 100, 100, 100, 255, 150, 150, 150, 255, 200, 200, 200, 255,
            ];
            let hist = Histogram::from_rgba(&data, 2, 2).expect("Histogram");
            assert!(hist.min_value() >= 50);
            assert!(hist.max_value() <= 200);
        }
        #[test]
        fn test_histogram_mean() {
            let data = vec![0, 0, 0, 255, 255, 255, 255, 255];
            let hist = Histogram::from_rgba(&data, 2, 1).expect("Histogram");
            let mean = hist.mean();
            assert!(mean > 100.0 && mean < 150.0);
        }
    }
    /// Tests for Profiler functionality
    mod profiler_tests {
        use super::*;
        #[test]
        fn test_profiler_record() {
            let mut profiler = Profiler::new();
            profiler.record("test", 10.0);
            profiler.record("test", 20.0);
            profiler.record("test", 30.0);
            let stats = profiler.counter_stats("test").expect("Counter exists");
            assert_eq!(stats.count, 3);
            assert_eq!(stats.average_ms, 20.0);
        }
        #[test]
        fn test_profiler_timer() {
            let mut profiler = Profiler::new();
            profiler.start_timer("operation", 0.0);
            profiler.stop_timer("operation", 10.0);
            let stats = profiler.counter_stats("operation").expect("Counter exists");
            assert_eq!(stats.count, 1);
            assert_eq!(stats.total_time_ms, 10.0);
        }
        #[test]
        fn test_profiler_reset() {
            let mut profiler = Profiler::new();
            profiler.record("test", 10.0);
            assert!(profiler.counter_stats("test").is_some());
            profiler.reset();
            let summary = profiler.summary();
            assert!(summary.counters.is_empty());
        }
        #[test]
        fn test_memory_monitor() {
            let mut monitor = MemoryMonitor::new();
            monitor.record(MemorySnapshot::new(0.0, 1000));
            monitor.record(MemorySnapshot::new(1.0, 2000));
            monitor.record(MemorySnapshot::new(2.0, 1500));
            let stats = monitor.stats();
            assert_eq!(stats.current_heap_used, 1500);
            assert_eq!(stats.peak_heap_used, 2000);
            assert_eq!(stats.average_heap_used, 1500.0);
        }
    }
    /// Tests for Viewport functionality
    mod viewport_tests {
        use super::*;
        #[test]
        fn test_viewport_transform_identity() {
            let transform = ViewportTransform::identity();
            let (x, y) = transform.transform_point(10.0, 20.0);
            assert_eq!(x, 10.0);
            assert_eq!(y, 20.0);
        }
        #[test]
        fn test_viewport_transform_translate() {
            let transform = ViewportTransform::translate(5.0, 10.0);
            let (x, y) = transform.transform_point(0.0, 0.0);
            assert_eq!(x, 5.0);
            assert_eq!(y, 10.0);
        }
        #[test]
        fn test_viewport_transform_scale() {
            let transform = ViewportTransform::scale(2.0, 2.0);
            let (x, y) = transform.transform_point(10.0, 20.0);
            assert_eq!(x, 20.0);
            assert_eq!(y, 40.0);
        }
        #[test]
        fn test_viewport_transform_inverse() {
            let transform = ViewportTransform::translate(10.0, 20.0);
            let (x, y) = transform.inverse_transform_point(10.0, 20.0);
            assert!((x - 0.0).abs() < 0.001);
            assert!((y - 0.0).abs() < 0.001);
        }
        #[test]
        fn test_viewport_state_pan() {
            let mut state = ViewportState::new(800, 600);
            state.pan(10.0, 20.0);
            assert_eq!(state.transform.tx, 10.0);
            assert_eq!(state.transform.ty, 20.0);
        }
        #[test]
        fn test_viewport_history() {
            let mut history = ViewportHistory::new(10);
            let state1 = ViewportState::new(800, 600);
            let mut state2 = ViewportState::new(800, 600);
            state2.pan(10.0, 20.0);
            history.push(state1);
            history.push(state2);
            assert!(history.can_undo());
            history.undo();
            assert!(history.can_redo());
        }
    }
    /// Tests for CanvasBuffer functionality
    mod canvas_buffer_tests {
        use super::*;
        #[test]
        fn test_buffer_creation() {
            let buffer = CanvasBuffer::new(256, 256).expect("Create buffer");
            assert_eq!(buffer.dimensions(), (256, 256));
            assert!(buffer.is_dirty());
        }
        #[test]
        fn test_buffer_clear() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Create buffer");
            buffer.mark_clean();
            assert!(!buffer.is_dirty());
            buffer.clear();
            assert!(buffer.is_dirty());
        }
        #[test]
        fn test_buffer_clear_with_color() {
            let mut buffer = CanvasBuffer::new(2, 2).expect("Create buffer");
            buffer.clear_with_color(255, 0, 0, 255);
            let data = buffer.data();
            assert_eq!(data[0], 255);
            assert_eq!(data[1], 0);
            assert_eq!(data[2], 0);
            assert_eq!(data[3], 255);
        }
        #[test]
        fn test_buffer_resize() {
            let mut buffer = CanvasBuffer::new(256, 256).expect("Create buffer");
            buffer.resize(512, 512).expect("Resize");
            assert_eq!(buffer.dimensions(), (512, 512));
            assert!(buffer.is_dirty());
        }
    }
    /// Tests for FrameRateTracker functionality
    mod frame_rate_tests {
        use super::*;
        #[test]
        fn test_frame_rate_tracker() {
            let mut tracker = FrameRateTracker::new(60.0);
            for i in 0..120 {
                tracker.record_frame((i as f64) * 16.67);
            }
            let stats = tracker.stats();
            assert!(stats.current_fps > 55.0 && stats.current_fps < 65.0);
            assert!(!stats.is_below_target);
        }
        #[test]
        fn test_frame_rate_below_target() {
            let mut tracker = FrameRateTracker::new(60.0);
            for i in 0..60 {
                tracker.record_frame((i as f64) * 33.33);
            }
            let stats = tracker.stats();
            assert!(stats.is_below_target);
        }
    }
    /// Tests for BottleneckDetector functionality
    mod bottleneck_tests {
        use super::*;
        #[test]
        fn test_bottleneck_detection() {
            let mut detector = BottleneckDetector::new(10.0);
            detector.record("fast_op", 5.0);
            detector.record("slow_op", 50.0);
            detector.record("slow_op", 60.0);
            let bottlenecks = detector.detect_bottlenecks();
            assert_eq!(bottlenecks.len(), 1);
            assert_eq!(bottlenecks[0].operation, "slow_op");
        }
        #[test]
        fn test_bottleneck_severity() {
            let mut detector = BottleneckDetector::new(10.0);
            detector.record("critical", 100.0);
            detector.record("warning", 25.0);
            let bottlenecks = detector.detect_bottlenecks();
            assert!(bottlenecks[0].severity > bottlenecks[1].severity);
        }
        #[test]
        fn test_bottleneck_recommendations() {
            let mut detector = BottleneckDetector::new(10.0);
            detector.record("slow", 60.0);
            let recommendations = detector.recommendations();
            assert!(!recommendations.is_empty());
            assert!(recommendations[0].contains("slow"));
        }
    }
    /// Tests for TypeScript bindings
    mod bindings_tests {
        use super::*;
        #[test]
        fn test_ts_type_conversion() {
            assert_eq!(TsType::String.to_ts_string(), "string");
            assert_eq!(TsType::Number.to_ts_string(), "number");
            assert_eq!(TsType::Boolean.to_ts_string(), "boolean");
            assert_eq!(
                TsType::Array(Box::new(TsType::String)).to_ts_string(),
                "string[]"
            );
        }
        #[test]
        fn test_ts_parameter() {
            let param = TsParameter::new("name", TsType::String);
            assert_eq!(param.to_ts_string(), "name: string");
            let optional = TsParameter::new("age", TsType::Number).optional();
            assert_eq!(optional.to_ts_string(), "age?: number");
        }
        #[test]
        fn test_ts_function() {
            let func = TsFunction::new("greet", TsType::String)
                .parameter(TsParameter::new("name", TsType::String));
            let declaration = func.to_ts_declaration();
            assert!(declaration.contains("greet"));
            assert!(declaration.contains("name: string"));
        }
        #[test]
        fn test_ts_interface() {
            let interface = TsInterface::new("Person")
                .field("name", TsType::String)
                .field("age", TsType::Number);
            let declaration = interface.to_ts_declaration();
            assert!(declaration.contains("interface Person"));
            assert!(declaration.contains("name: string"));
            assert!(declaration.contains("age: number"));
        }
    }
}
