//! Test data generation utilities
//!
//! This module provides tools for generating test data for OxiGDAL operations.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Test data generator
pub struct DataGenerator {
    /// Random seed
    seed: u64,
}

impl DataGenerator {
    /// Create a new data generator
    pub fn new() -> Self {
        Self { seed: 12345 }
    }

    /// Create a generator with a specific seed
    pub fn with_seed(seed: u64) -> Self {
        Self { seed }
    }

    /// Generate raster data
    pub fn generate_raster(&self, width: usize, height: usize, pattern: RasterPattern) -> Vec<f64> {
        let mut data = vec![0.0; width * height];

        match pattern {
            RasterPattern::Flat(value) => {
                data.fill(value);
            }
            RasterPattern::Gradient {
                from,
                to,
                direction,
            } => {
                for y in 0..height {
                    for x in 0..width {
                        let t = match direction {
                            GradientDirection::Horizontal => x as f64 / (width - 1) as f64,
                            GradientDirection::Vertical => y as f64 / (height - 1) as f64,
                            GradientDirection::Diagonal => {
                                ((x + y) as f64) / ((width + height - 2) as f64)
                            }
                        };
                        data[y * width + x] = from + (to - from) * t;
                    }
                }
            }
            RasterPattern::Checkerboard {
                size,
                color1,
                color2,
            } => {
                for y in 0..height {
                    for x in 0..width {
                        let is_odd = ((x / size) + (y / size)) % 2 == 1;
                        data[y * width + x] = if is_odd { color1 } else { color2 };
                    }
                }
            }
            RasterPattern::Noise { min, max } => {
                for (i, item) in data.iter_mut().enumerate() {
                    *item = min + (max - min) * self.pseudo_random(i);
                }
            }
            RasterPattern::Sine {
                amplitude,
                frequency,
            } => {
                use std::f64::consts::PI;
                for y in 0..height {
                    for x in 0..width {
                        let phase = 2.0 * PI * frequency * (x as f64 / width as f64);
                        data[y * width + x] = amplitude * phase.sin();
                    }
                }
            }
        }

        data
    }

    /// Simple pseudo-random number generator (LCG)
    fn pseudo_random(&self, index: usize) -> f64 {
        let a = 1103515245u64;
        let c = 12345u64;
        let m = 2u64.pow(31);

        let x = ((a
            .wrapping_mul(self.seed.wrapping_add(index as u64))
            .wrapping_add(c))
            % m) as f64;
        x / m as f64
    }

    /// Generate vector features (points)
    pub fn generate_points(&self, count: usize, bounds: Bounds) -> Vec<Point> {
        let mut points = Vec::with_capacity(count);

        for i in 0..count {
            let x = bounds.min_x + (bounds.max_x - bounds.min_x) * self.pseudo_random(i * 2);
            let y = bounds.min_y + (bounds.max_y - bounds.min_y) * self.pseudo_random(i * 2 + 1);

            points.push(Point { x, y });
        }

        points
    }

    /// Generate regular grid of points
    pub fn generate_grid(&self, rows: usize, cols: usize, bounds: Bounds) -> Vec<Point> {
        let mut points = Vec::with_capacity(rows * cols);

        let dx = (bounds.max_x - bounds.min_x) / (cols - 1) as f64;
        let dy = (bounds.max_y - bounds.min_y) / (rows - 1) as f64;

        for row in 0..rows {
            for col in 0..cols {
                let x = bounds.min_x + col as f64 * dx;
                let y = bounds.min_y + row as f64 * dy;
                points.push(Point { x, y });
            }
        }

        points
    }
}

impl Default for DataGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Raster pattern
#[derive(Debug, Clone)]
pub enum RasterPattern {
    /// Flat value
    Flat(f64),
    /// Gradient
    Gradient {
        /// Start value
        from: f64,
        /// End value
        to: f64,
        /// Direction
        direction: GradientDirection,
    },
    /// Checkerboard pattern
    Checkerboard {
        /// Cell size
        size: usize,
        /// Color 1
        color1: f64,
        /// Color 2
        color2: f64,
    },
    /// Random noise
    Noise {
        /// Minimum value
        min: f64,
        /// Maximum value
        max: f64,
    },
    /// Sine wave
    Sine {
        /// Amplitude
        amplitude: f64,
        /// Frequency
        frequency: f64,
    },
}

/// Gradient direction
#[derive(Debug, Clone, Copy)]
pub enum GradientDirection {
    /// Horizontal (left to right)
    Horizontal,
    /// Vertical (top to bottom)
    Vertical,
    /// Diagonal (top-left to bottom-right)
    Diagonal,
}

/// 2D point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
}

/// Bounding box
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    /// Minimum X
    pub min_x: f64,
    /// Minimum Y
    pub min_y: f64,
    /// Maximum X
    pub max_x: f64,
    /// Maximum Y
    pub max_y: f64,
}

impl Bounds {
    /// Create new bounds
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }
}

/// File generator for creating test files
pub struct FileGenerator;

impl FileGenerator {
    /// Generate a simple GeoTIFF-like file
    pub fn generate_geotiff(_path: &Path, _width: usize, _height: usize) -> Result<()> {
        // Placeholder - would need actual GeoTIFF writer
        Ok(())
    }

    /// Generate a simple GeoJSON file
    pub fn generate_geojson(path: &Path, points: &[Point]) -> Result<()> {
        use std::io::Write;

        let mut geojson =
            String::from("{\n  \"type\": \"FeatureCollection\",\n  \"features\": [\n");

        for (i, point) in points.iter().enumerate() {
            geojson.push_str(&format!(
                "    {{\n      \"type\": \"Feature\",\n      \"geometry\": {{\n        \"type\": \"Point\",\n        \"coordinates\": [{}, {}]\n      }},\n      \"properties\": {{\n        \"id\": {}\n      }}\n    }}",
                point.x, point.y, i
            ));

            if i < points.len() - 1 {
                geojson.push_str(",\n");
            } else {
                geojson.push('\n');
            }
        }

        geojson.push_str("  ]\n}");

        let mut file = std::fs::File::create(path)?;
        file.write_all(geojson.as_bytes())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_creation() {
        let generator = DataGenerator::new();
        assert_eq!(generator.seed, 12345);
    }

    #[test]
    fn test_generate_flat_raster() {
        let generator = DataGenerator::new();
        let data = generator.generate_raster(10, 10, RasterPattern::Flat(42.0));
        assert_eq!(data.len(), 100);
        assert!(data.iter().all(|&v| v == 42.0));
    }

    #[test]
    fn test_generate_gradient_raster() {
        let generator = DataGenerator::new();
        let data = generator.generate_raster(
            10,
            10,
            RasterPattern::Gradient {
                from: 0.0,
                to: 100.0,
                direction: GradientDirection::Horizontal,
            },
        );
        assert_eq!(data.len(), 100);
        assert_eq!(data[0], 0.0); // First column
        assert!((data[9] - 100.0).abs() < 0.01); // Last column
    }

    #[test]
    fn test_generate_checkerboard() {
        let generator = DataGenerator::new();
        let data = generator.generate_raster(
            10,
            10,
            RasterPattern::Checkerboard {
                size: 5,
                color1: 0.0,
                color2: 100.0,
            },
        );
        assert_eq!(data.len(), 100);
    }

    #[test]
    fn test_generate_noise() {
        let generator = DataGenerator::new();
        let data = generator.generate_raster(
            10,
            10,
            RasterPattern::Noise {
                min: 0.0,
                max: 100.0,
            },
        );
        assert_eq!(data.len(), 100);
        assert!(data.iter().all(|&v| (0.0..=100.0).contains(&v)));
    }

    #[test]
    fn test_generate_points() {
        let generator = DataGenerator::new();
        let bounds = Bounds::new(0.0, 0.0, 100.0, 100.0);
        let points = generator.generate_points(10, bounds);
        assert_eq!(points.len(), 10);
        assert!(points.iter().all(|p| p.x >= 0.0 && p.x <= 100.0));
        assert!(points.iter().all(|p| p.y >= 0.0 && p.y <= 100.0));
    }

    #[test]
    fn test_generate_grid() {
        let generator = DataGenerator::new();
        let bounds = Bounds::new(0.0, 0.0, 100.0, 100.0);
        let points = generator.generate_grid(5, 5, bounds);
        assert_eq!(points.len(), 25);
    }

    #[test]
    fn test_generate_geojson() -> Result<()> {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new()?;
        let points = vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }];

        FileGenerator::generate_geojson(temp_file.path(), &points)?;

        let content = std::fs::read_to_string(temp_file.path())?;
        assert!(content.contains("FeatureCollection"));
        assert!(content.contains("Point"));

        Ok(())
    }
}
