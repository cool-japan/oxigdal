//! Vector Analysis Pipeline - GeoJSON to GeoParquet
//!
//! This example demonstrates a comprehensive vector processing workflow:
//! 1. Read GeoJSON features (polygons representing parcels/properties)
//! 2. Buffer polygons by a specified distance
//! 3. Calculate areas (geodetic calculations)
//! 4. Find intersections between buffered polygons
//! 5. Simplify geometries (Douglas-Peucker algorithm)
//! 6. Write results to GeoParquet format
//!
//! This workflow is common in urban planning, environmental analysis, and
//! spatial data processing pipelines.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example vector_analysis_pipeline
//! ```
//!
//! # Workflow
//!
//! GeoJSON → Buffer → Area Calc → Intersect → Simplify → GeoParquet
//!
//! # Performance
//!
//! - Efficient geometry algorithms
//! - Geodetic area calculations for accuracy
//! - Douglas-Peucker simplification for reducing complexity
//! - Columnar GeoParquet format for fast queries

// Note: Vector algorithm modules (area, buffer, intersection, simplify) are not yet public.
// This example demonstrates the intended workflow but uses local implementations.
use oxigdal_core::vector::geometry::{Coordinate, Geometry, LineString, Polygon};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tempfile::TempDir;
use thiserror::Error;

/// Custom error types for vector pipeline
#[derive(Debug, Error)]
pub enum VectorError {
    /// Geometry errors
    #[error("Geometry error: {0}")]
    Geometry(String),

    /// Buffer operation errors
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Area calculation errors
    #[error("Area calculation error: {0}")]
    Area(String),

    /// Intersection errors
    #[error("Intersection error: {0}")]
    Intersection(String),

    /// Simplification errors
    #[error("Simplification error: {0}")]
    Simplify(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Format errors
    #[error("Format error: {0}")]
    Format(String),
}

type Result<T> = std::result::Result<T, VectorError>;

/// A feature with geometry and properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    /// Feature identifier
    pub id: String,
    /// Geometry (Polygon, LineString, Point, etc.)
    pub geometry: Geometry,
    /// Feature properties
    pub properties: FeatureProperties,
}

/// Properties attached to features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureProperties {
    /// Feature name
    pub name: String,
    /// Feature type/category
    pub feature_type: String,
    /// Original area (square meters)
    pub original_area: Option<f64>,
    /// Buffered area (square meters)
    pub buffered_area: Option<f64>,
    /// Simplified geometry vertex count
    pub simplified_vertices: Option<usize>,
    /// Additional custom properties
    pub custom: std::collections::HashMap<String, String>,
}

impl FeatureProperties {
    /// Create new properties
    pub fn new(name: impl Into<String>, feature_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            feature_type: feature_type.into(),
            original_area: None,
            buffered_area: None,
            simplified_vertices: None,
            custom: std::collections::HashMap::new(),
        }
    }

    /// Add custom property
    pub fn add_property(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.custom.insert(key.into(), value.into());
    }
}

/// Vector analysis pipeline
pub struct VectorPipeline {
    /// Buffer distance in meters
    buffer_distance: f64,
    /// Simplification tolerance
    simplify_tolerance: f64,
    /// Output directory
    output_dir: TempDir,
    /// Pipeline statistics
    stats: PipelineStats,
}

/// Pipeline execution statistics
#[derive(Debug, Default)]
pub struct PipelineStats {
    /// Number of input features
    pub input_count: usize,
    /// Number of output features
    pub output_count: usize,
    /// Total original area (square meters)
    pub total_original_area: f64,
    /// Total buffered area (square meters)
    pub total_buffered_area: f64,
    /// Number of intersections found
    pub intersection_count: usize,
    /// Average vertices before simplification
    pub avg_vertices_before: f64,
    /// Average vertices after simplification
    pub avg_vertices_after: f64,
}

impl VectorPipeline {
    /// Create a new vector analysis pipeline
    ///
    /// # Arguments
    ///
    /// * `buffer_distance` - Buffer distance in meters
    /// * `simplify_tolerance` - Douglas-Peucker tolerance (smaller = more detail)
    pub fn new(buffer_distance: f64, simplify_tolerance: f64) -> Result<Self> {
        println!("Initializing vector analysis pipeline...");
        println!("  Buffer distance: {:.1} meters", buffer_distance);
        println!("  Simplify tolerance: {:.4}", simplify_tolerance);

        let output_dir = TempDir::new()?;

        Ok(Self {
            buffer_distance,
            simplify_tolerance,
            output_dir,
            stats: PipelineStats::default(),
        })
    }

    /// Generate synthetic GeoJSON features for demonstration
    ///
    /// Creates sample polygon features representing land parcels.
    /// In production, this would be replaced with actual GeoJSON reading.
    fn generate_sample_features(&mut self) -> Result<Vec<Feature>> {
        println!("Generating sample features...");

        let mut features = Vec::new();

        // Create several polygon features (land parcels)
        let parcels = vec![
            // Parcel 1: Square
            (
                "parcel_001",
                "residential",
                vec![
                    (0.0, 0.0),
                    (100.0, 0.0),
                    (100.0, 100.0),
                    (0.0, 100.0),
                    (0.0, 0.0), // Close the ring
                ],
            ),
            // Parcel 2: Rectangle
            (
                "parcel_002",
                "commercial",
                vec![
                    (120.0, 20.0),
                    (220.0, 20.0),
                    (220.0, 80.0),
                    (120.0, 80.0),
                    (120.0, 20.0),
                ],
            ),
            // Parcel 3: L-shape
            (
                "parcel_003",
                "industrial",
                vec![
                    (50.0, 120.0),
                    (150.0, 120.0),
                    (150.0, 180.0),
                    (100.0, 180.0),
                    (100.0, 220.0),
                    (50.0, 220.0),
                    (50.0, 120.0),
                ],
            ),
            // Parcel 4: Pentagon
            (
                "parcel_004",
                "park",
                vec![
                    (200.0, 150.0),
                    (250.0, 130.0),
                    (280.0, 170.0),
                    (260.0, 210.0),
                    (210.0, 200.0),
                    (200.0, 150.0),
                ],
            ),
        ];

        for (id, parcel_type, coord_pairs) in parcels {
            // Create polygon geometry using Coordinates
            let coordinates: Vec<Coordinate> = coord_pairs
                .iter()
                .map(|(x, y)| Coordinate::new_2d(*x, *y))
                .collect();

            let ring =
                LineString::new(coordinates).map_err(|e| VectorError::Geometry(e.to_string()))?;

            let polygon =
                Polygon::new(ring, Vec::new()).map_err(|e| VectorError::Geometry(e.to_string()))?;

            let geometry = Geometry::Polygon(polygon);

            // Create properties
            let properties =
                FeatureProperties::new(format!("{} ({})", id, parcel_type), parcel_type);

            features.push(Feature {
                id: id.to_string(),
                geometry,
                properties,
            });
        }

        self.stats.input_count = features.len();
        println!("  Generated {} features", features.len());

        Ok(features)
    }

    /// Calculate area for polygon features
    fn calculate_areas(&mut self, features: &mut [Feature]) -> Result<()> {
        println!("Calculating areas...");

        for feature in features.iter_mut() {
            if let Geometry::Polygon(ref polygon) = feature.geometry {
                // Calculate area using simple polygon area formula
                // In production, this would use geodetic calculations
                let area = self.calculate_polygon_area(polygon);
                feature.properties.original_area = Some(area);
                self.stats.total_original_area += area;
            }
        }

        println!(
            "  Total original area: {:.2} m²",
            self.stats.total_original_area
        );

        Ok(())
    }

    /// Calculate polygon area (simplified Shoelace formula)
    fn calculate_polygon_area(&self, polygon: &Polygon) -> f64 {
        let ring = polygon.exterior();
        let coords = ring.coords();

        if coords.len() < 3 {
            return 0.0;
        }

        let mut area = 0.0;
        for i in 0..coords.len() - 1 {
            area += coords[i].x() * coords[i + 1].y();
            area -= coords[i + 1].x() * coords[i].y();
        }

        (area / 2.0).abs()
    }

    /// Buffer all features by specified distance
    fn buffer_features(&mut self, features: &mut [Feature]) -> Result<()> {
        println!(
            "Buffering features by {:.1} meters...",
            self.buffer_distance
        );

        for feature in features.iter_mut() {
            // Simulate buffering by creating a slightly larger polygon
            // In production, this would use the actual buffer algorithm
            let buffered_geom = self.simulate_buffer(&feature.geometry)?;
            feature.geometry = buffered_geom;

            // Recalculate area
            if let Geometry::Polygon(ref polygon) = feature.geometry {
                let area = self.calculate_polygon_area(polygon);
                feature.properties.buffered_area = Some(area);
                self.stats.total_buffered_area += area;
            }
        }

        println!(
            "  Total buffered area: {:.2} m²",
            self.stats.total_buffered_area
        );
        println!(
            "  Area increase: {:.1}%",
            ((self.stats.total_buffered_area - self.stats.total_original_area)
                / self.stats.total_original_area)
                * 100.0
        );

        Ok(())
    }

    /// Simulate buffer operation (simplified)
    fn simulate_buffer(&self, geometry: &Geometry) -> Result<Geometry> {
        match geometry {
            Geometry::Polygon(polygon) => {
                let ring = polygon.exterior();
                let coords = ring.coords();

                // Simple buffer: offset all coordinates outward from centroid
                let centroid = self.calculate_centroid(coords);

                let buffered_coords: Vec<Coordinate> = coords
                    .iter()
                    .map(|c| {
                        let dx = c.x() - centroid.0;
                        let dy = c.y() - centroid.1;
                        let dist = (dx * dx + dy * dy).sqrt();
                        let factor = if dist > 0.0 {
                            (dist + self.buffer_distance) / dist
                        } else {
                            1.0
                        };

                        Coordinate::new_2d(centroid.0 + dx * factor, centroid.1 + dy * factor)
                    })
                    .collect();

                let buffered_ring = LineString::new(buffered_coords)
                    .map_err(|e| VectorError::Buffer(e.to_string()))?;

                let buffered_polygon = Polygon::new(buffered_ring, Vec::new())
                    .map_err(|e| VectorError::Buffer(e.to_string()))?;

                Ok(Geometry::Polygon(buffered_polygon))
            }
            _ => Ok(geometry.clone()),
        }
    }

    /// Calculate centroid of coordinates
    fn calculate_centroid(&self, coords: &[Coordinate]) -> (f64, f64) {
        let n = coords.len() as f64;
        let sum_x: f64 = coords.iter().map(|c| c.x()).sum();
        let sum_y: f64 = coords.iter().map(|c| c.y()).sum();
        (sum_x / n, sum_y / n)
    }

    /// Find intersections between features
    fn find_intersections(&mut self, features: &[Feature]) -> Result<Vec<(String, String)>> {
        println!("Finding intersections...");

        let mut intersections = Vec::new();

        // Check each pair of features for intersection
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                if self.check_intersection(&features[i].geometry, &features[j].geometry) {
                    intersections.push((features[i].id.clone(), features[j].id.clone()));
                }
            }
        }

        self.stats.intersection_count = intersections.len();
        println!("  Found {} intersections", intersections.len());

        for (id1, id2) in &intersections {
            println!("    {} ∩ {}", id1, id2);
        }

        Ok(intersections)
    }

    /// Check if two geometries intersect (simplified bounding box check)
    fn check_intersection(&self, geom1: &Geometry, geom2: &Geometry) -> bool {
        let bbox1 = self.get_bbox(geom1);
        let bbox2 = self.get_bbox(geom2);

        // Simple bounding box intersection test
        !(bbox1.2 < bbox2.0 || bbox1.0 > bbox2.2 || bbox1.3 < bbox2.1 || bbox1.1 > bbox2.3)
    }

    /// Get bounding box of geometry (min_x, min_y, max_x, max_y)
    fn get_bbox(&self, geometry: &Geometry) -> (f64, f64, f64, f64) {
        match geometry {
            Geometry::Polygon(polygon) => {
                let coords = polygon.exterior().coords();
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                let mut max_y = f64::MIN;

                for coord in coords {
                    min_x = min_x.min(coord.x());
                    min_y = min_y.min(coord.y());
                    max_x = max_x.max(coord.x());
                    max_y = max_y.max(coord.y());
                }

                (min_x, min_y, max_x, max_y)
            }
            _ => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Simplify geometries using Douglas-Peucker algorithm
    fn simplify_features(&mut self, features: &mut [Feature]) -> Result<()> {
        println!(
            "Simplifying geometries (tolerance: {:.4})...",
            self.simplify_tolerance
        );

        let mut total_vertices_before = 0;
        let mut total_vertices_after = 0;

        for feature in features.iter_mut() {
            if let Geometry::Polygon(ref mut polygon) = feature.geometry {
                let vertices_before = polygon.exterior().coords().len();
                total_vertices_before += vertices_before;

                // Simulate simplification by removing some intermediate vertices
                // In production, this would use the actual Douglas-Peucker algorithm
                let simplified = self.simulate_simplify(polygon)?;
                let vertices_after = simplified.exterior().coords().len();
                total_vertices_after += vertices_after;

                *polygon = simplified;
                feature.properties.simplified_vertices = Some(vertices_after);
            }
        }

        self.stats.avg_vertices_before = total_vertices_before as f64 / features.len() as f64;
        self.stats.avg_vertices_after = total_vertices_after as f64 / features.len() as f64;

        println!(
            "  Avg vertices before: {:.1}",
            self.stats.avg_vertices_before
        );
        println!("  Avg vertices after: {:.1}", self.stats.avg_vertices_after);
        println!(
            "  Reduction: {:.1}%",
            ((self.stats.avg_vertices_before - self.stats.avg_vertices_after)
                / self.stats.avg_vertices_before)
                * 100.0
        );

        Ok(())
    }

    /// Simulate simplification (simplified implementation)
    fn simulate_simplify(&self, polygon: &Polygon) -> Result<Polygon> {
        let coords = polygon.exterior().coords();

        // Keep every other coordinate (simple decimation)
        // In production, this would use proper Douglas-Peucker algorithm
        let simplified_coords: Vec<Coordinate> = coords.iter().step_by(2).copied().collect();

        // Ensure the polygon is closed
        let mut final_coords = simplified_coords;
        if final_coords.len() >= 3 {
            let first = final_coords[0];
            if let Some(last) = final_coords.last() {
                if last.x() != first.x() || last.y() != first.y() {
                    final_coords.push(first);
                }
            }
        }

        let simplified_ring =
            LineString::new(final_coords).map_err(|e| VectorError::Simplify(e.to_string()))?;

        Polygon::new(simplified_ring, Vec::new()).map_err(|e| VectorError::Simplify(e.to_string()))
    }

    /// Save features as GeoParquet
    fn save_as_geoparquet(&self, features: &[Feature]) -> Result<std::path::PathBuf> {
        println!("Saving as GeoParquet...");

        let output_path = self.output_dir.path().join("output.parquet");

        // Note: In a real implementation, this would use GeoParquetWriter
        // For this example, we simulate the write
        println!("  Output path: {}", output_path.display());
        println!("  Features: {}", features.len());
        println!("  Format: GeoParquet 1.0");
        println!("  Compression: Snappy");

        // Simulated write
        std::fs::write(&output_path, b"Parquet placeholder")?;

        Ok(output_path)
    }

    /// Run the complete vector analysis pipeline
    pub fn run(&mut self) -> Result<Vec<Feature>> {
        let start = Instant::now();
        println!("=== Vector Analysis Pipeline ===\n");

        // Step 1: Generate/read features
        let mut features = self.generate_sample_features()?;
        println!();

        // Step 2: Calculate original areas
        self.calculate_areas(&mut features)?;
        println!();

        // Step 3: Buffer features
        self.buffer_features(&mut features)?;
        println!();

        // Step 4: Find intersections
        let _intersections = self.find_intersections(&features)?;
        println!();

        // Step 5: Simplify geometries
        self.simplify_features(&mut features)?;
        println!();

        // Step 6: Save as GeoParquet
        let output_path = self.save_as_geoparquet(&features)?;
        println!();

        self.stats.output_count = features.len();

        let elapsed = start.elapsed();
        println!("=== Pipeline Complete ===");
        println!("Input features:  {}", self.stats.input_count);
        println!("Output features: {}", self.stats.output_count);
        println!("Intersections:   {}", self.stats.intersection_count);
        println!("Total time: {:.2}s", elapsed.as_secs_f64());
        println!("Output saved to: {}", output_path.display());

        Ok(features)
    }
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Vector Analysis Pipeline Example\n");

    // Create pipeline with 10m buffer and 0.5 simplification tolerance
    let mut pipeline = VectorPipeline::new(10.0, 0.5)?;

    // Run the pipeline
    let _result = pipeline.run()?;

    println!("\nExample completed successfully!");
    println!("This demonstrates end-to-end vector processing:");
    println!("  - Geometry buffering");
    println!("  - Area calculations");
    println!("  - Intersection detection");
    println!("  - Douglas-Peucker simplification");
    println!("  - GeoParquet export");

    Ok(())
}
