//! Tests for the streaming chunked raster abstraction.
//!
//! All tests use `InMemoryRasterSource` as the raster source backend.
//! The streaming layer is exercised through the public re-exports from
//! `oxigdal_algorithms`.

use oxigdal_algorithms::{
    ChunkedRaster, InMemoryRasterSource, RasterError, RasterSource, process_streaming,
    streaming_focal_mean, streaming_hillshade, streaming_slope,
};

// ============================================================================
// Test helpers
// ============================================================================

/// Build a flat ramp: data[y * w + x] = y * w + x  (i.e. 0, 1, 2, …).
fn make_ramp(width: usize, height: usize) -> Vec<f32> {
    (0..width * height).map(|i| i as f32).collect()
}

/// Build a simple DEM with a Gaussian-like hill centred in the raster.
fn make_hill(width: usize, height: usize) -> Vec<f32> {
    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;
    (0..width * height)
        .map(|i| {
            let x = (i % width) as f64;
            let y = (i / width) as f64;
            let r2 = (x - cx) * (x - cx) + (y - cy) * (y - cy);
            (100.0 * (-r2 / 20.0_f64).exp()) as f32
        })
        .collect()
}

// ============================================================================
// 1. InMemoryRasterSource: read_window full extent
// ============================================================================

#[test]
fn test_inmemory_source_read_window_full_extent() {
    let data: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let src = InMemoryRasterSource::new(data.clone(), 4, 4);

    let window = src.read_window(0, 0, 4, 4).expect("full extent read");
    assert_eq!(window.len(), 16);
    for (a, b) in window.iter().zip(data.iter()) {
        assert!((a - b).abs() < 1e-6, "mismatch: got {a} expected {b}");
    }
}

// ============================================================================
// 2. InMemoryRasterSource: partial (centre) window
// ============================================================================

#[test]
fn test_inmemory_source_read_window_partial() {
    let data: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let src = InMemoryRasterSource::new(data, 4, 4);

    // Read the 2×2 block at (1,1): pixels (1,1),(2,1),(1,2),(2,2)
    let window = src.read_window(1, 1, 2, 2).expect("partial read");
    assert_eq!(window.len(), 4);
    // Row-major: row 1 cols 1..2, then row 2 cols 1..2
    // indices: 1*4+1=5, 1*4+2=6, 2*4+1=9, 2*4+2=10
    assert!((window[0] - 5.0).abs() < 1e-6);
    assert!((window[1] - 6.0).abs() < 1e-6);
    assert!((window[2] - 9.0).abs() < 1e-6);
    assert!((window[3] - 10.0).abs() < 1e-6);
}

// ============================================================================
// 3. InMemoryRasterSource: origin outside raster → OutOfBounds
// ============================================================================

#[test]
fn test_inmemory_source_out_of_bounds_returns_error() {
    let src = InMemoryRasterSource::new(vec![1.0_f32; 16], 4, 4);

    let result = src.read_window(4, 0, 2, 2); // x == width
    assert_eq!(result, Err(RasterError::OutOfBounds));

    let result2 = src.read_window(0, 4, 2, 2); // y == height
    assert_eq!(result2, Err(RasterError::OutOfBounds));

    let result3 = src.read_window(10, 10, 2, 2); // both out
    assert_eq!(result3, Err(RasterError::OutOfBounds));
}

// ============================================================================
// 4. ChunkedRaster iterator: expected chunk count  (10×10, chunk_size=4, halo=0)
// ============================================================================

#[test]
fn test_chunked_raster_iter_yields_expected_chunk_count() {
    let src = InMemoryRasterSource::new(vec![1.0_f32; 100], 10, 10);
    let cr = ChunkedRaster::new(src, 4, 0).expect("create ChunkedRaster");

    // ceil(10/4) = 3 in each dimension → 3×3 = 9 chunks
    let count = cr.iter().count();
    assert_eq!(
        count, 9,
        "expected 9 chunks for 10×10 raster with chunk_size=4"
    );
}

// ============================================================================
// 5. Each chunk.data.len() == (width + 2*halo) × (height + 2*halo)
// ============================================================================

#[test]
fn test_chunked_raster_iter_chunk_dimensions_with_halo() {
    let src = InMemoryRasterSource::new(vec![1.0_f32; 100], 10, 10);
    let halo: usize = 2;
    let cr = ChunkedRaster::new(src, 4, halo).expect("create ChunkedRaster");

    for chunk_result in cr.iter() {
        let chunk = chunk_result.expect("chunk ok");
        let expected_data_len = (chunk.width + 2 * halo) * (chunk.height + 2 * halo);
        assert_eq!(
            chunk.data.len(),
            expected_data_len,
            "chunk ({},{}) size={} h={}: data.len() should be {}",
            chunk.x,
            chunk.y,
            chunk.width,
            chunk.height,
            expected_data_len
        );
        assert_eq!(chunk.halo, halo, "halo stored in chunk should match");
    }
}

// ============================================================================
// 6. Edge chunk at (0,0) with halo=1: top-left halo cell is 0.0 (zero-padded)
// ============================================================================

#[test]
fn test_chunked_raster_iter_edge_chunks_have_zero_halo_padding() {
    // 8×8 raster, all values = 1.0; with halo=1 the top-left halo cell should
    // be 0.0 because it falls outside the raster at (-1,-1).
    let src = InMemoryRasterSource::new(vec![1.0_f32; 64], 8, 8);
    let halo: usize = 1;
    let cr = ChunkedRaster::new(src, 4, halo).expect("create ChunkedRaster");

    // The very first chunk emitted is the top-left corner chunk at (0,0).
    let first_chunk = cr
        .iter()
        .next()
        .expect("at least one chunk")
        .expect("chunk ok");
    assert_eq!(first_chunk.x, 0);
    assert_eq!(first_chunk.y, 0);

    // In the data array (full_w × full_h), position (0,0) is the top-left
    // halo cell which maps to source (-1, -1) → zero-padded.
    let top_left_halo = first_chunk.data[0];
    assert!(
        (top_left_halo - 0.0_f32).abs() < 1e-6,
        "top-left halo cell should be zero-padded, got {top_left_halo}"
    );
}

// ============================================================================
// 7. process_streaming with identity kernel reproduces source data
// ============================================================================

#[test]
fn test_process_streaming_identity_returns_original_data() {
    let data: Vec<f32> = make_ramp(6, 6);
    let src = InMemoryRasterSource::new(data.clone(), 6, 6);
    let cr = ChunkedRaster::new(src, 3, 0).expect("create ChunkedRaster");

    // Identity kernel: return the core region from chunk.data (no halo here)
    let result = process_streaming(&cr, |chunk| {
        let mut core = Vec::with_capacity(chunk.width * chunk.height);
        for row in 0..chunk.height {
            for col in 0..chunk.width {
                core.push(chunk.get_core(col, row));
            }
        }
        Ok(core)
    })
    .expect("process_streaming ok");

    assert_eq!(result.len(), data.len());
    for (i, (got, expected)) in result.iter().zip(data.iter()).enumerate() {
        assert!(
            (got - expected).abs() < 1e-5,
            "pixel {i}: got {got} expected {expected}"
        );
    }
}

// ============================================================================
// 8. process_streaming with ×2 kernel doubles all values
// ============================================================================

#[test]
fn test_process_streaming_constant_kernel_matches_full_compute() {
    let data: Vec<f32> = make_ramp(8, 8);
    let expected: Vec<f32> = data.iter().map(|v| v * 2.0).collect();

    let src = InMemoryRasterSource::new(data, 8, 8);
    let cr = ChunkedRaster::new(src, 4, 0).expect("create ChunkedRaster");

    let result = process_streaming(&cr, |chunk| {
        let mut out = Vec::with_capacity(chunk.width * chunk.height);
        for row in 0..chunk.height {
            for col in 0..chunk.width {
                out.push(chunk.get_core(col, row) * 2.0);
            }
        }
        Ok(out)
    })
    .expect("process_streaming ok");

    assert_eq!(result.len(), expected.len());
    for (i, (got, exp)) in result.iter().zip(expected.iter()).enumerate() {
        assert!((got - exp).abs() < 1e-5, "pixel {i}: got {got} exp {exp}");
    }
}

// ============================================================================
// 9. streaming_hillshade vs full hillshade on a single-chunk raster
// ============================================================================

#[test]
fn test_streaming_hillshade_matches_full_hillshade_within_tolerance() {
    use oxigdal_algorithms::raster::{HillshadeParams, hillshade as full_hillshade};
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    let w = 8_usize;
    let h = 8_usize;
    let data = make_hill(w, h);

    // Build the reference result using the full (non-streaming) hillshade.
    let dem = RasterBuffer::from_typed_vec(w, h, data.clone(), RasterDataType::Float32)
        .expect("build RasterBuffer");
    let params = HillshadeParams::new(315.0, 45.0);
    let ref_buf = full_hillshade(&dem, params).expect("full hillshade");
    let ref_data: Vec<f32> = ref_buf.as_slice::<f32>().expect("as_slice").to_vec();

    // Streaming version: chunk_size=8 (fits in one chunk) with halo=1.
    let src = InMemoryRasterSource::new(data, w, h);
    let cr = ChunkedRaster::new(src, 8, 1).expect("create ChunkedRaster");
    let stream_data = streaming_hillshade(&cr, 315.0, 45.0).expect("streaming hillshade");

    assert_eq!(stream_data.len(), ref_data.len());

    // Compare interior pixels only (skip 1-pixel border where edge handling differs).
    let mut max_diff = 0.0_f32;
    for row in 1..(h - 1) {
        for col in 1..(w - 1) {
            let idx = row * w + col;
            let diff = (stream_data[idx] - ref_data[idx]).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }
    }
    assert!(
        max_diff < 1e-3,
        "streaming hillshade interior max diff {max_diff} exceeds tolerance 1e-3"
    );
}

// ============================================================================
// 10. streaming_slope vs full slope on a single-chunk raster
// ============================================================================

#[test]
fn test_streaming_slope_matches_full_slope_within_tolerance() {
    use oxigdal_algorithms::raster::slope as full_slope;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    let w = 8_usize;
    let h = 8_usize;
    let data = make_hill(w, h);

    let dem = RasterBuffer::from_typed_vec(w, h, data.clone(), RasterDataType::Float32)
        .expect("build RasterBuffer");
    let ref_buf = full_slope(&dem, 1.0, 1.0).expect("full slope");
    let ref_data: Vec<f32> = ref_buf.as_slice::<f32>().expect("as_slice").to_vec();

    let src = InMemoryRasterSource::new(data, w, h);
    let cr = ChunkedRaster::new(src, 8, 1).expect("create ChunkedRaster");
    let stream_data = streaming_slope(&cr).expect("streaming slope");

    assert_eq!(stream_data.len(), ref_data.len());

    let mut max_diff = 0.0_f32;
    for row in 1..(h - 1) {
        for col in 1..(w - 1) {
            let idx = row * w + col;
            let diff = (stream_data[idx] - ref_data[idx]).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }
    }
    assert!(
        max_diff < 1e-3,
        "streaming slope interior max diff {max_diff} exceeds tolerance 1e-3"
    );
}

// ============================================================================
// 11. streaming_focal_mean with radius=2 vs full focal_mean on one chunk
// ============================================================================

#[test]
fn test_streaming_focal_mean_radius_2_matches_full_focal_mean() {
    use oxigdal_algorithms::raster::{
        FocalBoundaryMode, WindowShape, focal_mean as full_focal_mean,
    };
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    let w = 8_usize;
    let h = 8_usize;
    let data: Vec<f32> = make_ramp(w, h);

    let src_buf = RasterBuffer::from_typed_vec(w, h, data.clone(), RasterDataType::Float32)
        .expect("build RasterBuffer");
    let window = WindowShape::circular(2.0).expect("circular window");
    let ref_buf =
        full_focal_mean(&src_buf, &window, &FocalBoundaryMode::Edge).expect("full focal mean");
    let ref_data: Vec<f32> = ref_buf.as_slice::<f32>().expect("as_slice").to_vec();

    // chunk_size=8 means the whole raster fits in a single chunk; halo=2 for radius=2.
    let src = InMemoryRasterSource::new(data, w, h);
    let cr = ChunkedRaster::new(src, 8, 2).expect("create ChunkedRaster");
    let stream_data = streaming_focal_mean(&cr, 2).expect("streaming focal mean");

    assert_eq!(stream_data.len(), ref_data.len());

    // Interior pixels (radius away from any edge) should match very closely.
    let radius = 2_usize;
    let mut max_diff = 0.0_f32;
    for row in radius..(h - radius) {
        for col in radius..(w - radius) {
            let idx = row * w + col;
            let diff = (stream_data[idx] - ref_data[idx]).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }
    }
    assert!(
        max_diff < 1e-3,
        "streaming focal_mean interior max diff {max_diff} exceeds tolerance 1e-3"
    );
}

// ============================================================================
// 12. Sum of (chunk.width × chunk.height) == total raster size
// ============================================================================

#[test]
fn test_chunk_iterator_total_pixels_processed_equals_raster_size() {
    let w = 13_usize;
    let h = 11_usize;
    let src = InMemoryRasterSource::new(vec![0.0_f32; w * h], w, h);
    let cr = ChunkedRaster::new(src, 5, 1).expect("create ChunkedRaster");

    let total: usize = cr
        .iter()
        .map(|c| {
            let chunk = c.expect("chunk ok");
            chunk.width * chunk.height
        })
        .sum();

    assert_eq!(
        total,
        w * h,
        "sum of chunk core pixels must equal total raster pixels"
    );
}

// ============================================================================
// 13. Non-multiple chunk_size: 7×7 raster, chunk_size=4, halo=0 → 4 chunks sum to 49
// ============================================================================

#[test]
fn test_streaming_handles_non_multiple_chunk_size() {
    let w = 7_usize;
    let h = 7_usize;
    let src = InMemoryRasterSource::new(vec![1.0_f32; w * h], w, h);
    let cr = ChunkedRaster::new(src, 4, 0).expect("create ChunkedRaster");

    let chunk_count = cr.iter().count();
    // ceil(7/4) = 2 per dimension → 2×2 = 4 chunks
    assert_eq!(
        chunk_count, 4,
        "7×7 raster with chunk_size=4 should yield 4 chunks"
    );

    // Re-iterate to count total pixels
    let src2 = InMemoryRasterSource::new(vec![1.0_f32; w * h], w, h);
    let cr2 = ChunkedRaster::new(src2, 4, 0).expect("create ChunkedRaster");
    let total: usize = cr2
        .iter()
        .map(|c| {
            let chunk = c.expect("chunk ok");
            chunk.width * chunk.height
        })
        .sum();
    assert_eq!(total, 49, "total pixels must equal 7*7=49");
}
