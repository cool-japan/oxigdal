//! Automatic fixes for quality control issues.
//!
//! This module provides functionality to automatically fix common
//! quality control issues where safe to do so.

use crate::error::{QcError, QcResult};
use oxigdal_core::vector::{
    Coordinate, Feature, FeatureCollection, FeatureId, Geometry, LineString, Polygon,
};

/// Helper function to convert FeatureId to String
fn feature_id_to_string(id: &FeatureId) -> String {
    match id {
        FeatureId::Integer(i) => i.to_string(),
        FeatureId::String(s) => s.clone(),
    }
}

/// Strategy for automatic fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixStrategy {
    /// Conservative fixes only (no data loss).
    Conservative,

    /// Moderate fixes (minimal data modification).
    Moderate,

    /// Aggressive fixes (may modify data significantly).
    Aggressive,
}

/// Result of fix operation.
#[derive(Debug, Clone)]
pub struct FixResult {
    /// Number of features processed.
    pub features_processed: usize,

    /// Number of features fixed.
    pub features_fixed: usize,

    /// Number of features unchanged.
    pub features_unchanged: usize,

    /// Number of features removed.
    pub features_removed: usize,

    /// Detailed fix operations.
    pub operations: Vec<FixOperation>,
}

/// A fix operation that was performed.
#[derive(Debug, Clone)]
pub struct FixOperation {
    /// Feature ID (if available).
    pub feature_id: Option<String>,

    /// Type of fix applied.
    pub fix_type: FixType,

    /// Description of the fix.
    pub description: String,
}

/// Types of fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixType {
    /// Removed duplicate vertex.
    RemoveDuplicateVertex,

    /// Closed open ring.
    CloseRing,

    /// Simplified geometry.
    SimplifyGeometry,

    /// Snapped to grid.
    SnapToGrid,

    /// Removed sliver polygon.
    RemoveSliver,

    /// Fixed invalid geometry.
    FixInvalidGeometry,

    /// Removed feature.
    RemoveFeature,
}

/// Topology fixer.
pub struct TopologyFixer {
    #[allow(dead_code)]
    strategy: FixStrategy,
    tolerance: f64,
}

impl TopologyFixer {
    /// Creates a new topology fixer with the given strategy.
    #[must_use]
    pub fn new(strategy: FixStrategy) -> Self {
        Self {
            strategy,
            tolerance: 1e-9,
        }
    }

    /// Sets the coordinate tolerance.
    #[must_use]
    pub const fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Fixes topology issues in a feature collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fixing fails.
    pub fn fix_topology(
        &self,
        features: &FeatureCollection,
    ) -> QcResult<(FeatureCollection, FixResult)> {
        let mut fixed_features = Vec::new();
        let mut operations = Vec::new();
        let mut features_fixed = 0;
        let mut features_unchanged = 0;
        let mut features_removed = 0;

        for feature in &features.features {
            match self.fix_feature(feature) {
                Ok(Some((fixed_feature, ops))) => {
                    if !ops.is_empty() {
                        features_fixed += 1;
                        operations.extend(ops);
                    } else {
                        features_unchanged += 1;
                    }
                    fixed_features.push(fixed_feature);
                }
                Ok(None) => {
                    features_removed += 1;
                    operations.push(FixOperation {
                        feature_id: feature.id.as_ref().map(feature_id_to_string),
                        fix_type: FixType::RemoveFeature,
                        description: "Feature removed due to unfixable issues".to_string(),
                    });
                }
                Err(_) => {
                    // Keep original feature if fix fails
                    features_unchanged += 1;
                    fixed_features.push(feature.clone());
                }
            }
        }

        let result = FixResult {
            features_processed: features.features.len(),
            features_fixed,
            features_unchanged,
            features_removed,
            operations,
        };

        Ok((
            FeatureCollection {
                features: fixed_features,
                metadata: features.metadata.clone(),
            },
            result,
        ))
    }

    fn fix_feature(&self, feature: &Feature) -> QcResult<Option<(Feature, Vec<FixOperation>)>> {
        let mut fixed_feature = feature.clone();
        let mut operations = Vec::new();

        if let Some(geometry) = &feature.geometry {
            match self.fix_geometry(geometry) {
                Ok(Some((fixed_geom, ops))) => {
                    fixed_feature.geometry = Some(fixed_geom);
                    operations.extend(ops);
                }
                Ok(None) => {
                    // Geometry unfixable, remove feature
                    return Ok(None);
                }
                Err(_) => {
                    // Keep original geometry
                }
            }
        }

        Ok(Some((fixed_feature, operations)))
    }

    fn fix_geometry(&self, geometry: &Geometry) -> QcResult<Option<(Geometry, Vec<FixOperation>)>> {
        let mut operations = Vec::new();

        let fixed = match geometry {
            Geometry::LineString(linestring) => {
                let (fixed_ls, ops) = self.fix_linestring(linestring)?;
                operations.extend(ops);
                Some(Geometry::LineString(fixed_ls))
            }
            Geometry::Polygon(polygon) => match self.fix_polygon(polygon)? {
                Some((fixed_poly, ops)) => {
                    operations.extend(ops);
                    Some(Geometry::Polygon(fixed_poly))
                }
                None => None,
            },
            _ => Some(geometry.clone()),
        };

        Ok(fixed.map(|g| (g, operations)))
    }

    fn fix_linestring(&self, linestring: &LineString) -> QcResult<(LineString, Vec<FixOperation>)> {
        let mut operations = Vec::new();
        let mut coords = linestring.coords.clone();

        // Remove duplicate consecutive vertices
        let original_len = coords.len();
        coords.dedup_by(|a, b| self.coords_equal(a, b));

        if coords.len() < original_len {
            operations.push(FixOperation {
                feature_id: None,
                fix_type: FixType::RemoveDuplicateVertex,
                description: format!("Removed {} duplicate vertices", original_len - coords.len()),
            });
        }

        Ok((LineString { coords }, operations))
    }

    fn fix_polygon(&self, polygon: &Polygon) -> QcResult<Option<(Polygon, Vec<FixOperation>)>> {
        let mut operations = Vec::new();
        let (mut exterior, ext_ops) = self.fix_linestring(&polygon.exterior)?;
        operations.extend(ext_ops);

        // Ensure ring is closed
        if !exterior.coords.is_empty() {
            let first = exterior.coords[0];
            let last = *exterior
                .coords
                .last()
                .ok_or_else(|| QcError::FixError("Cannot get last coordinate".to_string()))?;

            if !self.coords_equal(&first, &last) {
                exterior.coords.push(first);
                operations.push(FixOperation {
                    feature_id: None,
                    fix_type: FixType::CloseRing,
                    description: "Closed open polygon ring".to_string(),
                });
            }
        }

        // Check if polygon is valid after fixes
        if exterior.coords.len() < 4 {
            return Ok(None); // Polygon too small
        }

        // Fix interior rings (holes)
        let mut fixed_interiors = Vec::new();
        for interior in &polygon.interiors {
            if let Ok((fixed_interior, interior_ops)) = self.fix_linestring(interior) {
                operations.extend(interior_ops);
                if fixed_interior.coords.len() >= 4 {
                    fixed_interiors.push(fixed_interior);
                }
            }
        }

        Ok(Some((
            Polygon {
                exterior,
                interiors: fixed_interiors,
            },
            operations,
        )))
    }

    fn coords_equal(&self, a: &Coordinate, b: &Coordinate) -> bool {
        (a.x - b.x).abs() < self.tolerance && (a.y - b.y).abs() < self.tolerance
    }

    /// Removes sliver polygons from a feature collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn remove_slivers(
        &self,
        features: &FeatureCollection,
        area_threshold: f64,
    ) -> QcResult<(FeatureCollection, FixResult)> {
        let mut fixed_features = Vec::new();
        let mut operations = Vec::new();
        let mut features_removed = 0;

        for feature in &features.features {
            let mut remove = false;

            if let Some(Geometry::Polygon(polygon)) = &feature.geometry {
                let area = self.calculate_area(polygon);
                if area < area_threshold {
                    remove = true;
                    features_removed += 1;
                    operations.push(FixOperation {
                        feature_id: feature.id.as_ref().map(feature_id_to_string),
                        fix_type: FixType::RemoveSliver,
                        description: format!("Removed sliver polygon with area {:.6}", area),
                    });
                }
            }

            if !remove {
                fixed_features.push(feature.clone());
            }
        }

        let result = FixResult {
            features_processed: features.features.len(),
            features_fixed: 0,
            features_unchanged: fixed_features.len(),
            features_removed,
            operations,
        };

        Ok((
            FeatureCollection {
                features: fixed_features,
                metadata: features.metadata.clone(),
            },
            result,
        ))
    }

    /// Snaps coordinates to grid.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn snap_to_grid(
        &self,
        features: &FeatureCollection,
        grid_size: f64,
    ) -> QcResult<(FeatureCollection, FixResult)> {
        let mut fixed_features = Vec::new();
        let mut operations = Vec::new();
        let mut features_fixed = 0;

        for feature in &features.features {
            let mut fixed_feature = feature.clone();
            let mut snapped = false;

            if let Some(geometry) = &feature.geometry {
                if let Some(snapped_geom) = self.snap_geometry_to_grid(geometry, grid_size)? {
                    fixed_feature.geometry = Some(snapped_geom);
                    snapped = true;
                    features_fixed += 1;
                }
            }

            if snapped {
                operations.push(FixOperation {
                    feature_id: feature.id.as_ref().map(feature_id_to_string),
                    fix_type: FixType::SnapToGrid,
                    description: format!("Snapped to grid size {:.6}", grid_size),
                });
            }

            fixed_features.push(fixed_feature);
        }

        let result = FixResult {
            features_processed: features.features.len(),
            features_fixed,
            features_unchanged: features.features.len() - features_fixed,
            features_removed: 0,
            operations,
        };

        Ok((
            FeatureCollection {
                features: fixed_features,
                metadata: features.metadata.clone(),
            },
            result,
        ))
    }

    fn snap_geometry_to_grid(
        &self,
        geometry: &Geometry,
        grid_size: f64,
    ) -> QcResult<Option<Geometry>> {
        match geometry {
            Geometry::Point(point) => {
                let snapped_coord = self.snap_coordinate(&point.coord, grid_size);
                Ok(Some(Geometry::Point(
                    oxigdal_core::vector::Point::from_coord(snapped_coord),
                )))
            }
            Geometry::LineString(linestring) => {
                let snapped_coords: Vec<Coordinate> = linestring
                    .coords
                    .iter()
                    .map(|c| self.snap_coordinate(c, grid_size))
                    .collect();
                Ok(Some(Geometry::LineString(LineString {
                    coords: snapped_coords,
                })))
            }
            Geometry::Polygon(polygon) => {
                let snapped_exterior: Vec<Coordinate> = polygon
                    .exterior
                    .coords
                    .iter()
                    .map(|c| self.snap_coordinate(c, grid_size))
                    .collect();

                let snapped_interiors: Vec<LineString> = polygon
                    .interiors
                    .iter()
                    .map(|interior| LineString {
                        coords: interior
                            .coords
                            .iter()
                            .map(|c| self.snap_coordinate(c, grid_size))
                            .collect(),
                    })
                    .collect();

                Ok(Some(Geometry::Polygon(Polygon {
                    exterior: LineString {
                        coords: snapped_exterior,
                    },
                    interiors: snapped_interiors,
                })))
            }
            _ => Ok(Some(geometry.clone())),
        }
    }

    fn snap_coordinate(&self, coord: &Coordinate, grid_size: f64) -> Coordinate {
        Coordinate {
            x: (coord.x / grid_size).round() * grid_size,
            y: (coord.y / grid_size).round() * grid_size,
            z: coord.z,
            m: coord.m,
        }
    }

    fn calculate_area(&self, polygon: &Polygon) -> f64 {
        let coords = &polygon.exterior.coords;
        if coords.len() < 3 {
            return 0.0;
        }

        let mut area = 0.0;
        for i in 0..coords.len() - 1 {
            area += coords[i].x * coords[i + 1].y;
            area -= coords[i + 1].x * coords[i].y;
        }

        (area / 2.0).abs()
    }
}

impl Default for TopologyFixer {
    fn default() -> Self {
        Self::new(FixStrategy::Conservative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_fixer_creation() {
        let fixer = TopologyFixer::new(FixStrategy::Conservative);
        assert_eq!(fixer.strategy, FixStrategy::Conservative);
    }

    #[test]
    fn test_fix_linestring() {
        let fixer = TopologyFixer::new(FixStrategy::Conservative);

        let linestring = LineString {
            coords: vec![
                Coordinate::new_2d(0.0, 0.0),
                Coordinate::new_2d(0.0, 0.0), // Duplicate
                Coordinate::new_2d(1.0, 1.0),
            ],
        };

        let result = fixer.fix_linestring(&linestring);
        assert!(result.is_ok());

        #[allow(clippy::unwrap_used)]
        let (fixed, ops) =
            result.expect("linestring fix should succeed for duplicate vertex removal");
        assert_eq!(fixed.coords.len(), 2);
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_snap_coordinate() {
        let fixer = TopologyFixer::new(FixStrategy::Conservative);
        let coord = Coordinate::new_2d(1.234, 5.678);
        let snapped = fixer.snap_coordinate(&coord, 0.1);

        assert!((snapped.x - 1.2).abs() < 1e-10);
        assert!((snapped.y - 5.7).abs() < 1e-10);
    }

    #[test]
    fn test_coords_equal() {
        let fixer = TopologyFixer::new(FixStrategy::Conservative);
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(0.0, 0.0);
        let c3 = Coordinate::new_2d(1.0, 1.0);

        assert!(fixer.coords_equal(&c1, &c2));
        assert!(!fixer.coords_equal(&c1, &c3));
    }
}
