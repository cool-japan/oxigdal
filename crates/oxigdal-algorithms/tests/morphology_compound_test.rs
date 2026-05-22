//! Integration tests for the compound morphological operators on flat
//! `&[f32]` rasters.
//!
//! These tests verify the OxiGDAL Slice 13 / W6 acceptance criteria:
//!
//! - `opening` removes small bright features and preserves large ones
//! - `closing` fills small dark gaps and preserves large ones
//! - `top_hat` / `black_hat` extract small bright / dark features
//! - opening and closing are idempotent (within epsilon)
//! - `*_with` option-bag variants compose correctly with `iterations > 1`

use oxigdal_algorithms::{
    BorderMode, MorphologyOptions, black_hat, black_hat_with, closing, closing_with, opening,
    opening_with, top_hat, top_hat_with,
};

/// Construct a `width x height` raster filled with `bg`, with a single
/// foreground pixel of `fg` at `(x, y)`.
fn raster_with_spike(
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    bg: f32,
    fg: f32,
) -> Vec<f32> {
    let mut r = vec![bg; width * height];
    r[y * width + x] = fg;
    r
}

/// Construct a `width x height` raster filled with `bg`, with a square
/// `[x0..x1) x [y0..y1)` set to `fg`.
#[allow(clippy::too_many_arguments)]
fn raster_with_square(
    width: usize,
    height: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    bg: f32,
    fg: f32,
) -> Vec<f32> {
    let mut r = vec![bg; width * height];
    for y in y0..y1 {
        for x in x0..x1 {
            r[y * width + x] = fg;
        }
    }
    r
}

/// Check two rasters are equal within `eps` and report the first
/// divergence on failure.
fn assert_rasters_close(a: &[f32], b: &[f32], width: usize, eps: f32) {
    assert_eq!(a.len(), b.len(), "raster length mismatch");
    for i in 0..a.len() {
        let diff = (a[i] - b[i]).abs();
        assert!(
            diff <= eps,
            "raster mismatch at index {} (x={}, y={}): a={}, b={}, diff={}",
            i,
            i % width,
            i / width,
            a[i],
            b[i],
            diff
        );
    }
}

#[test]
fn test_opening_removes_small_bright_features() {
    // A flat dark field with a single bright spike: opening with a 3x3
    // square structuring element must annihilate the spike.
    let raster = raster_with_spike(10, 10, 5, 5, 0.0, 100.0);
    let opened = opening(&raster, 10, 10, 3);

    assert_eq!(opened.len(), 10 * 10);
    assert!(
        opened[5 * 10 + 5].abs() < 1e-6,
        "spike not removed: opened[5,5]={}",
        opened[5 * 10 + 5]
    );
    // The entire output should be zero (the input is zero everywhere
    // except the single spike, which opening should erase).
    for (i, &v) in opened.iter().enumerate() {
        assert!(v.abs() < 1e-6, "non-zero residual at index {}: {}", i, v);
    }
}

#[test]
fn test_opening_preserves_large_bright_features() {
    // A 5x5 bright square in a 16x16 dark field: a 3x3 opening should
    // preserve the interior (it shrinks the perimeter by half-kernel and
    // then re-grows it back). Specifically the 3x3 inner core of the
    // square (rows 5..8 cols 5..8, but counting carefully) should still
    // be bright after opening.
    let raster = raster_with_square(16, 16, 5, 10, 5, 10, 0.0, 100.0);
    let opened = opening(&raster, 16, 16, 3);

    // The inner 3x3 core (centre pixels of the square) must be preserved.
    for y in 6..9 {
        for x in 6..9 {
            let v = opened[y * 16 + x];
            assert!(
                (v - 100.0).abs() < 1e-4,
                "expected preserved bright pixel at ({},{}), got {}",
                x,
                y,
                v
            );
        }
    }
}

#[test]
fn test_closing_fills_small_gaps() {
    // A bright field with a single dark pinhole. Closing with a 3x3
    // structuring element must fill the hole.
    let raster = raster_with_spike(10, 10, 5, 5, 100.0, 0.0);
    let closed = closing(&raster, 10, 10, 3);

    assert_eq!(closed.len(), 10 * 10);
    assert!(
        (closed[5 * 10 + 5] - 100.0).abs() < 1e-6,
        "pinhole not filled: closed[5,5]={}",
        closed[5 * 10 + 5]
    );
    // Entire output should be uniformly bright.
    for (i, &v) in closed.iter().enumerate() {
        assert!(
            (v - 100.0).abs() < 1e-6,
            "non-bright pixel at index {}: {}",
            i,
            v
        );
    }
}

#[test]
fn test_closing_preserves_large_gaps() {
    // A bright field with a 5x5 dark hole. A small (3x3) closing cannot
    // bridge it: the dark hole core must remain dark.
    let raster = raster_with_square(16, 16, 5, 10, 5, 10, 100.0, 0.0);
    let closed = closing(&raster, 16, 16, 3);

    for y in 6..9 {
        for x in 6..9 {
            let v = closed[y * 16 + x];
            assert!(
                v.abs() < 1e-4,
                "expected preserved dark pixel at ({},{}), got {}",
                x,
                y,
                v
            );
        }
    }
}

#[test]
fn test_top_hat_extracts_thin_bright_features() {
    // A flat dark field with a single bright spike: top-hat should
    // recover exactly the spike (since opening annihilates it).
    let raster = raster_with_spike(10, 10, 5, 5, 0.0, 100.0);
    let th = top_hat(&raster, 10, 10, 3);

    assert!(
        (th[5 * 10 + 5] - 100.0).abs() < 1e-6,
        "spike not extracted: top_hat[5,5]={}",
        th[5 * 10 + 5]
    );
    // Elsewhere the top-hat should be ~0 since both raster and opening
    // are 0 there.
    for y in 0..10 {
        for x in 0..10 {
            if !(x == 5 && y == 5) {
                let v = th[y * 10 + x];
                assert!(
                    v.abs() < 1e-6,
                    "expected zero background, got top_hat[{},{}]={}",
                    x,
                    y,
                    v
                );
            }
        }
    }
}

#[test]
fn test_black_hat_extracts_thin_dark_features() {
    // A bright field with a single dark pinhole: black-hat should
    // extract the pinhole as a bright pixel.
    let raster = raster_with_spike(10, 10, 5, 5, 100.0, 0.0);
    let bh = black_hat(&raster, 10, 10, 3);

    assert!(
        (bh[5 * 10 + 5] - 100.0).abs() < 1e-6,
        "dark pixel not extracted: black_hat[5,5]={}",
        bh[5 * 10 + 5]
    );
    for y in 0..10 {
        for x in 0..10 {
            if !(x == 5 && y == 5) {
                let v = bh[y * 10 + x];
                assert!(
                    v.abs() < 1e-6,
                    "expected zero background, got black_hat[{},{}]={}",
                    x,
                    y,
                    v
                );
            }
        }
    }
}

#[test]
fn test_opening_idempotent_on_second_application() {
    // Mathematical morphology: opening is idempotent — opening(opening(x))
    // == opening(x). Use a non-trivial raster with a mix of features.
    let mut raster = vec![0.0f32; 12 * 12];
    // A medium bright region
    for y in 4..9 {
        for x in 4..9 {
            raster[y * 12 + x] = 80.0;
        }
    }
    // Plus a small bright spike that opening will remove
    raster[2 * 12 + 10] = 100.0;
    // Plus a tiny bright pair that opening will remove
    raster[10 * 12 + 1] = 60.0;

    let opened_once = opening(&raster, 12, 12, 3);
    let opened_twice = opening(&opened_once, 12, 12, 3);

    assert_rasters_close(&opened_once, &opened_twice, 12, 1e-5);
}

#[test]
fn test_closing_idempotent_on_second_application() {
    // Closing is also idempotent.
    let mut raster = vec![100.0f32; 12 * 12];
    for y in 4..9 {
        for x in 4..9 {
            raster[y * 12 + x] = 20.0;
        }
    }
    // Small dark pinhole that closing will fill
    raster[2 * 12 + 10] = 0.0;
    raster[10 * 12 + 1] = 0.0;

    let closed_once = closing(&raster, 12, 12, 3);
    let closed_twice = closing(&closed_once, 12, 12, 3);

    assert_rasters_close(&closed_once, &closed_twice, 12, 1e-5);
}

#[test]
fn test_opening_then_closing_equals_alternating_filter() {
    // Sanity check: composing opening then closing produces a valid
    // raster of the right dimensions, no panics, no NaNs.
    let raster = raster_with_square(16, 16, 4, 12, 4, 12, 10.0, 90.0);
    let opened = opening(&raster, 16, 16, 3);
    let asf = closing(&opened, 16, 16, 3);

    assert_eq!(asf.len(), 16 * 16);
    for (i, &v) in asf.iter().enumerate() {
        assert!(v.is_finite(), "non-finite output at index {}: {}", i, v);
    }
}

#[test]
fn test_morphology_options_iterations_compose() {
    // `opening_with` with iterations=2 must equal `opening(opening(x))`.
    let mut raster = vec![0.0f32; 14 * 14];
    for y in 5..10 {
        for x in 5..10 {
            raster[y * 14 + x] = 50.0;
        }
    }
    raster[14 + 12] = 100.0; // small spike at (12,1) that opens away
    raster[12 * 14 + 1] = 100.0; // another small spike at (1,12)

    let opts = MorphologyOptions::new(3).with_iterations(2);
    let twice_via_opts = opening_with(&raster, 14, 14, &opts);

    let once = opening(&raster, 14, 14, 3);
    let twice_manual = opening(&once, 14, 14, 3);

    assert_rasters_close(&twice_via_opts, &twice_manual, 14, 1e-5);
}

#[test]
fn test_closing_with_iterations_compose() {
    // Mirror of the opening_with composition test.
    let mut raster = vec![100.0f32; 14 * 14];
    for y in 5..10 {
        for x in 5..10 {
            raster[y * 14 + x] = 20.0;
        }
    }
    raster[14 + 12] = 0.0;
    raster[12 * 14 + 1] = 0.0;

    let opts = MorphologyOptions::new(3).with_iterations(2);
    let twice_via_opts = closing_with(&raster, 14, 14, &opts);

    let once = closing(&raster, 14, 14, 3);
    let twice_manual = closing(&once, 14, 14, 3);

    assert_rasters_close(&twice_via_opts, &twice_manual, 14, 1e-5);
}

#[test]
fn test_top_hat_with_matches_top_hat_single_iteration() {
    // With iterations=1, *_with variants must match the simple variants.
    let raster = raster_with_spike(10, 10, 5, 5, 0.0, 100.0);

    let th_simple = top_hat(&raster, 10, 10, 3);
    let th_with = top_hat_with(&raster, 10, 10, &MorphologyOptions::new(3));

    assert_rasters_close(&th_simple, &th_with, 10, 1e-6);
}

#[test]
fn test_black_hat_with_matches_black_hat_single_iteration() {
    let raster = raster_with_spike(10, 10, 5, 5, 100.0, 0.0);

    let bh_simple = black_hat(&raster, 10, 10, 3);
    let bh_with = black_hat_with(&raster, 10, 10, &MorphologyOptions::new(3));

    assert_rasters_close(&bh_simple, &bh_with, 10, 1e-6);
}

#[test]
fn test_border_mode_default_is_replicate() {
    let m = BorderMode::default();
    let is_replicate = matches!(m, BorderMode::Replicate);
    assert!(is_replicate, "expected default border mode to be Replicate");
}
