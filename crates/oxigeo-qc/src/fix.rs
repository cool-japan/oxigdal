//! Automatic fixes for quality control issues.
//!
//! This module provides functionality to automatically fix common
//! quality control issues where safe to do so.

use crate::error::{QcError, QcResult};
use oxigeo_core::vector::{
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

    /// Whether the configured strategy permits dropping an entire feature
    /// (or its polygon) when it cannot be repaired into a valid geometry
    /// (e.g. an exterior ring with fewer than 4 points after fixing).
    ///
    /// Only [`FixStrategy::Aggressive`] allows this: [`FixStrategy::Conservative`]
    /// promises "no data loss" and [`FixStrategy::Moderate`] only permits
    /// pruning interior rings/points (see [`Self::allows_interior_ring_pruning`]),
    /// not removing whole features.
    const fn allows_feature_removal(&self) -> bool {
        matches!(self.strategy, FixStrategy::Aggressive)
    }

    /// Whether the configured strategy permits dropping a degenerate
    /// interior ring (hole) that has fewer than 4 points after fixing.
    ///
    /// [`FixStrategy::Moderate`] and [`FixStrategy::Aggressive`] both allow
    /// this ("minimal data modification" / "may modify data significantly");
    /// [`FixStrategy::Conservative`] retains the ring unmodified instead.
    const fn allows_interior_ring_pruning(&self) -> bool {
        matches!(
            self.strategy,
            FixStrategy::Moderate | FixStrategy::Aggressive
        )
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
            if self.allows_feature_removal() {
                return Ok(None); // Polygon too small; Aggressive may drop it
            }

            // Conservative/Moderate must not silently lose the whole
            // feature: retain the (still under-sized) exterior unmodified
            // and report the problem instead.
            operations.push(FixOperation {
                feature_id: None,
                fix_type: FixType::FixInvalidGeometry,
                description: format!(
                    "Exterior ring has only {} point(s) after fixing (a valid ring needs >= 4); \
                     FixStrategy::{:?} retains it rather than removing the feature",
                    exterior.coords.len(),
                    self.strategy
                ),
            });

            return Ok(Some((
                Polygon {
                    exterior,
                    interiors: polygon.interiors.clone(),
                },
                operations,
            )));
        }

        // Fix interior rings (holes)
        let mut fixed_interiors = Vec::new();
        for interior in &polygon.interiors {
            if let Ok((fixed_interior, interior_ops)) = self.fix_linestring(interior) {
                operations.extend(interior_ops);
                if fixed_interior.coords.len() >= 4 {
                    fixed_interiors.push(fixed_interior);
                } else if self.allows_interior_ring_pruning() {
                    operations.push(FixOperation {
                        feature_id: None,
                        fix_type: FixType::RemoveSliver,
                        description: format!(
                            "Removed interior ring with only {} point(s) after fixing (needs >= 4)",
                            fixed_interior.coords.len()
                        ),
                    });
                } else {
                    // Conservative: keep the degenerate ring rather than
                    // losing data.
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

            if let Some(geometry) = &feature.geometry
                && let Some(snapped_geom) = self.snap_geometry_to_grid(geometry, grid_size)?
            {
                fixed_feature.geometry = Some(snapped_geom);
                snapped = true;
                features_fixed += 1;
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
                    oxigeo_core::vector::Point::from_coord(snapped_coord),
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

        #[allow(clippy::unwrap_used, clippy::expect_used)]
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

    /// A degenerate polygon whose exterior collapses to fewer than 4 points
    /// after duplicate-vertex removal (all points are the same location).
    fn degenerate_polygon() -> Polygon {
        Polygon {
            exterior: LineString {
                coords: vec![
                    Coordinate::new_2d(0.0, 0.0),
                    Coordinate::new_2d(0.0, 0.0),
                    Coordinate::new_2d(0.0, 0.0),
                ],
            },
            interiors: vec![],
        }
    }

    #[test]
    fn test_conservative_strategy_never_removes_degenerate_polygon() {
        let fixer = TopologyFixer::new(FixStrategy::Conservative);
        let polygon = degenerate_polygon();

        let result = fixer
            .fix_polygon(&polygon)
            .expect("fix_polygon should not error");
        assert!(
            result.is_some(),
            "FixStrategy::Conservative must never drop a feature/polygon (no data loss), \
             even when it cannot be repaired into a valid ring"
        );
        let (fixed, ops) = result.expect("checked is_some above");
        // The under-sized exterior is retained unmodified, not silently
        // padded/discarded.
        assert!(fixed.exterior.coords.len() < 4);
        assert!(
            ops.iter()
                .any(|op| op.fix_type == FixType::FixInvalidGeometry),
            "the unfixable condition must still be reported"
        );
    }

    #[test]
    fn test_moderate_strategy_also_never_removes_degenerate_polygon() {
        let fixer = TopologyFixer::new(FixStrategy::Moderate);
        let polygon = degenerate_polygon();

        let result = fixer
            .fix_polygon(&polygon)
            .expect("fix_polygon should not error");
        assert!(
            result.is_some(),
            "FixStrategy::Moderate only allows interior-ring/point pruning, not feature removal"
        );
    }

    #[test]
    fn test_aggressive_strategy_removes_degenerate_polygon() {
        let fixer = TopologyFixer::new(FixStrategy::Aggressive);
        let polygon = degenerate_polygon();

        let result = fixer
            .fix_polygon(&polygon)
            .expect("fix_polygon should not error");
        assert!(
            result.is_none(),
            "FixStrategy::Aggressive is documented to allow removing unfixable features"
        );
    }

    /// A polygon with a valid exterior but a degenerate interior ring (hole)
    /// that collapses to fewer than 4 points after fixing.
    fn polygon_with_degenerate_hole() -> Polygon {
        Polygon {
            exterior: LineString {
                coords: vec![
                    Coordinate::new_2d(0.0, 0.0),
                    Coordinate::new_2d(10.0, 0.0),
                    Coordinate::new_2d(10.0, 10.0),
                    Coordinate::new_2d(0.0, 10.0),
                    Coordinate::new_2d(0.0, 0.0),
                ],
            },
            interiors: vec![LineString {
                coords: vec![
                    Coordinate::new_2d(5.0, 5.0),
                    Coordinate::new_2d(5.0, 5.0),
                    Coordinate::new_2d(5.0, 5.0),
                ],
            }],
        }
    }

    #[test]
    fn test_conservative_strategy_keeps_degenerate_interior_ring() {
        let fixer = TopologyFixer::new(FixStrategy::Conservative);
        let polygon = polygon_with_degenerate_hole();

        let (fixed, _ops) = fixer
            .fix_polygon(&polygon)
            .expect("fix_polygon should not error")
            .expect("valid exterior should not cause feature removal");

        assert_eq!(
            fixed.interiors.len(),
            1,
            "Conservative must retain the degenerate interior ring instead of pruning it"
        );
    }

    #[test]
    fn test_moderate_strategy_prunes_degenerate_interior_ring() {
        let fixer = TopologyFixer::new(FixStrategy::Moderate);
        let polygon = polygon_with_degenerate_hole();

        let (fixed, ops) = fixer
            .fix_polygon(&polygon)
            .expect("fix_polygon should not error")
            .expect("valid exterior should not cause feature removal");

        assert_eq!(
            fixed.interiors.len(),
            0,
            "Moderate is documented to allow interior-ring pruning"
        );
        assert!(ops.iter().any(|op| op.fix_type == FixType::RemoveSliver));
    }
}
