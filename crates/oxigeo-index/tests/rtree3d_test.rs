//! Integration tests for the 3D R-tree (`RTree3D`) and `Bbox3D`.

use oxigeo_index::{Bbox3D, RTree3D};

// ---------------------------------------------------------------------------
// Bbox3D tests
// ---------------------------------------------------------------------------

#[test]
fn test_bbox3d_volume_and_surface_area_basic() {
    // 2 × 3 × 4 box: volume = 24, surface area = 2*(6+8+12) = 52.
    let b = Bbox3D::new(0.0, 0.0, 0.0, 2.0, 3.0, 4.0).expect("valid bbox");
    assert_eq!(b.width(), 2.0);
    assert_eq!(b.height(), 3.0);
    assert_eq!(b.depth(), 4.0);
    assert_eq!(b.volume(), 24.0);
    assert_eq!(b.surface_area(), 52.0);

    // Unit cube: volume = 1, surface area = 6.
    let cube = Bbox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).expect("unit cube");
    assert_eq!(cube.volume(), 1.0);
    assert_eq!(cube.surface_area(), 6.0);

    // Flat rectangle embedded in Z: depth = 0, volume = 0, surface area = 2*(w*h).
    let flat = Bbox3D::new(0.0, 0.0, 5.0, 3.0, 4.0, 5.0).expect("flat");
    assert_eq!(flat.volume(), 0.0);
    // 2*(3*4 + 3*0 + 4*0) = 24
    assert_eq!(flat.surface_area(), 24.0);
    assert!(flat.is_degenerate());
}

#[test]
fn test_bbox3d_intersects_disjoint_and_overlapping() {
    let a = Bbox3D::new(0.0, 0.0, 0.0, 3.0, 3.0, 3.0).unwrap();

    // Overlapping in all three dimensions.
    let b_overlap = Bbox3D::new(2.0, 2.0, 2.0, 5.0, 5.0, 5.0).unwrap();
    assert!(a.intersects(&b_overlap));
    assert!(b_overlap.intersects(&a));

    // Touching on a face: still intersects (closed intervals).
    let b_touch = Bbox3D::new(3.0, 0.0, 0.0, 6.0, 3.0, 3.0).unwrap();
    assert!(a.intersects(&b_touch));

    // Disjoint in X.
    let b_disjoint_x = Bbox3D::new(4.0, 0.0, 0.0, 7.0, 3.0, 3.0).unwrap();
    assert!(!a.intersects(&b_disjoint_x));

    // Disjoint in Y only.
    let b_disjoint_y = Bbox3D::new(0.0, 5.0, 0.0, 3.0, 8.0, 3.0).unwrap();
    assert!(!a.intersects(&b_disjoint_y));

    // Disjoint in Z only.
    let b_disjoint_z = Bbox3D::new(0.0, 0.0, 5.0, 3.0, 3.0, 8.0).unwrap();
    assert!(!a.intersects(&b_disjoint_z));

    // Intersection: the overlap region.
    let intersection = a.intersection(&b_overlap).expect("should intersect");
    assert_eq!(
        intersection,
        Bbox3D::new(2.0, 2.0, 2.0, 3.0, 3.0, 3.0).unwrap()
    );

    // No intersection when disjoint.
    assert!(a.intersection(&b_disjoint_x).is_none());
}

// ---------------------------------------------------------------------------
// RTree3D — basic insert and search
// ---------------------------------------------------------------------------

#[test]
fn test_rtree3d_insert_and_search_simple() {
    let mut tree: RTree3D<u32> = RTree3D::new();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);

    // Insert three non-overlapping cubes.
    let b0 = Bbox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).unwrap();
    let b1 = Bbox3D::new(5.0, 5.0, 5.0, 6.0, 6.0, 6.0).unwrap();
    let b2 = Bbox3D::new(10.0, 10.0, 10.0, 11.0, 11.0, 11.0).unwrap();

    tree.insert(b0, 0_u32);
    tree.insert(b1, 1_u32);
    tree.insert(b2, 2_u32);

    assert_eq!(tree.len(), 3);
    assert!(!tree.is_empty());

    // Query overlapping only b0.
    let q0 = Bbox3D::new(-0.5, -0.5, -0.5, 0.5, 0.5, 0.5).unwrap();
    let hits0 = tree.search(&q0);
    assert_eq!(hits0.len(), 1);
    assert_eq!(*hits0[0], 0);

    // Query overlapping only b1.
    let q1 = Bbox3D::new(5.5, 5.5, 5.5, 5.9, 5.9, 5.9).unwrap();
    let hits1 = tree.search(&q1);
    assert_eq!(hits1.len(), 1);
    assert_eq!(*hits1[0], 1);

    // World query — all three.
    let world = Bbox3D::new(-100.0, -100.0, -100.0, 100.0, 100.0, 100.0).unwrap();
    let all = tree.search(&world);
    assert_eq!(all.len(), 3);

    // Disjoint query — none.
    let empty_q = Bbox3D::new(50.0, 50.0, 50.0, 60.0, 60.0, 60.0).unwrap();
    assert!(tree.search(&empty_q).is_empty());
}

// ---------------------------------------------------------------------------
// RTree3D — k-NN distance ordering
// ---------------------------------------------------------------------------

#[test]
fn test_rtree3d_knn_3d_distance_orders_correctly() {
    let mut tree: RTree3D<u32> = RTree3D::new();

    // Place 10 unit cubes at integer positions along the x-axis.
    // Distance of cube at x=i from origin along x-axis is i-1 (when i>=1).
    for i in 0..10_u32 {
        let f = i as f64 * 3.0; // spaced 3 apart so they do not touch.
        tree.insert(Bbox3D::new(f, 0.0, 0.0, f + 1.0, 1.0, 1.0).unwrap(), i);
    }
    assert_eq!(tree.len(), 10);

    // Query from the origin. The k-NN call returns values only, ordered by
    // min-distance.  The first cube (i=0, [0..1]^3) contains the origin
    // (distance 0).  The second (i=1, [3..4]^3) is at distance 2 in x.
    let nn = tree.nearest_k(0.0, 0.0, 0.0, 3);
    assert_eq!(nn.len(), 3);

    // Nearest must be the cube that contains the query point (i=0).
    assert_eq!(*nn[0], 0);
    // Second-nearest is i=1 (min dist = 3-1=2 along x).
    assert_eq!(*nn[1], 1);
    // Third-nearest is i=2 (min dist = 6-1=5 along x).
    assert_eq!(*nn[2], 2);

    // Requesting more than available returns all.
    let all_nn = tree.nearest_k(0.0, 0.0, 0.0, 100);
    assert_eq!(all_nn.len(), 10);

    // Requesting 0 returns empty.
    assert!(tree.nearest_k(0.0, 0.0, 0.0, 0).is_empty());
}

// ---------------------------------------------------------------------------
// RTree3D — 1000-point correctness vs brute force
// ---------------------------------------------------------------------------

/// Deterministic pseudo-random value in `[0, upper)` using the golden ratio.
/// Index `i` is combined with an axis seed to produce distinct sequences per
/// axis without pulling in the `rand` crate.
#[inline]
fn pseudo_coord(i: usize, axis_seed: f64, upper: f64) -> f64 {
    ((i as f64 * 1.618_033_988_749_895 + axis_seed) % upper).abs()
}

#[test]
fn test_rtree3d_point_cloud_1000_uniform_query_correctness() {
    let n = 1000_usize;
    let mut tree: RTree3D<usize> = RTree3D::new();
    let mut reference: Vec<(Bbox3D, usize)> = Vec::with_capacity(n);

    for i in 0..n {
        let x = pseudo_coord(i, 0.0, 100.0);
        let y = pseudo_coord(i, 31.4159, 100.0);
        let z = pseudo_coord(i, 7.1828, 100.0);
        // Point-like bbox (side = 0.1 to avoid exact-boundary confusion).
        let bbox = Bbox3D::new(x, y, z, x + 0.1, y + 0.1, z + 0.1).unwrap();
        tree.insert(bbox, i);
        reference.push((bbox, i));
    }

    assert_eq!(tree.len(), n);

    // Query a fixed sub-region and compare against brute force.
    let query = Bbox3D::new(20.0, 20.0, 20.0, 60.0, 60.0, 60.0).unwrap();

    let tree_hits: Vec<usize> = {
        let mut v: Vec<usize> = tree.search(&query).into_iter().copied().collect();
        v.sort_unstable();
        v
    };

    let brute_hits: Vec<usize> = {
        let mut v: Vec<usize> = reference
            .iter()
            .filter(|(b, _)| b.intersects(&query))
            .map(|(_, i)| *i)
            .collect();
        v.sort_unstable();
        v
    };

    assert_eq!(
        tree_hits,
        brute_hits,
        "RTree3D search must match brute-force for 1000-point cloud: \
         tree_hits={}, brute_hits={}",
        tree_hits.len(),
        brute_hits.len()
    );
}

// ---------------------------------------------------------------------------
// RTree3D — Z-axis discriminator query
// ---------------------------------------------------------------------------

#[test]
fn test_rtree3d_z_axis_discriminator_query_only_returns_z_overlap() {
    let mut tree: RTree3D<&str> = RTree3D::new();

    // Both boxes share the same XY footprint but differ in Z.
    let low_z = Bbox3D::new(0.0, 0.0, 0.0, 5.0, 5.0, 2.0).unwrap();
    let high_z = Bbox3D::new(0.0, 0.0, 8.0, 5.0, 5.0, 10.0).unwrap();

    tree.insert(low_z, "low");
    tree.insert(high_z, "high");

    // Query that covers only the low-Z range [0..3] in Z.
    let query_low = Bbox3D::new(0.0, 0.0, 0.0, 5.0, 5.0, 3.0).unwrap();
    let hits_low = tree.search(&query_low);
    assert_eq!(hits_low.len(), 1, "only 'low' should be found for Z=[0..3]");
    assert_eq!(*hits_low[0], "low");

    // Query that covers only the high-Z range [7..11] in Z.
    let query_high = Bbox3D::new(0.0, 0.0, 7.0, 5.0, 5.0, 11.0).unwrap();
    let hits_high = tree.search(&query_high);
    assert_eq!(
        hits_high.len(),
        1,
        "only 'high' should be found for Z=[7..11]"
    );
    assert_eq!(*hits_high[0], "high");

    // Query that covers neither's Z range (gap [2..8]).
    let query_gap = Bbox3D::new(0.0, 0.0, 3.0, 5.0, 5.0, 7.0).unwrap();
    assert!(
        tree.search(&query_gap).is_empty(),
        "the Z gap [3..7] should return nothing"
    );

    // Query covering both Z ranges.
    let query_all = Bbox3D::new(0.0, 0.0, -1.0, 5.0, 5.0, 11.0).unwrap();
    assert_eq!(tree.search(&query_all).len(), 2);
}

// ---------------------------------------------------------------------------
// RTree3D — bulk-load returns all for world query
// ---------------------------------------------------------------------------

#[test]
fn test_rtree3d_bulk_load_returns_all_for_world_query() {
    let n = 500_usize;
    let items: Vec<(Bbox3D, usize)> = (0..n)
        .map(|i| {
            let x = pseudo_coord(i, 0.0, 80.0);
            let y = pseudo_coord(i, 100.0, 80.0);
            let z = pseudo_coord(i, 200.0, 80.0);
            let bbox = Bbox3D::new(x, y, z, x + 1.0, y + 1.0, z + 1.0).unwrap();
            (bbox, i)
        })
        .collect();

    let tree = RTree3D::bulk_load(items.clone());
    assert_eq!(tree.len(), n);
    assert!(!tree.is_empty());

    // A world-spanning query must return all entries.
    let world = Bbox3D::new(-1.0, -1.0, -1.0, 200.0, 200.0, 200.0).unwrap();
    let hits = tree.search(&world);
    assert_eq!(
        hits.len(),
        n,
        "bulk-loaded RTree3D must return all {n} entries for a world query"
    );

    // Spot query: brute-force check a known sub-region.
    let sub = Bbox3D::new(10.0, 10.0, 10.0, 30.0, 30.0, 30.0).unwrap();
    let tree_sub: Vec<usize> = {
        let mut v: Vec<usize> = tree.search(&sub).into_iter().copied().collect();
        v.sort_unstable();
        v
    };
    let brute_sub: Vec<usize> = {
        let mut v: Vec<usize> = items
            .iter()
            .filter(|(b, _)| b.intersects(&sub))
            .map(|(_, i)| *i)
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        tree_sub, brute_sub,
        "bulk-loaded tree sub-query must match brute force"
    );
}
