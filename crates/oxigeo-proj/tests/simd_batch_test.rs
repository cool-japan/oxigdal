//! Integration tests for SIMD-accelerated batch coordinate transformation.
//!
//! These tests verify that `Transformer::transform_batch` produces results
//! numerically consistent with single-point calls to the same batch kernel
//! for the three supported fast-path projections (Transverse Mercator,
//! Mercator, Lambert Conformal Conic), and that the scalar fallback works
//! correctly for projections not in the supported set.
//!
//! Design note — scope of this file, and what it deliberately does NOT cover
//! ------------------------------------------------------------------------
//! `Transformer::transform_batch` routes through our pure-Rust SIMD kernels
//! for TM/UTM, Mercator, and LCC.  Every test below verifies that the batch
//! result (n points together) matches the result of processing each point
//! individually through `transform_batch` (1-point slice), which exercises the
//! kernel's scalar tail.  That isolates **batching and lane handling**: both
//! sides execute identical f64 arithmetic, so agreement is sub-nanometre.
//!
//! An earlier version of this note argued that comparing `transform_batch`
//! against `transform` "would test formula agreement, not batching
//! correctness", and used that as a reason not to do it anywhere.  **That
//! reasoning was wrong and it cost us a shipped bug.**  Comparing a kernel only
//! against itself cannot detect a kernel that drops one of the projection's
//! parameters — such a kernel is perfectly self-consistent while being
//! arbitrarily wrong.  The Transverse Mercator kernel hardcoded `+lat_0=0`;
//! UTM has `lat_0 = 0` so every test here passed, while the Japan Plane
//! Rectangular CS came out 3 985 144 m too far north.
//!
//! Parameter fidelity is therefore covered separately, in
//! `simd_batch_params_test.rs`, which compares the batch path against the
//! scalar OxiProj pipeline (a genuinely independent implementation) and against
//! absolute PROJ 9.5.1 coordinates.  Keep both: this file guards the lane
//! logic, that one guards the parameters.

#[allow(clippy::expect_used)]
mod simd_batch {
    use oxigeo_proj::transform::{Coordinate, Transformer};

    /// Tolerance: results from the same kernel must agree to machine-precision,
    /// so 1e-6 m is a very generous bound.
    const METRE_TOL: f64 = 1e-6;

    // -------------------------------------------------------------------------
    // Helper: generate a deterministic sequence of (lon, lat) coordinates.
    // -------------------------------------------------------------------------

    fn make_coords(
        count: usize,
        lon_start: f64,
        lon_step: f64,
        lat_start: f64,
        lat_step: f64,
    ) -> Vec<Coordinate> {
        (0..count)
            .map(|i| {
                Coordinate::from_lon_lat(
                    lon_start + i as f64 * lon_step,
                    lat_start + i as f64 * lat_step,
                )
            })
            .collect()
    }

    // =========================================================================
    // Test 1 — TM (UTM zone 32N, EPSG:32632): batch vs scalar within 1e-6 m
    //
    // Processes 256 points through the TM SIMD kernel.  Each result is
    // compared against the single-point result from the same batch kernel
    // (1-element slice → scalar tail).
    // =========================================================================

    #[test]
    fn test_transform_batch_matches_scalar_within_1ulp_tm() {
        let coords = make_coords(256, -10.0, 0.1, 40.0, 0.05);

        let transformer = Transformer::from_epsg(4326, 32632)
            .expect("should build EPSG:4326→EPSG:32632 transformer")
            .with_strict(false);

        // SIMD batch result (may use 4-lane unrolled loop)
        let batch_results = transformer
            .transform_batch(&coords)
            .expect("batch transform should succeed");

        assert_eq!(batch_results.len(), 256, "result length must match input");

        // Reference: process each point individually via transform_batch(&[c])
        // → exercises the kernel's scalar tail (remainder % 4 == 1).
        for (i, (coord, batch)) in coords.iter().zip(&batch_results).enumerate() {
            let ref_result = transformer
                .transform_batch(&[*coord])
                .expect("single-point batch should succeed");

            assert_eq!(ref_result.len(), 1);
            let reference = ref_result[0];

            assert!(
                (batch.x - reference.x).abs() <= METRE_TOL,
                "easting mismatch at i={i}: batch={:.9} ref={:.9} diff={:.2e}",
                batch.x,
                reference.x,
                (batch.x - reference.x).abs()
            );
            assert!(
                (batch.y - reference.y).abs() <= METRE_TOL,
                "northing mismatch at i={i}: batch={:.9} ref={:.9} diff={:.2e}",
                batch.y,
                reference.y,
                (batch.y - reference.y).abs()
            );
        }
    }

    // =========================================================================
    // Test 2 — Mercator (EPSG:4326 → EPSG:3857): batch vs scalar within 1e-6 m
    // =========================================================================

    #[test]
    fn test_transform_batch_matches_scalar_within_1ulp_mercator() {
        // 256 coordinates across the world (Web Mercator valid range is ±85°).
        let coords = make_coords(256, -100.0, 0.5, -30.0, 0.25);

        let transformer = Transformer::from_epsg(4326, 3857)
            .expect("should build EPSG:4326→EPSG:3857 transformer")
            .with_strict(false);

        let batch_results = transformer
            .transform_batch(&coords)
            .expect("batch transform should succeed");

        assert_eq!(batch_results.len(), 256);

        for (i, (coord, batch)) in coords.iter().zip(&batch_results).enumerate() {
            let ref_result = transformer
                .transform_batch(&[*coord])
                .expect("single-point batch should succeed");

            assert_eq!(ref_result.len(), 1);
            let reference = ref_result[0];

            assert!(
                (batch.x - reference.x).abs() <= METRE_TOL,
                "x mismatch at i={i}: batch={:.9} ref={:.9} diff={:.2e}",
                batch.x,
                reference.x,
                (batch.x - reference.x).abs()
            );
            assert!(
                (batch.y - reference.y).abs() <= METRE_TOL,
                "y mismatch at i={i}: batch={:.9} ref={:.9} diff={:.2e}",
                batch.y,
                reference.y,
                (batch.y - reference.y).abs()
            );
        }
    }

    // =========================================================================
    // Test 3 — LCC (EPSG:2154 RGF93/Lambert-93): relative tolerance 1e-9
    //
    // Batch-vs-scalar-tail consistency with a relative tolerance that reflects
    // double-precision arithmetic.  (The kernel used to be sphere-based, which
    // put it ~2e2 m away from the true ellipsoidal LCC; it is ellipsoidal now,
    // and `simd_batch_params_test.rs` is what pins that.)
    // =========================================================================

    #[test]
    fn test_transform_batch_matches_scalar_lcc_1e9() {
        // Lambert-93 covers France (roughly lon 2°–8°, lat 43°–51°)
        let coords = make_coords(128, 2.0, 0.05, 43.0, 0.06);

        let transformer = Transformer::from_epsg(4326, 2154)
            .expect("should build EPSG:4326→EPSG:2154 transformer")
            .with_strict(false);

        let batch_results = transformer
            .transform_batch(&coords)
            .expect("batch transform should succeed");

        assert_eq!(batch_results.len(), 128);

        for (i, (coord, batch)) in coords.iter().zip(&batch_results).enumerate() {
            let ref_result = transformer
                .transform_batch(&[*coord])
                .expect("single-point batch should succeed");

            assert_eq!(ref_result.len(), 1);
            let reference = ref_result[0];

            // Relative tolerance: |batch - ref| / max(|ref|, 1) < 1e-9
            let rel_x = (batch.x - reference.x).abs() / reference.x.abs().max(1.0);
            let rel_y = (batch.y - reference.y).abs() / reference.y.abs().max(1.0);

            assert!(
                rel_x < 1e-9,
                "x relative mismatch at i={i}: batch={:.3} ref={:.3} rel={:.2e}",
                batch.x,
                reference.x,
                rel_x
            );
            assert!(
                rel_y < 1e-9,
                "y relative mismatch at i={i}: batch={:.3} ref={:.3} rel={:.2e}",
                batch.y,
                reference.y,
                rel_y
            );
        }
    }

    // =========================================================================
    // Test 4 — Partial tail: 5 points (not a multiple of 4)
    //
    // Verifies that the remainder handling (5 % 4 = 1 trailing point) in the
    // AVX2 kernel path produces the same result as the scalar tail.
    // =========================================================================

    #[test]
    fn test_transform_batch_partial_lane_at_tail() {
        let coords = make_coords(5, 8.0, 0.1, 48.0, 0.05);

        let transformer = Transformer::from_epsg(4326, 32632)
            .expect("should build transformer")
            .with_strict(false);

        let batch_results = transformer
            .transform_batch(&coords)
            .expect("batch transform should succeed");

        assert_eq!(batch_results.len(), 5, "all 5 points should be present");

        for (i, (coord, batch)) in coords.iter().zip(&batch_results).enumerate() {
            assert!(batch.is_valid(), "result at i={i} must be finite");

            // Compare against single-point batch (scalar tail path)
            let ref_result = transformer
                .transform_batch(&[*coord])
                .expect("single-point batch should succeed");

            assert_eq!(ref_result.len(), 1);
            let reference = ref_result[0];

            assert!(
                (batch.x - reference.x).abs() <= METRE_TOL,
                "x mismatch at i={i}: batch={:.9} ref={:.9}",
                batch.x,
                reference.x
            );
            assert!(
                (batch.y - reference.y).abs() <= METRE_TOL,
                "y mismatch at i={i}: batch={:.9} ref={:.9}",
                batch.y,
                reference.y
            );
        }
    }

    // =========================================================================
    // Test 5 — Empty input: no panic, empty result
    // =========================================================================

    #[test]
    fn test_transform_batch_empty_input() {
        let transformer = Transformer::from_epsg(4326, 32632).expect("should build transformer");

        let result = transformer.transform_batch(&[]);
        assert!(result.is_ok(), "empty batch should succeed");
        assert!(result.expect("ok").is_empty(), "result should be empty");
    }

    // =========================================================================
    // Test 6 — Fallback: projection not in the SIMD set (EPSG:3413 — polar stere)
    //
    // When `try_simd_batch` returns `None`, `transform_batch` falls back to the
    // scalar OxiProj loop.  The result must be identical to `transform` since
    // both go through the same OxiProj pipeline.
    // =========================================================================

    #[test]
    fn test_transform_batch_falls_back_when_no_simd_path_available() {
        // EPSG:3413 = WGS84 / NSIDC Sea Ice Polar Stereographic North
        // `+proj=stere` is NOT in the SIMD-accelerated set → fallback.
        let coords = make_coords(8, -45.0, 5.0, 70.0, 1.0);

        let transformer = Transformer::from_epsg(4326, 3413)
            .expect("should build EPSG:4326→EPSG:3413 transformer")
            .with_strict(false);

        let batch_results = transformer
            .transform_batch(&coords)
            .expect("batch transform should succeed via fallback");

        assert_eq!(batch_results.len(), 8);

        for (i, (coord, batch)) in coords.iter().zip(&batch_results).enumerate() {
            let scalar = transformer
                .transform(coord)
                .expect("scalar transform should succeed");

            // Fallback path goes through OxiProj for both → results must be
            // bit-identical.
            assert_eq!(
                batch.x, scalar.x,
                "x must be identical (both through OxiProj) at i={i}"
            );
            assert_eq!(
                batch.y, scalar.y,
                "y must be identical (both through OxiProj) at i={i}"
            );
        }
    }
}
