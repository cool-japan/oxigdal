//! Point cloud classification algorithms
//!
//! Provides algorithms for automatic point cloud classification including:
//! - Ground classification
//! - Vegetation classification
//! - Building extraction
//! - Noise filtering

use crate::error::{Error, Result};
use crate::pointcloud::{Classification, Point, PointCloud, SpatialIndex};
use rayon::prelude::*;

/// Classification parameters
#[derive(Debug, Clone)]
pub struct ClassificationParams {
    /// Maximum distance for neighbor search (meters)
    pub search_radius: f64,
    /// Minimum points for classification
    pub min_points: usize,
    /// Ground height threshold (meters)
    pub ground_threshold: f64,
    /// Vegetation height range (min, max) in meters
    pub vegetation_range: (f64, f64),
    /// Building height threshold (meters)
    pub building_height: f64,
    /// Noise distance threshold (meters)
    pub noise_threshold: f64,
}

impl Default for ClassificationParams {
    fn default() -> Self {
        Self {
            search_radius: 2.0,
            min_points: 5,
            ground_threshold: 0.5,
            vegetation_range: (0.5, 30.0),
            building_height: 3.0,
            noise_threshold: 0.1,
        }
    }
}

/// Classify ground points using a progressive morphological filter
pub fn classify_ground(points: &[Point]) -> Result<Vec<Point>> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points to classify".to_string()));
    }

    let params = ClassificationParams::default();
    classify_ground_with_params(points, &params)
}

/// Classify ground points with custom parameters
pub fn classify_ground_with_params(
    points: &[Point],
    params: &ClassificationParams,
) -> Result<Vec<Point>> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points to classify".to_string()));
    }

    // Build spatial index
    let index = SpatialIndex::new(points.to_vec());

    // Find potential ground points (lowest points in neighborhoods)
    let ground_points: Vec<Point> = points
        .par_iter()
        .filter_map(|point| {
            // Find neighbors
            let neighbors = index.within_radius(point.x, point.y, point.z, params.search_radius);

            if neighbors.is_empty() {
                return None;
            }

            // Calculate local minimum height
            let min_z = neighbors
                .iter()
                .map(|p| p.z)
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(point.z);

            // Classify as ground if close to local minimum
            if (point.z - min_z).abs() <= params.ground_threshold {
                let mut ground_point = (*point).clone();
                ground_point.classification = Classification::Ground;
                Some(ground_point)
            } else {
                None
            }
        })
        .collect();

    Ok(ground_points)
}

/// Classify vegetation points based on height above ground
pub fn classify_vegetation(points: &[Point], ground_points: &[Point]) -> Result<Vec<Point>> {
    let params = ClassificationParams::default();
    classify_vegetation_with_params(points, ground_points, &params)
}

/// Classify vegetation with custom parameters
pub fn classify_vegetation_with_params(
    points: &[Point],
    ground_points: &[Point],
    params: &ClassificationParams,
) -> Result<Vec<Point>> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points to classify".to_string()));
    }

    if ground_points.is_empty() {
        return Err(Error::EmptyDataset("No ground points provided".to_string()));
    }

    // Build spatial index for ground points
    let ground_index = SpatialIndex::new(ground_points.to_vec());

    let vegetation_points: Vec<Point> = points
        .par_iter()
        .filter_map(|point| {
            // Find nearest ground point
            if let Some(ground) = ground_index.nearest(point.x, point.y, point.z) {
                let height_above_ground = point.z - ground.z;

                // Classify based on height above ground
                let classification = if height_above_ground >= params.vegetation_range.0
                    && height_above_ground < params.vegetation_range.1
                {
                    if height_above_ground < 2.0 {
                        Classification::LowVegetation
                    } else if height_above_ground < 10.0 {
                        Classification::MediumVegetation
                    } else {
                        Classification::HighVegetation
                    }
                } else {
                    return None;
                };

                let mut veg_point = (*point).clone();
                veg_point.classification = classification;
                Some(veg_point)
            } else {
                None
            }
        })
        .collect();

    Ok(vegetation_points)
}

/// Extract building points based on planarity and height
pub fn extract_buildings(points: &[Point], ground_points: &[Point]) -> Result<Vec<Point>> {
    let params = ClassificationParams::default();
    extract_buildings_with_params(points, ground_points, &params)
}

/// Extract buildings with custom parameters
pub fn extract_buildings_with_params(
    points: &[Point],
    ground_points: &[Point],
    params: &ClassificationParams,
) -> Result<Vec<Point>> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points to classify".to_string()));
    }

    if ground_points.is_empty() {
        return Err(Error::EmptyDataset("No ground points provided".to_string()));
    }

    // Build spatial indices
    let point_index = SpatialIndex::new(points.to_vec());
    let ground_index = SpatialIndex::new(ground_points.to_vec());

    let building_points: Vec<Point> = points
        .par_iter()
        .filter_map(|point| {
            // Find nearest ground point
            if let Some(ground) = ground_index.nearest(point.x, point.y, point.z) {
                let height_above_ground = point.z - ground.z;

                // Must be above building height threshold
                if height_above_ground < params.building_height {
                    return None;
                }

                // Find neighbors
                let neighbors =
                    point_index.within_radius(point.x, point.y, point.z, params.search_radius);

                if neighbors.len() < params.min_points {
                    return None;
                }

                // Check planarity (buildings tend to have planar surfaces)
                let planarity = calculate_planarity(&neighbors);

                if planarity > 0.8 {
                    // High planarity suggests building
                    let mut building_point = (*point).clone();
                    building_point.classification = Classification::Building;
                    Some(building_point)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    Ok(building_points)
}

/// Filter noise points (isolated points)
pub fn filter_noise(points: &[Point]) -> Result<Vec<Point>> {
    let params = ClassificationParams::default();
    filter_noise_with_params(points, &params)
}

/// Filter noise with custom parameters
pub fn filter_noise_with_params(
    points: &[Point],
    params: &ClassificationParams,
) -> Result<Vec<Point>> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points to filter".to_string()));
    }

    let index = SpatialIndex::new(points.to_vec());

    let filtered_points: Vec<Point> = points
        .par_iter()
        .filter_map(|point| {
            // Find neighbors
            let neighbors = index.within_radius(point.x, point.y, point.z, params.noise_threshold);

            // Keep point if it has enough neighbors (noise points are filtered out)
            if neighbors.len() >= params.min_points {
                Some((*point).clone())
            } else {
                // Filter out noise points (isolated points)
                None
            }
        })
        .collect();

    Ok(filtered_points)
}

/// Eigenvalues of a symmetric 3×3 matrix, sorted descending.
/// Closed-form analytic solution (Smith 1961). Returns [0.0; 3] for non-finite input.
fn symmetric_eig_3x3(cov: &[[f64; 3]; 3]) -> [f64; 3] {
    // non-finite guard
    for row in cov {
        for v in row {
            if !v.is_finite() {
                return [0.0; 3];
            }
        }
    }
    let p1 = cov[0][1].powi(2) + cov[0][2].powi(2) + cov[1][2].powi(2);
    if p1 <= f64::EPSILON {
        // diagonal matrix — eigenvalues are the diagonal, sorted descending
        let mut d = [cov[0][0], cov[1][1], cov[2][2]];
        d.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        return d;
    }
    let q = (cov[0][0] + cov[1][1] + cov[2][2]) / 3.0;
    let p2 = (cov[0][0] - q).powi(2) + (cov[1][1] - q).powi(2) + (cov[2][2] - q).powi(2) + 2.0 * p1;
    let p = (p2 / 6.0).sqrt();
    if p <= f64::EPSILON {
        return [q, q, q];
    }
    // B = (1/p) * (cov - q*I)
    let mut b = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let a = cov[i][j] - if i == j { q } else { 0.0 };
            b[i][j] = a / p;
        }
    }
    // r = det(B) / 2
    let det_b = b[0][0] * (b[1][1] * b[2][2] - b[1][2] * b[2][1])
        - b[0][1] * (b[1][0] * b[2][2] - b[1][2] * b[2][0])
        + b[0][2] * (b[1][0] * b[2][1] - b[1][1] * b[2][0]);
    let r = (det_b / 2.0).clamp(-1.0, 1.0);
    let phi = r.acos() / 3.0;
    let eig1 = q + 2.0 * p * phi.cos();
    let eig3 = q + 2.0 * p * (phi + 2.0 * std::f64::consts::PI / 3.0).cos();
    let eig2 = 3.0 * q - eig1 - eig3;
    // eig1 >= eig2 >= eig3 by construction of this method
    [eig1, eig2, eig3]
}

/// PCA dimensionality features (Demantké et al. 2011).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DimensionalityFeatures {
    /// Linearity: dominance of the largest eigenvalue (1D structure).
    pub linearity: f64,
    /// Planarity: dominance of the two largest eigenvalues (2D structure).
    pub planarity: f64,
    /// Sphericity: isotropy of the eigenvalue spectrum (3D structure).
    pub sphericity: f64,
}

/// Compute linearity / planarity / sphericity from a 3×3 covariance matrix.
///
/// Implements the dimensionality index of Demantké et al. 2011,
/// "Dimensionality based scale selection in 3D LiDAR point clouds".
/// For eigenvalues `λ₁ ≥ λ₂ ≥ λ₃`:
/// - linearity  `L_λ = (λ₁ - λ₂) / λ₁`
/// - planarity  `P_λ = (λ₂ - λ₃) / λ₁`
/// - sphericity `S_λ = λ₃ / λ₁`
///
/// Degenerate cluster (λ₁ ≈ 0) → all-zero features.
pub(crate) fn dimensionality_features(cov: &[[f64; 3]; 3]) -> DimensionalityFeatures {
    let [l1, l2, l3] = symmetric_eig_3x3(cov);
    if l1 <= f64::EPSILON {
        return DimensionalityFeatures {
            linearity: 0.0,
            planarity: 0.0,
            sphericity: 0.0,
        };
    }
    DimensionalityFeatures {
        linearity: ((l1 - l2) / l1).clamp(0.0, 1.0),
        planarity: ((l2 - l3) / l1).clamp(0.0, 1.0),
        sphericity: (l3 / l1).clamp(0.0, 1.0),
    }
}

/// Calculate planarity of a point set (0 = not planar, 1 = perfectly planar)
fn calculate_planarity(points: &[&Point]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }

    // Calculate centroid
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_z = 0.0;

    for point in points {
        sum_x += point.x;
        sum_y += point.y;
        sum_z += point.z;
    }

    let n = points.len() as f64;
    let centroid = [sum_x / n, sum_y / n, sum_z / n];

    // Calculate covariance matrix
    let mut cov = [[0.0; 3]; 3];

    for point in points {
        let dx = point.x - centroid[0];
        let dy = point.y - centroid[1];
        let dz = point.z - centroid[2];

        cov[0][0] += dx * dx;
        cov[0][1] += dx * dy;
        cov[0][2] += dx * dz;
        cov[1][1] += dy * dy;
        cov[1][2] += dy * dz;
        cov[2][2] += dz * dz;
    }

    cov[1][0] = cov[0][1];
    cov[2][0] = cov[0][2];
    cov[2][1] = cov[1][2];

    for row in &mut cov {
        for val in row {
            *val /= n;
        }
    }

    // Planarity from the PCA dimensionality index (Demantké et al. 2011):
    // P_λ = (λ₂ - λ₃) / λ₁ for the local covariance eigenvalues λ₁ ≥ λ₂ ≥ λ₃.
    dimensionality_features(&cov).planarity
}

/// Automatic classification pipeline
pub fn auto_classify(points: &[Point]) -> Result<PointCloud> {
    let params = ClassificationParams::default();
    auto_classify_with_params(points, &params)
}

/// Automatic classification with custom parameters
pub fn auto_classify_with_params(
    points: &[Point],
    params: &ClassificationParams,
) -> Result<PointCloud> {
    if points.is_empty() {
        return Err(Error::EmptyDataset("No points to classify".to_string()));
    }

    // Step 1: Filter noise
    let filtered = filter_noise_with_params(points, params)?;

    // Step 2: Classify ground
    let ground = classify_ground_with_params(&filtered, params)?;

    // Step 3: Classify vegetation
    let vegetation = classify_vegetation_with_params(&filtered, &ground, params)?;

    // Step 4: Extract buildings
    let buildings = extract_buildings_with_params(&filtered, &ground, params)?;

    // Combine all classified points
    let mut classified_points = Vec::new();
    classified_points.extend(ground);
    classified_points.extend(vegetation);
    classified_points.extend(buildings);

    // Add remaining unclassified points
    for point in &filtered {
        if !classified_points.iter().any(|p| {
            (p.x - point.x).abs() < 1e-6
                && (p.y - point.y).abs() < 1e-6
                && (p.z - point.z).abs() < 1e-6
        }) {
            classified_points.push(point.clone());
        }
    }

    // Create point cloud (simplified header)
    use crate::pointcloud::{Bounds3d, LasHeader, PointFormat};

    // Calculate bounds
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;

    for point in &classified_points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
        min_z = min_z.min(point.z);
        max_z = max_z.max(point.z);
    }

    let header = LasHeader {
        version: "1.4".to_string(),
        point_format: PointFormat::Format0,
        point_count: classified_points.len() as u64,
        bounds: Bounds3d::new(min_x, max_x, min_y, max_y, min_z, max_z),
        scale: (0.01, 0.01, 0.01),
        offset: (0.0, 0.0, 0.0),
        system_identifier: "OxiGeo".to_string(),
        generating_software: "oxigeo-3d classification".to_string(),
    };

    Ok(PointCloud::new(header, classified_points))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_ground_simple() {
        let points = vec![
            Point::new(0.0, 0.0, 0.0),
            Point::new(1.0, 0.0, 0.1),
            Point::new(0.0, 1.0, 5.0), // High point (not ground)
        ];

        let ground = classify_ground(&points);
        assert!(ground.is_ok());

        let ground = ground.expect("Ground classification should succeed with valid points");
        assert!(!ground.is_empty());
        assert!(ground.iter().all(|p| p.is_ground()));
    }

    #[test]
    fn test_filter_noise() {
        let points = vec![
            Point::new(0.0, 0.0, 0.0),
            Point::new(0.1, 0.0, 0.0),
            Point::new(0.0, 0.1, 0.0),
            Point::new(100.0, 100.0, 100.0), // Isolated noise point
        ];

        let filtered = filter_noise(&points);
        assert!(filtered.is_ok());

        let filtered = filtered.expect("Noise filtering should succeed with valid points");
        // Should have fewer points after filtering noise
        assert!(filtered.len() < points.len());
    }

    #[test]
    fn test_calculate_planarity() {
        // Perfect plane (Z = 0)
        let p1 = Point::new(0.0, 0.0, 0.0);
        let p2 = Point::new(1.0, 0.0, 0.0);
        let p3 = Point::new(0.0, 1.0, 0.0);
        let p4 = Point::new(1.0, 1.0, 0.0);
        let planar_points = vec![&p1, &p2, &p3, &p4];

        let planarity = calculate_planarity(&planar_points);
        assert!(planarity > 0.5); // Should be high for planar points

        // Non-planar points
        let np1 = Point::new(0.0, 0.0, 0.0);
        let np2 = Point::new(1.0, 0.0, 1.0);
        let np3 = Point::new(0.0, 1.0, 2.0);
        let non_planar = vec![&np1, &np2, &np3];

        let planarity2 = calculate_planarity(&non_planar);
        assert!(planarity2 < planarity);
    }

    #[test]
    fn test_classification_params() {
        let params = ClassificationParams::default();
        assert_eq!(params.search_radius, 2.0);
        assert_eq!(params.min_points, 5);
    }

    /// Build the symmetric 3×3 covariance matrix of a point set the same way
    /// `calculate_planarity` does — used to exercise the dimensionality index.
    fn covariance_of(points: &[Point]) -> [[f64; 3]; 3] {
        let n = points.len() as f64;
        let mut sum = [0.0f64; 3];
        for p in points {
            sum[0] += p.x;
            sum[1] += p.y;
            sum[2] += p.z;
        }
        let centroid = [sum[0] / n, sum[1] / n, sum[2] / n];
        let mut cov = [[0.0f64; 3]; 3];
        for p in points {
            let dx = p.x - centroid[0];
            let dy = p.y - centroid[1];
            let dz = p.z - centroid[2];
            cov[0][0] += dx * dx;
            cov[0][1] += dx * dy;
            cov[0][2] += dx * dz;
            cov[1][1] += dy * dy;
            cov[1][2] += dy * dz;
            cov[2][2] += dz * dz;
        }
        cov[1][0] = cov[0][1];
        cov[2][0] = cov[0][2];
        cov[2][1] = cov[1][2];
        for row in &mut cov {
            for val in row {
                *val /= n;
            }
        }
        cov
    }

    #[test]
    fn test_symmetric_eig_3x3_diagonal_matrix() {
        // Diagonal matrix: eigenvalues are the diagonal entries, sorted descending.
        let cov = [[3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]];
        let eig = symmetric_eig_3x3(&cov);
        assert!((eig[0] - 3.0).abs() < 1e-12);
        assert!((eig[1] - 2.0).abs() < 1e-12);
        assert!((eig[2] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_symmetric_eig_3x3_known_eigenvalues() {
        // The symmetric matrix
        //   [ 2 1 1 ]
        //   [ 1 2 1 ]
        //   [ 1 1 2 ]
        // has characteristic polynomial with eigenvalues 4, 1, 1
        // (1 is a double root; trace = 6, det = 4).
        let cov = [[2.0, 1.0, 1.0], [1.0, 2.0, 1.0], [1.0, 1.0, 2.0]];
        let eig = symmetric_eig_3x3(&cov);
        assert!((eig[0] - 4.0).abs() < 1e-9, "λ₁ = {}", eig[0]);
        assert!((eig[1] - 1.0).abs() < 1e-9, "λ₂ = {}", eig[1]);
        assert!((eig[2] - 1.0).abs() < 1e-9, "λ₃ = {}", eig[2]);
        // Trace and determinant invariants.
        let trace = eig[0] + eig[1] + eig[2];
        assert!((trace - 6.0).abs() < 1e-9);
        let det = eig[0] * eig[1] * eig[2];
        assert!((det - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_symmetric_eig_3x3_sorted_descending() {
        // An arbitrary symmetric matrix — eigenvalues must come out λ₁ ≥ λ₂ ≥ λ₃.
        let cov = [[4.0, 0.7, -1.3], [0.7, 2.5, 0.9], [-1.3, 0.9, 6.1]];
        let eig = symmetric_eig_3x3(&cov);
        assert!(eig[0] >= eig[1], "λ₁ {} < λ₂ {}", eig[0], eig[1]);
        assert!(eig[1] >= eig[2], "λ₂ {} < λ₃ {}", eig[1], eig[2]);
        // Trace invariant for a symmetric matrix.
        let trace = eig[0] + eig[1] + eig[2];
        assert!((trace - (4.0 + 2.5 + 6.1)).abs() < 1e-9);
    }

    #[test]
    fn test_planarity_perfectly_planar_returns_near_one() {
        // A symmetric 5×5 grid in the XY plane with zero Z extent: equal X and
        // Y variance (λ₁ ≈ λ₂) and λ₃ = 0, so P_λ = (λ₂ - λ₃) / λ₁ ≈ 1.
        let mut points = Vec::new();
        for ix in -2..=2 {
            for iy in -2..=2 {
                points.push(Point::new(ix as f64, iy as f64, 0.0));
            }
        }
        let cov = covariance_of(&points);
        let feat = dimensionality_features(&cov);
        assert!(
            feat.planarity > 0.95,
            "planarity should be near 1 for a symmetric flat grid, got {}",
            feat.planarity
        );
        // A flat patch has near-zero sphericity (λ₃ ≈ 0).
        assert!(
            feat.sphericity < 0.05,
            "sphericity should be near 0 for a flat grid, got {}",
            feat.sphericity
        );
    }

    #[test]
    fn test_planarity_spherical_cluster_returns_near_zero() {
        // Isotropic covariance: eight cube corners give equal variance on each axis.
        let points = vec![
            Point::new(-1.0, -1.0, -1.0),
            Point::new(1.0, -1.0, -1.0),
            Point::new(-1.0, 1.0, -1.0),
            Point::new(1.0, 1.0, -1.0),
            Point::new(-1.0, -1.0, 1.0),
            Point::new(1.0, -1.0, 1.0),
            Point::new(-1.0, 1.0, 1.0),
            Point::new(1.0, 1.0, 1.0),
        ];
        let cov = covariance_of(&points);
        let feat = dimensionality_features(&cov);
        assert!(
            feat.planarity < 0.05,
            "planarity should be near 0 for an isotropic cluster, got {}",
            feat.planarity
        );
    }

    #[test]
    fn test_planarity_linear_cluster_returns_low() {
        // Points along the X axis only.
        let points = vec![
            Point::new(0.0, 0.0, 0.0),
            Point::new(1.0, 0.0, 0.0),
            Point::new(2.0, 0.0, 0.0),
            Point::new(3.0, 0.0, 0.0),
            Point::new(4.0, 0.0, 0.0),
        ];
        let cov = covariance_of(&points);
        let feat = dimensionality_features(&cov);
        assert!(
            feat.planarity < 0.05,
            "planarity should be near 0 for a line, got {}",
            feat.planarity
        );
        assert!(
            feat.linearity > 0.95,
            "linearity should be near 1 for a line, got {}",
            feat.linearity
        );
    }

    #[test]
    fn test_linearity_collinear_points_returns_near_one() {
        // Collinear points along a slanted direction in 3D.
        let points = vec![
            Point::new(0.0, 0.0, 0.0),
            Point::new(1.0, 1.0, 1.0),
            Point::new(2.0, 2.0, 2.0),
            Point::new(3.0, 3.0, 3.0),
            Point::new(-1.0, -1.0, -1.0),
        ];
        let cov = covariance_of(&points);
        let feat = dimensionality_features(&cov);
        assert!(
            feat.linearity > 0.99,
            "linearity should be near 1 for collinear points, got {}",
            feat.linearity
        );
    }

    #[test]
    fn test_sphericity_isotropic_cluster_returns_near_one() {
        // Cube corners: equal variance on every axis → sphericity ≈ 1.
        let points = vec![
            Point::new(-1.0, -1.0, -1.0),
            Point::new(1.0, -1.0, -1.0),
            Point::new(-1.0, 1.0, -1.0),
            Point::new(1.0, 1.0, -1.0),
            Point::new(-1.0, -1.0, 1.0),
            Point::new(1.0, -1.0, 1.0),
            Point::new(-1.0, 1.0, 1.0),
            Point::new(1.0, 1.0, 1.0),
        ];
        let cov = covariance_of(&points);
        let feat = dimensionality_features(&cov);
        assert!(
            feat.sphericity > 0.95,
            "sphericity should be near 1 for an isotropic cluster, got {}",
            feat.sphericity
        );
    }

    #[test]
    fn test_dimensionality_features_degenerate_cluster_all_zero() {
        // Zero covariance (all points coincident) → λ₁ ≈ 0 → all features 0.
        let cov = [[0.0; 3]; 3];
        let feat = dimensionality_features(&cov);
        assert_eq!(feat.linearity, 0.0);
        assert_eq!(feat.planarity, 0.0);
        assert_eq!(feat.sphericity, 0.0);
    }
}
