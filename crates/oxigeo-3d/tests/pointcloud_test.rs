//! Comprehensive tests for point cloud functionality

use approx::assert_relative_eq;
use oxigeo_3d::classification::*;
use oxigeo_3d::mesh::*;
use oxigeo_3d::pointcloud::*;
use oxigeo_3d::terrain::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_3d_pointcloud_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn test_point_creation_and_classification() {
    let mut point = Point::new(1.0, 2.0, 3.0);
    assert_eq!(point.classification, Classification::Unclassified);

    point.classification = Classification::Ground;
    assert!(point.is_ground());
    assert!(!point.is_building());
    assert!(!point.is_vegetation());
}

#[test]
fn test_point_distance_calculations() {
    let p1 = Point::new(0.0, 0.0, 0.0);
    let p2 = Point::new(3.0, 4.0, 0.0);

    assert_relative_eq!(p1.distance_2d(&p2), 5.0, epsilon = 0.0001);
    assert_relative_eq!(p1.distance_to(&p2), 5.0, epsilon = 0.0001);

    let p3 = Point::new(3.0, 4.0, 12.0);
    assert_relative_eq!(p1.distance_to(&p3), 13.0, epsilon = 0.0001);
}

#[test]
fn test_bounds_operations() {
    let bounds = Bounds3d::new(0.0, 10.0, 0.0, 10.0, 0.0, 10.0);

    assert!(bounds.contains(5.0, 5.0, 5.0));
    assert!(!bounds.contains(15.0, 5.0, 5.0));

    let bounds2 = Bounds3d::new(5.0, 15.0, 5.0, 15.0, 5.0, 15.0);
    assert!(bounds.intersects(&bounds2));

    let (cx, cy, cz) = bounds.center();
    assert_relative_eq!(cx, 5.0);
    assert_relative_eq!(cy, 5.0);
    assert_relative_eq!(cz, 5.0);
}

#[test]
fn test_point_cloud_filtering() {
    let _points = [
        Point::new(0.0, 0.0, 0.0),
        Point::new(1.0, 0.0, 1.0),
        Point::new(2.0, 0.0, 0.5),
    ];

    let mut cloud_point = Point::new(0.0, 0.0, 0.0);
    cloud_point.classification = Classification::Ground;
    let points_with_ground = vec![cloud_point];

    let header = LasHeader {
        version: "1.4".to_string(),
        point_format: PointFormat::Format0,
        point_count: points_with_ground.len() as u64,
        bounds: Bounds3d::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        scale: (0.01, 0.01, 0.01),
        offset: (0.0, 0.0, 0.0),
        system_identifier: "test".to_string(),
        generating_software: "test".to_string(),
    };

    let cloud = PointCloud::new(header, points_with_ground);
    let ground_points = cloud.ground_points();

    assert_eq!(ground_points.len(), 1);
}

#[test]
fn test_spatial_index_operations() {
    let points = vec![
        Point::new(0.0, 0.0, 0.0),
        Point::new(1.0, 1.0, 1.0),
        Point::new(2.0, 2.0, 2.0),
        Point::new(3.0, 3.0, 3.0),
    ];

    let index = SpatialIndex::new(points);

    // Test nearest neighbor
    let nearest = index.nearest(0.5, 0.5, 0.5);
    assert!(nearest.is_some());
    let nearest = nearest.expect("Nearest neighbor should be found");
    assert_relative_eq!(
        nearest.distance_to(&Point::new(0.5, 0.5, 0.5)),
        0.866,
        epsilon = 0.01
    );

    // Test k nearest neighbors
    let k_nearest = index.nearest_k(1.0, 1.0, 1.0, 2);
    assert_eq!(k_nearest.len(), 2);

    // Test within radius
    let within = index.within_radius(1.0, 1.0, 1.0, 2.0);
    assert!(!within.is_empty());
}

#[test]
fn test_tin_creation_and_operations() {
    let points = vec![
        Point::new(0.0, 0.0, 0.0),
        Point::new(1.0, 0.0, 1.0),
        Point::new(0.0, 1.0, 1.0),
        Point::new(1.0, 1.0, 2.0),
    ];

    let tin = create_tin(&points);
    assert!(tin.is_ok());

    let tin = tin.expect("TIN creation should succeed");
    assert_eq!(tin.point_count(), 4);
    assert!(tin.triangle_count() > 0);

    // Test elevation interpolation
    let z = tin.interpolate_elevation(0.5, 0.5);
    assert!(z.is_some());

    // Test bounds
    assert_relative_eq!(
        tin.min_elevation()
            .expect("Min elevation should be available"),
        0.0
    );
    assert_relative_eq!(
        tin.max_elevation()
            .expect("Max elevation should be available"),
        2.0
    );
}

#[test]
fn test_tin_to_mesh_conversion() {
    let points = vec![
        Point::new(0.0, 0.0, 0.0),
        Point::new(1.0, 0.0, 0.0),
        Point::new(0.0, 1.0, 0.0),
        Point::new(1.0, 1.0, 1.0),
    ];

    let tin = create_tin(&points).expect("TIN creation should succeed");
    let mesh = tin_to_mesh(&tin);

    assert!(mesh.is_ok());
    let mesh = mesh.expect("Mesh conversion should succeed");
    assert_eq!(mesh.vertex_count(), 4);
    assert!(mesh.triangle_count() > 0);
    assert!(mesh.validate().is_ok());
}

#[test]
fn test_mesh_normal_calculation() {
    let mut mesh = Mesh::new();

    mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
    mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
    mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
    mesh.add_triangle(0, 1, 2);

    mesh.calculate_normals();

    // Normals should point in +Z direction
    for vertex in &mesh.vertices {
        assert_relative_eq!(vertex.normal[2], 1.0, epsilon = 0.1);
    }
}

#[test]
fn test_classification_ground() {
    let points = vec![
        Point::new(0.0, 0.0, 0.0),
        Point::new(1.0, 0.0, 0.1),
        Point::new(2.0, 0.0, 0.05),
        Point::new(0.0, 1.0, 5.0), // High point
    ];

    let ground = classify_ground(&points);
    assert!(ground.is_ok());

    let ground = ground.expect("Ground classification should succeed");
    assert!(!ground.is_empty());
    assert!(ground.len() <= points.len());
}

#[test]
fn test_classification_noise_filtering() {
    let points = vec![
        Point::new(0.0, 0.0, 0.0),
        Point::new(0.05, 0.0, 0.0),
        Point::new(0.0, 0.05, 0.0),
        Point::new(100.0, 100.0, 100.0), // Isolated noise
    ];

    let filtered = filter_noise(&points);
    assert!(filtered.is_ok());

    let filtered = filtered.expect("Noise filtering should succeed");
    assert!(filtered.len() < points.len());
}

#[test]
fn test_dem_to_mesh() {
    use oxigeo_3d::terrain::dem_to_mesh::{Dem, DemMeshOptions};

    let elevations = vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    let bounds = [0.0, 0.0, 10.0, 10.0];

    let dem = Dem::new(3, 3, elevations, bounds).expect("DEM creation should succeed");
    let options = DemMeshOptions::default();
    let mesh = dem_to_mesh(&dem, &options);

    assert!(mesh.is_ok());
    let mesh = mesh.expect("DEM to mesh conversion should succeed");
    assert_eq!(mesh.vertex_count(), 9);
    assert!(mesh.triangle_count() > 0);
}

#[test]
fn test_dem_with_vertical_exaggeration() {
    use oxigeo_3d::terrain::dem_to_mesh::{Dem, DemMeshOptions};

    let elevations = vec![0.0, 1.0, 2.0, 3.0];
    let bounds = [0.0, 0.0, 10.0, 10.0];

    let dem = Dem::new(2, 2, elevations, bounds).expect("DEM creation should succeed");
    let options = DemMeshOptions::default().with_exaggeration(2.0);
    let mesh = dem_to_mesh(&dem, &options).expect("DEM to mesh conversion should succeed");

    // Check that Z values are exaggerated
    let max_z = mesh
        .vertices
        .iter()
        .map(|v| v.position[2])
        .max_by(|a, b| a.partial_cmp(b).expect("Float comparison should succeed"))
        .expect("Maximum Z value should be found");

    assert_relative_eq!(max_z, 6.0, epsilon = 0.001); // 3.0 * 2.0
}

#[test]
fn test_mesh_validation() {
    let mut mesh = Mesh::new();

    mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
    mesh.add_vertex(Vertex::new([1.0, 0.0, 0.0]));
    mesh.add_vertex(Vertex::new([0.0, 1.0, 0.0]));
    mesh.add_triangle(0, 1, 2);

    assert!(mesh.validate().is_ok());

    // Test invalid mesh
    let mut invalid_mesh = Mesh::new();
    invalid_mesh.add_vertex(Vertex::new([0.0, 0.0, 0.0]));
    invalid_mesh.add_triangle(0, 1, 2); // Invalid indices

    assert!(invalid_mesh.validate().is_err());
}

#[test]
fn test_integration_point_cloud_to_mesh_export() {
    let points = vec![
        Point::new(0.0, 0.0, 0.0),
        Point::new(1.0, 0.0, 0.5),
        Point::new(0.0, 1.0, 0.5),
        Point::new(1.0, 1.0, 1.0),
    ];

    // Create TIN from points
    let tin = create_tin(&points).expect("TIN creation should succeed");

    // Convert to mesh
    let mesh = tin_to_mesh(&tin).expect("TIN to mesh conversion should succeed");

    // Export to OBJ
    let obj_path = TempPath::new("test_integration.obj");
    let result = export_obj(&mesh, &obj_path);
    assert!(result.is_ok());

    // Export to GLB
    let glb_path = TempPath::new("test_integration.glb");
    let result = export_glb(&mesh, &glb_path);
    assert!(result.is_ok());
}

#[test]
fn test_color_support() {
    let mut point = Point::new(0.0, 0.0, 0.0);
    point.color = Some(ColorRgb {
        red: 255,
        green: 128,
        blue: 64,
    });

    assert!(point.color.is_some());
    let color = point.color.expect("Color should be set");
    assert_eq!(color.red, 255);
}
