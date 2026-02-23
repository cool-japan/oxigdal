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

    // Simplified planarity: ratio of smallest to largest eigenvalue
    // For perfect planarity, one eigenvalue should be near zero
    // This is a simplified approximation
    let trace = cov[0][0] + cov[1][1] + cov[2][2];
    let min_variance = cov[0][0].min(cov[1][1]).min(cov[2][2]);

    if trace > 0.0 {
        1.0 - (min_variance / trace)
    } else {
        0.0
    }
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
        system_identifier: "OxiGDAL".to_string(),
        generating_software: "oxigdal-3d classification".to_string(),
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
}
