//! End-to-end integration test for the PCA dimensionality index
//! (Demantké et al. 2011) used by the `oxigeo-3d` classifier.
//!
//! The eigensolver (`symmetric_eig_3x3`) and feature computation
//! (`dimensionality_features`) are `pub(crate)` and therefore not reachable
//! from an integration test; their unit tests live inline in
//! `src/classification.rs` under `#[cfg(test)] mod tests`.
//!
//! This file exercises the *public* classification API, asserting that the
//! planarity-driven building extractor cleanly separates a planar (roof-like)
//! patch from a scattered (vegetation-like) cluster at the same height.

use oxigeo_3d::classification::{
    ClassificationParams, extract_buildings, extract_buildings_with_params,
};
use oxigeo_3d::pointcloud::Point;

/// Deterministic pseudo-random jitter in `[-amp, amp]` driven by a small LCG,
/// so the "vegetation" cluster is reproducible without a RNG dependency.
fn jitter(seed: &mut u64, amp: f64) -> f64 {
    // Numerical Recipes LCG constants.
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let unit = (*seed >> 11) as f64 / (1u64 << 53) as f64; // [0, 1)
    (unit * 2.0 - 1.0) * amp
}

/// Build a flat ground patch at z ≈ 0 — used as the reference surface that the
/// building extractor measures "height above ground" against.
fn ground_patch() -> Vec<Point> {
    let mut pts = Vec::new();
    for ix in -6..=6 {
        for iy in -6..=6 {
            pts.push(Point::new(ix as f64 * 0.5, iy as f64 * 0.5, 0.0));
        }
    }
    pts
}

#[test]
fn test_classification_ground_vs_vegetation_separation() {
    let ground = ground_patch();

    // Planar roof-like patch: dense, flat, well above the building-height
    // threshold (default 3.0 m). A horizontal plane → planarity ≈ 1.
    let roof_z = 8.0;
    let mut planar = Vec::new();
    for ix in -6..=6 {
        for iy in -6..=6 {
            planar.push(Point::new(ix as f64 * 0.3, iy as f64 * 0.3, roof_z));
        }
    }

    // Scattered vegetation-like cluster: same mean height, but fully 3D
    // scatter → low planarity. Offset far in X so neighbor searches do not mix
    // the two clusters.
    let mut seed: u64 = 0x1234_5678_9abc_def0;
    let mut scattered = Vec::new();
    for ix in -6..=6 {
        for iy in -6..=6 {
            let cx = 100.0 + ix as f64 * 0.3 + jitter(&mut seed, 0.4);
            let cy = iy as f64 * 0.3 + jitter(&mut seed, 0.4);
            let cz = roof_z + jitter(&mut seed, 1.5);
            scattered.push(Point::new(cx, cy, cz));
        }
    }

    // --- Planar patch should be extracted as buildings ---
    let mut planar_scene = ground.clone();
    planar_scene.extend(planar.clone());
    let planar_buildings = extract_buildings(&planar_scene, &ground)
        .expect("building extraction on the planar scene should succeed");
    assert!(
        !planar_buildings.is_empty(),
        "a flat roof-like patch should yield building points (planarity ≈ 1)"
    );

    // --- Scattered cluster should NOT be extracted as buildings ---
    let mut scattered_scene = ground.clone();
    scattered_scene.extend(scattered.clone());
    let scattered_buildings = extract_buildings(&scattered_scene, &ground)
        .expect("building extraction on the scattered scene should succeed");

    // The scattered cluster has the same mean height but a fully 3D point
    // distribution, so its local covariance is far from planar: with the
    // Demantké index `P_λ = (λ₂ - λ₃) / λ₁`, the non-zero λ₃ drives planarity
    // well below the 0.8 building threshold. It must yield no building points.
    assert!(
        scattered_buildings.is_empty(),
        "scattered (vegetation-like) cluster should yield no building points, \
         got {}",
        scattered_buildings.len(),
    );

    // The planar patch separates cleanly from the scattered cluster: it
    // contributes strictly more building points. This is the ground-vs-
    // vegetation separation the Demantké dimensionality index provides — a
    // flat surface scores high planarity, a scattered cluster scores low.
    assert!(
        planar_buildings.len() > scattered_buildings.len(),
        "planar patch ({} buildings) must out-score the scattered cluster \
         ({} buildings) on planarity",
        planar_buildings.len(),
        scattered_buildings.len(),
    );

    // Sanity bound: the planar patch's interior points (those with a full,
    // roughly isotropic neighborhood disc) pass the threshold, so the planar
    // patch yields a non-trivial number of building points.
    assert!(
        planar_buildings.len() >= 5,
        "the planar patch interior should yield several building points, \
         got {} of {}",
        planar_buildings.len(),
        planar.len(),
    );
}

#[test]
fn test_planarity_threshold_respects_custom_params() {
    // A planar patch well above a *raised* building-height threshold is still
    // extracted; the planarity computation is independent of the height gate.
    let ground = ground_patch();
    let mut planar = Vec::new();
    for ix in -5..=5 {
        for iy in -5..=5 {
            planar.push(Point::new(ix as f64 * 0.3, iy as f64 * 0.3, 12.0));
        }
    }
    let mut scene = ground.clone();
    scene.extend(planar);

    let params = ClassificationParams {
        building_height: 10.0,
        ..ClassificationParams::default()
    };
    let buildings = extract_buildings_with_params(&scene, &ground, &params)
        .expect("building extraction with custom params should succeed");
    assert!(
        !buildings.is_empty(),
        "a planar patch above a raised height threshold should still extract"
    );
}
