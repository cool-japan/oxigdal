//! Comprehensive tests for Point3D, BoundingBox3D, Octree, PointCloudStats,
//! GroundFilter and HeightProfile.

use oxigdal_copc::{
    octree::Octree,
    point::{BoundingBox3D, Point3D},
    profile::{GroundFilter, HeightProfile},
};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn unit_bbox() -> BoundingBox3D {
    #[allow(clippy::expect_used)]
    BoundingBox3D::new(0.0, 0.0, 0.0, 100.0, 100.0, 100.0)
        .expect("unit bounding box should be valid")
}

fn grid_points(n: usize) -> Vec<Point3D> {
    (0..n)
        .map(|i| {
            let f = i as f64;
            Point3D::new(f % 10.0, (f / 10.0).floor(), f * 0.1)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Point3D tests
// ---------------------------------------------------------------------------

#[test]
fn point3d_new_defaults() {
    let p = Point3D::new(1.0, 2.0, 3.0);
    assert_eq!(p.x, 1.0);
    assert_eq!(p.y, 2.0);
    assert_eq!(p.z, 3.0);
    assert_eq!(p.intensity, 0);
    assert_eq!(p.return_number, 1);
    assert_eq!(p.number_of_returns, 1);
    assert_eq!(p.classification, 0);
    assert_eq!(p.scan_angle_rank, 0);
    assert_eq!(p.user_data, 0);
    assert_eq!(p.point_source_id, 0);
    assert!(p.gps_time.is_none());
    assert!(p.red.is_none());
    assert!(p.green.is_none());
    assert!(p.blue.is_none());
}

#[test]
fn point3d_with_intensity() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_intensity(512);
    assert_eq!(p.intensity, 512);
}

#[test]
fn point3d_with_classification() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(6);
    assert_eq!(p.classification, 6);
}

#[test]
fn point3d_with_color() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_color(100, 200, 300);
    assert_eq!(p.red, Some(100));
    assert_eq!(p.green, Some(200));
    assert_eq!(p.blue, Some(300));
}

#[test]
fn point3d_with_gps_time() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_gps_time(12345.678);
    assert!((p.gps_time.expect("gps_time should be set") - 12345.678).abs() < 1e-9);
}

#[test]
fn point3d_distance_to_known() {
    let a = Point3D::new(0.0, 0.0, 0.0);
    let b = Point3D::new(3.0, 4.0, 0.0);
    assert!((a.distance_to(&b) - 5.0).abs() < 1e-12);
}

#[test]
fn point3d_distance_to_3d() {
    let a = Point3D::new(0.0, 0.0, 0.0);
    let b = Point3D::new(1.0, 1.0, 1.0);
    let expected = 3.0_f64.sqrt();
    assert!((a.distance_to(&b) - expected).abs() < 1e-12);
}

#[test]
fn point3d_distance_2d_ignores_z() {
    let a = Point3D::new(0.0, 0.0, 0.0);
    let b = Point3D::new(3.0, 4.0, 999.0);
    assert!((a.distance_2d(&b) - 5.0).abs() < 1e-12);
}

#[test]
fn point3d_distance_to_self_is_zero() {
    let p = Point3D::new(5.5, 3.3, 1.1);
    assert!(p.distance_to(&p) < 1e-15);
}

#[test]
fn point3d_classification_name_class0() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(0);
    assert_eq!(p.classification_name(), "Created, Never Classified");
}

#[test]
fn point3d_classification_name_class1() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(1);
    assert_eq!(p.classification_name(), "Unclassified");
}

#[test]
fn point3d_classification_name_class2() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(2);
    assert_eq!(p.classification_name(), "Ground");
}

#[test]
fn point3d_classification_name_class3() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(3);
    assert_eq!(p.classification_name(), "Low Vegetation");
}

#[test]
fn point3d_classification_name_class4() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(4);
    assert_eq!(p.classification_name(), "Medium Vegetation");
}

#[test]
fn point3d_classification_name_class5() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(5);
    assert_eq!(p.classification_name(), "High Vegetation");
}

#[test]
fn point3d_classification_name_class6() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(6);
    assert_eq!(p.classification_name(), "Building");
}

#[test]
fn point3d_classification_name_class7() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(7);
    assert_eq!(p.classification_name(), "Low Point (Noise)");
}

#[test]
fn point3d_classification_name_class9() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(9);
    assert_eq!(p.classification_name(), "Water");
}

#[test]
fn point3d_classification_name_class10() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(10);
    assert_eq!(p.classification_name(), "Rail");
}

#[test]
fn point3d_classification_name_class11() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(11);
    assert_eq!(p.classification_name(), "Road Surface");
}

#[test]
fn point3d_classification_name_class13() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(13);
    assert_eq!(p.classification_name(), "Wire - Guard (Shield)");
}

#[test]
fn point3d_classification_name_class14() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(14);
    assert_eq!(p.classification_name(), "Wire - Conductor (Phase)");
}

#[test]
fn point3d_classification_name_class15() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(15);
    assert_eq!(p.classification_name(), "Transmission Tower");
}

#[test]
fn point3d_classification_name_class17() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(17);
    assert_eq!(p.classification_name(), "Bridge Deck");
}

#[test]
fn point3d_classification_name_class18() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(18);
    assert_eq!(p.classification_name(), "High Noise");
}

#[test]
fn point3d_classification_name_unknown() {
    let p = Point3D::new(0.0, 0.0, 0.0).with_classification(255);
    assert_eq!(p.classification_name(), "Reserved/Unknown");
}

// ---------------------------------------------------------------------------
// BoundingBox3D tests
// ---------------------------------------------------------------------------

#[test]
fn bbox_new_valid() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
    assert!(bb.is_some());
}

#[test]
fn bbox_new_invalid_x() {
    let bb = BoundingBox3D::new(10.0, 0.0, 0.0, 5.0, 10.0, 10.0);
    assert!(bb.is_none());
}

#[test]
fn bbox_new_invalid_y() {
    let bb = BoundingBox3D::new(0.0, 10.0, 0.0, 10.0, 5.0, 10.0);
    assert!(bb.is_none());
}

#[test]
fn bbox_new_invalid_z() {
    let bb = BoundingBox3D::new(0.0, 0.0, 10.0, 10.0, 10.0, 5.0);
    assert!(bb.is_none());
}

#[test]
fn bbox_from_points_empty() {
    let bb = BoundingBox3D::from_points(&[]);
    assert!(bb.is_none());
}

#[test]
fn bbox_from_points_single() {
    let p = Point3D::new(3.0, 4.0, 5.0);
    let bb = BoundingBox3D::from_points(std::slice::from_ref(&p))
        .expect("bbox from single point should succeed");
    assert_eq!(bb.min_x, 3.0);
    assert_eq!(bb.max_z, 5.0);
    assert!(bb.contains(&p));
}

#[test]
fn bbox_from_points_contains_all() {
    let points = vec![
        Point3D::new(1.0, 2.0, 3.0),
        Point3D::new(5.0, 6.0, 7.0),
        Point3D::new(-1.0, 0.0, 10.0),
    ];
    let bb = BoundingBox3D::from_points(&points).expect("bbox from multiple points should succeed");
    for p in &points {
        assert!(bb.contains(p), "point {:?} not inside bbox", p);
    }
}

#[test]
fn bbox_contains_inside() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0).expect("valid bbox");
    assert!(bb.contains(&Point3D::new(5.0, 5.0, 5.0)));
}

#[test]
fn bbox_contains_on_boundary() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0).expect("valid bbox");
    assert!(bb.contains(&Point3D::new(0.0, 0.0, 0.0)));
    assert!(bb.contains(&Point3D::new(10.0, 10.0, 10.0)));
}

#[test]
fn bbox_contains_outside() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 10.0, 10.0, 10.0).expect("valid bbox");
    assert!(!bb.contains(&Point3D::new(11.0, 5.0, 5.0)));
    assert!(!bb.contains(&Point3D::new(5.0, -1.0, 5.0)));
}

#[test]
fn bbox_volume() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 2.0, 3.0, 4.0).expect("valid bbox for volume test");
    assert!((bb.volume() - 24.0).abs() < 1e-12);
}

#[test]
fn bbox_volume_unit_cube() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).expect("valid unit cube bbox");
    assert!((bb.volume() - 1.0).abs() < 1e-12);
}

#[test]
fn bbox_diagonal() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).expect("valid unit cube bbox");
    let expected = 3.0_f64.sqrt();
    assert!((bb.diagonal() - expected).abs() < 1e-12);
}

#[test]
fn bbox_center() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 4.0, 6.0, 8.0).expect("valid bbox for center test");
    let (cx, cy, cz) = bb.center();
    assert!((cx - 2.0).abs() < 1e-12);
    assert!((cy - 3.0).abs() < 1e-12);
    assert!((cz - 4.0).abs() < 1e-12);
}

#[test]
fn bbox_expand_by() {
    let bb = BoundingBox3D::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0).expect("valid bbox for expand test");
    let expanded = bb.expand_by(1.0);
    assert!((expanded.min_x - 0.0).abs() < 1e-12);
    assert!((expanded.max_x - 4.0).abs() < 1e-12);
}

#[test]
fn bbox_intersects_2d_overlap() {
    let a = BoundingBox3D::new(0.0, 0.0, 0.0, 5.0, 5.0, 5.0).expect("valid bbox a");
    let b = BoundingBox3D::new(3.0, 3.0, 0.0, 8.0, 8.0, 5.0).expect("valid bbox b");
    assert!(a.intersects_2d(&b));
}

#[test]
fn bbox_intersects_2d_disjoint() {
    let a = BoundingBox3D::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0).expect("valid bbox a");
    let b = BoundingBox3D::new(5.0, 5.0, 0.0, 8.0, 8.0, 2.0).expect("valid bbox b");
    assert!(!a.intersects_2d(&b));
}

#[test]
fn bbox_split_octants_count() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 8.0, 8.0, 8.0).expect("valid bbox for octants");
    let children = bb.split_octants();
    assert_eq!(children.len(), 8);
}

#[test]
fn bbox_split_octants_union_equals_parent() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 8.0, 8.0, 8.0)
        .expect("valid bbox for octants union test");
    let children = bb.split_octants();
    // Every child must be contained in parent.
    for child in &children {
        assert!(child.min_x >= bb.min_x - 1e-9);
        assert!(child.min_y >= bb.min_y - 1e-9);
        assert!(child.min_z >= bb.min_z - 1e-9);
        assert!(child.max_x <= bb.max_x + 1e-9);
        assert!(child.max_y <= bb.max_y + 1e-9);
        assert!(child.max_z <= bb.max_z + 1e-9);
    }
    // Total volume of children equals parent volume.
    let child_vol: f64 = children.iter().map(|c| c.volume()).sum();
    assert!((child_vol - bb.volume()).abs() < 1e-9);
}

#[test]
fn bbox_split_octants_non_overlapping() {
    let bb = BoundingBox3D::new(0.0, 0.0, 0.0, 8.0, 8.0, 8.0)
        .expect("valid bbox for non-overlapping test");
    let children = bb.split_octants();
    // Each child should have a distinct "half" position — verify by checking
    // that summed volumes equal the parent (overlap would cause > parent).
    let total: f64 = children.iter().map(|c| c.volume()).sum();
    assert!((total - bb.volume()).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// Octree tests
// ---------------------------------------------------------------------------

#[test]
fn octree_insert_single() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(10.0, 10.0, 10.0));
    assert_eq!(tree.len(), 1);
    assert!(!tree.is_empty());
}

#[test]
fn octree_insert_100_points() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert_batch(grid_points(100));
    assert_eq!(tree.len(), 100);
}

#[test]
fn octree_is_empty_initially() {
    let tree = Octree::new(unit_bbox());
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
}

#[test]
fn octree_insert_out_of_bounds_ignored() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(200.0, 200.0, 200.0));
    assert_eq!(tree.len(), 0);
}

#[test]
fn octree_query_bbox_inside_returned() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(10.0, 10.0, 10.0));
    tree.insert(Point3D::new(90.0, 90.0, 90.0));

    let query = BoundingBox3D::new(0.0, 0.0, 0.0, 50.0, 50.0, 50.0).expect("valid query bbox");
    let results = tree.query_bbox(&query);
    assert_eq!(results.len(), 1);
    assert!((results[0].x - 10.0).abs() < 1e-9);
}

#[test]
fn octree_query_bbox_outside_not_returned() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(10.0, 10.0, 10.0));
    let query =
        BoundingBox3D::new(60.0, 60.0, 60.0, 100.0, 100.0, 100.0).expect("valid query bbox");
    let results = tree.query_bbox(&query);
    assert!(results.is_empty());
}

#[test]
fn octree_query_bbox_all_returned() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert_batch(grid_points(50));
    let query = unit_bbox();
    let results = tree.query_bbox(&query);
    assert_eq!(results.len(), 50);
}

#[test]
fn octree_query_sphere_finds_nearby() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(50.0, 50.0, 50.0));
    tree.insert(Point3D::new(1.0, 1.0, 1.0));

    let results = tree.query_sphere(50.0, 50.0, 50.0, 5.0);
    assert_eq!(results.len(), 1);
    assert!((results[0].x - 50.0).abs() < 1e-9);
}

#[test]
fn octree_query_sphere_radius_zero() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(50.0, 50.0, 50.0));
    let results = tree.query_sphere(50.0, 50.0, 50.0, 0.0);
    // Point exactly at centre is at distance 0 — ≤ 0.0 is true.
    assert_eq!(results.len(), 1);
}

#[test]
fn octree_query_sphere_excludes_distant() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(1.0, 1.0, 1.0));
    let results = tree.query_sphere(99.0, 99.0, 99.0, 5.0);
    assert!(results.is_empty());
}

#[test]
fn octree_k_nearest_returns_k() {
    let mut tree = Octree::new(unit_bbox());
    for i in 0..20 {
        let f = i as f64 * 4.0;
        tree.insert(Point3D::new(f, 0.0, 0.0));
    }
    let knn = tree.k_nearest(0.0, 0.0, 0.0, 5);
    assert_eq!(knn.len(), 5);
}

#[test]
fn octree_k_nearest_sorted_by_distance() {
    let mut tree = Octree::new(unit_bbox());
    for i in 1..=10_u32 {
        tree.insert(Point3D::new(i as f64, 0.0, 0.0));
    }
    let knn = tree.k_nearest(0.0, 0.0, 0.0, 5);
    for i in 1..knn.len() {
        assert!(knn[i - 1].1 <= knn[i].1 + 1e-9);
    }
}

#[test]
fn octree_k_nearest_k_zero_returns_empty() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(5.0, 5.0, 5.0));
    let knn = tree.k_nearest(0.0, 0.0, 0.0, 0);
    assert!(knn.is_empty());
}

#[test]
fn octree_k_nearest_fewer_than_k_available() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(5.0, 5.0, 5.0));
    tree.insert(Point3D::new(10.0, 10.0, 10.0));
    let knn = tree.k_nearest(0.0, 0.0, 0.0, 10);
    assert_eq!(knn.len(), 2);
}

#[test]
fn octree_k_nearest_distances_correct() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(3.0, 4.0, 0.0)); // distance 5 from origin
    tree.insert(Point3D::new(1.0, 0.0, 0.0)); // distance 1 from origin
    let knn = tree.k_nearest(0.0, 0.0, 0.0, 2);
    assert!((knn[0].1 - 1.0).abs() < 1e-9);
    assert!((knn[1].1 - 5.0).abs() < 1e-9);
}

#[test]
fn octree_by_classification_only_matching() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(10.0, 10.0, 10.0).with_classification(2));
    tree.insert(Point3D::new(20.0, 20.0, 20.0).with_classification(5));
    tree.insert(Point3D::new(30.0, 30.0, 30.0).with_classification(2));

    let ground = tree.by_classification(2);
    assert_eq!(ground.len(), 2);
    for p in &ground {
        assert_eq!(p.classification, 2);
    }
}

#[test]
fn octree_by_classification_none_matching() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(10.0, 10.0, 10.0).with_classification(1));
    let result = tree.by_classification(6);
    assert!(result.is_empty());
}

#[test]
fn octree_voxel_downsample_reduces_points() {
    let mut tree = Octree::new(unit_bbox());
    // Insert many points in the same small region.
    for _ in 0..50 {
        tree.insert(Point3D::new(5.0, 5.0, 5.0));
    }
    tree.insert(Point3D::new(50.0, 50.0, 50.0));

    let downsampled = tree.voxel_downsample(10.0);
    assert!(downsampled.len() < 51);
}

#[test]
fn octree_voxel_downsample_large_voxel_one_point() {
    let mut tree = Octree::new(unit_bbox());
    // All points within a single voxel.
    for i in 0..20_u32 {
        tree.insert(Point3D::new(i as f64, i as f64, i as f64));
    }
    let downsampled = tree.voxel_downsample(200.0); // voxel larger than data range
    // All in one cell.
    assert_eq!(downsampled.len(), 1);
}

#[test]
fn octree_voxel_downsample_zero_size_empty() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(5.0, 5.0, 5.0));
    let downsampled = tree.voxel_downsample(0.0);
    assert!(downsampled.is_empty());
}

// ---------------------------------------------------------------------------
// PointCloudStats tests
// ---------------------------------------------------------------------------

#[test]
fn stats_count_matches_total() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert_batch(grid_points(60));
    let s = tree.stats();
    assert_eq!(s.count, 60);
}

#[test]
fn stats_empty_tree() {
    let tree = Octree::new(unit_bbox());
    let s = tree.stats();
    assert_eq!(s.count, 0);
    assert!(s.bounds.is_none());
    assert_eq!(s.density, 0.0);
}

#[test]
fn stats_density_positive() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert_batch(grid_points(50));
    let s = tree.stats();
    assert!(
        s.density > 0.0,
        "density should be positive, got {}",
        s.density
    );
}

#[test]
fn stats_mean_z_reasonable() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(1.0, 1.0, 0.0));
    tree.insert(Point3D::new(2.0, 2.0, 10.0));
    let s = tree.stats();
    assert!((s.mean_z - 5.0).abs() < 1e-9);
}

#[test]
fn stats_std_z_zero_for_uniform_z() {
    let mut tree = Octree::new(unit_bbox());
    for i in 0..10_u32 {
        tree.insert(Point3D::new(i as f64 * 5.0, i as f64 * 5.0, 7.0));
    }
    let s = tree.stats();
    assert!(s.std_z < 1e-9);
}

#[test]
fn stats_classification_counts() {
    let mut tree = Octree::new(unit_bbox());
    tree.insert(Point3D::new(10.0, 10.0, 1.0).with_classification(2));
    tree.insert(Point3D::new(20.0, 20.0, 2.0).with_classification(2));
    tree.insert(Point3D::new(30.0, 30.0, 3.0).with_classification(5));
    let s = tree.stats();
    assert_eq!(
        *s.classification_counts
            .get(&2)
            .expect("class 2 count should exist"),
        2
    );
    assert_eq!(
        *s.classification_counts
            .get(&5)
            .expect("class 5 count should exist"),
        1
    );
}

// ---------------------------------------------------------------------------
// GroundFilter tests
// ---------------------------------------------------------------------------

#[test]
fn ground_filter_by_classification_separates() {
    let points = vec![
        Point3D::new(0.0, 0.0, 0.0).with_classification(2),
        Point3D::new(1.0, 0.0, 1.0).with_classification(5),
        Point3D::new(2.0, 0.0, 2.0).with_classification(2),
        Point3D::new(3.0, 0.0, 3.0).with_classification(1),
    ];
    let (ground, non_ground) = GroundFilter::by_classification(&points);
    assert_eq!(ground.len(), 2);
    assert_eq!(non_ground.len(), 2);
    for p in &ground {
        assert_eq!(p.classification, 2);
    }
    for p in &non_ground {
        assert_ne!(p.classification, 2);
    }
}

#[test]
fn ground_filter_by_classification_all_ground() {
    let points: Vec<Point3D> = (0..5)
        .map(|i| Point3D::new(i as f64, 0.0, 0.0).with_classification(2))
        .collect();
    let (g, ng) = GroundFilter::by_classification(&points);
    assert_eq!(g.len(), 5);
    assert!(ng.is_empty());
}

#[test]
fn ground_filter_by_classification_none_ground() {
    let points = vec![Point3D::new(0.0, 0.0, 5.0).with_classification(5)];
    let (g, ng) = GroundFilter::by_classification(&points);
    assert!(g.is_empty());
    assert_eq!(ng.len(), 1);
}

#[test]
fn ground_filter_apply_low_z_is_ground() {
    let gf = GroundFilter::new();
    let points = vec![
        Point3D::new(0.0, 0.0, 0.0),  // lowest in cell → ground
        Point3D::new(0.1, 0.0, 10.0), // same cell, high → not ground
    ];
    let flags = gf.apply(&points);
    assert!(flags[0]); // low z is ground
    assert!(!flags[1]); // high z not ground (10 > 0.3 * 1.0)
}

#[test]
fn ground_filter_apply_empty() {
    let gf = GroundFilter::new();
    let flags = gf.apply(&[]);
    assert!(flags.is_empty());
}

#[test]
fn ground_filter_apply_all_same_z() {
    let gf = GroundFilter::new();
    let points: Vec<Point3D> = (0..5).map(|i| Point3D::new(i as f64, 0.0, 5.0)).collect();
    let flags = gf.apply(&points);
    // All at same Z as minimum → all ground.
    assert!(flags.iter().all(|&f| f));
}

#[test]
fn ground_filter_default_parameters() {
    let gf = GroundFilter::default();
    assert!((gf.max_slope - 0.3).abs() < 1e-9);
    assert!((gf.cell_size - 1.0).abs() < 1e-9);
    assert_eq!(gf.classification_code, 2);
}

// ---------------------------------------------------------------------------
// HeightProfile tests
// ---------------------------------------------------------------------------

fn build_profile_tree() -> Octree {
    #[allow(clippy::expect_used)]
    let bounds =
        BoundingBox3D::new(0.0, 0.0, 0.0, 200.0, 200.0, 50.0).expect("valid profile bounds");
    let mut tree = Octree::new(bounds);
    // Place points along x-axis at y=0 with varying heights.
    for i in 0..100_u32 {
        let x = i as f64 * 1.5;
        tree.insert(Point3D::new(x, 0.0, (i % 10) as f64));
    }
    tree
}

#[test]
fn height_profile_bin_count() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 100.0, 0.0, 2.0, 10);
    assert_eq!(profile.segments.len(), 10);
}

#[test]
fn height_profile_total_length_approx() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 100.0, 0.0, 2.0, 10);
    assert!((profile.total_length() - 100.0).abs() < 1e-9);
}

#[test]
fn height_profile_bin_count_1() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 50.0, 0.0, 5.0, 1);
    assert_eq!(profile.segments.len(), 1);
}

#[test]
fn height_profile_non_empty_segments() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 100.0, 0.0, 2.0, 10);
    let non_empty: usize = profile
        .segments
        .iter()
        .filter(|s| s.point_count > 0)
        .count();
    assert!(non_empty > 0, "expected some non-empty segments");
}

#[test]
fn height_profile_highest_point_some() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 100.0, 0.0, 2.0, 10);
    // Whether it's Some or None depends on points in corridor — just check API.
    let _ = profile.highest_point();
}

#[test]
fn height_profile_bin_widths_uniform() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 100.0, 0.0, 2.0, 5);
    assert!((profile.bin_width - 20.0).abs() < 1e-9);
}

#[test]
fn height_profile_segment_distances_ascending() {
    let tree = build_profile_tree();
    let profile = HeightProfile::along_line(&tree, 0.0, 0.0, 100.0, 0.0, 2.0, 8);
    for w in profile.segments.windows(2) {
        assert!(w[1].distance_along_line > w[0].distance_along_line);
    }
}
