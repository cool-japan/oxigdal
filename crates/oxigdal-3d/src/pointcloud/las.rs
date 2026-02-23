//! LAS/LAZ point cloud format support
//!
//! Provides comprehensive LAS 1.4 format reading and writing with LAZ compression support.

use crate::error::{Error, Result};
use rstar::{AABB, RTree, RTreeObject};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// LAS point classification codes (ASPRS Standard)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Classification {
    /// Never classified
    NeverClassified = 0,
    /// Unclassified
    Unclassified = 1,
    /// Ground
    Ground = 2,
    /// Low vegetation
    LowVegetation = 3,
    /// Medium vegetation
    MediumVegetation = 4,
    /// High vegetation
    HighVegetation = 5,
    /// Building
    Building = 6,
    /// Low point (noise)
    LowPoint = 7,
    /// Reserved
    Reserved = 8,
    /// Water
    Water = 9,
    /// Rail
    Rail = 10,
    /// Road surface
    RoadSurface = 11,
    /// Reserved (overlap)
    ReservedOverlap = 12,
    /// Wire - guard (shield)
    WireGuard = 13,
    /// Wire - conductor (phase)
    WireConductor = 14,
    /// Transmission tower
    TransmissionTower = 15,
    /// Wire-structure connector (insulator)
    WireConnector = 16,
    /// Bridge deck
    BridgeDeck = 17,
    /// High noise
    HighNoise = 18,
    /// Other/custom
    Other(u8),
}

impl From<u8> for Classification {
    fn from(value: u8) -> Self {
        match value {
            0 => Classification::NeverClassified,
            1 => Classification::Unclassified,
            2 => Classification::Ground,
            3 => Classification::LowVegetation,
            4 => Classification::MediumVegetation,
            5 => Classification::HighVegetation,
            6 => Classification::Building,
            7 => Classification::LowPoint,
            8 => Classification::Reserved,
            9 => Classification::Water,
            10 => Classification::Rail,
            11 => Classification::RoadSurface,
            12 => Classification::ReservedOverlap,
            13 => Classification::WireGuard,
            14 => Classification::WireConductor,
            15 => Classification::TransmissionTower,
            16 => Classification::WireConnector,
            17 => Classification::BridgeDeck,
            18 => Classification::HighNoise,
            other => Classification::Other(other),
        }
    }
}

impl From<Classification> for u8 {
    fn from(class: Classification) -> Self {
        match class {
            Classification::NeverClassified => 0,
            Classification::Unclassified => 1,
            Classification::Ground => 2,
            Classification::LowVegetation => 3,
            Classification::MediumVegetation => 4,
            Classification::HighVegetation => 5,
            Classification::Building => 6,
            Classification::LowPoint => 7,
            Classification::Reserved => 8,
            Classification::Water => 9,
            Classification::Rail => 10,
            Classification::RoadSurface => 11,
            Classification::ReservedOverlap => 12,
            Classification::WireGuard => 13,
            Classification::WireConductor => 14,
            Classification::TransmissionTower => 15,
            Classification::WireConnector => 16,
            Classification::BridgeDeck => 17,
            Classification::HighNoise => 18,
            Classification::Other(val) => val,
        }
    }
}

/// RGB color values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorRgb {
    /// Red channel (0-65535)
    pub red: u16,
    /// Green channel (0-65535)
    pub green: u16,
    /// Blue channel (0-65535)
    pub blue: u16,
}

/// RGB + NIR color values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorRgbNir {
    /// Red channel (0-65535)
    pub red: u16,
    /// Green channel (0-65535)
    pub green: u16,
    /// Blue channel (0-65535)
    pub blue: u16,
    /// Near-infrared channel (0-65535)
    pub nir: u16,
}

/// 3D bounding box
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds3d {
    /// Minimum X coordinate
    pub min_x: f64,
    /// Maximum X coordinate
    pub max_x: f64,
    /// Minimum Y coordinate
    pub min_y: f64,
    /// Maximum Y coordinate
    pub max_y: f64,
    /// Minimum Z coordinate
    pub min_z: f64,
    /// Maximum Z coordinate
    pub max_z: f64,
}

impl Bounds3d {
    /// Create new bounds
    pub fn new(min_x: f64, max_x: f64, min_y: f64, max_y: f64, min_z: f64, max_z: f64) -> Self {
        Self {
            min_x,
            max_x,
            min_y,
            max_y,
            min_z,
            max_z,
        }
    }

    /// Check if bounds contain a point
    pub fn contains(&self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
            && z >= self.min_z
            && z <= self.max_z
    }

    /// Check if bounds intersect
    pub fn intersects(&self, other: &Bounds3d) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }

    /// Get bounds center
    pub fn center(&self) -> (f64, f64, f64) {
        (
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
            (self.min_z + self.max_z) / 2.0,
        )
    }
}

/// Point format enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointFormat {
    /// Format 0: X, Y, Z, Intensity, Return Number, etc.
    Format0 = 0,
    /// Format 1: Format 0 + GPS Time
    Format1 = 1,
    /// Format 2: Format 0 + RGB
    Format2 = 2,
    /// Format 3: Format 1 + RGB
    Format3 = 3,
    /// Format 4: Format 1 + Wave Packets
    Format4 = 4,
    /// Format 5: Format 3 + Wave Packets
    Format5 = 5,
    /// Format 6: Format 1 + Extended (LAS 1.4)
    Format6 = 6,
    /// Format 7: Format 6 + RGB
    Format7 = 7,
    /// Format 8: Format 7 + NIR
    Format8 = 8,
    /// Format 9: Format 6 + Wave Packets
    Format9 = 9,
    /// Format 10: Format 8 + Wave Packets
    Format10 = 10,
}

impl TryFrom<u8> for PointFormat {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(PointFormat::Format0),
            1 => Ok(PointFormat::Format1),
            2 => Ok(PointFormat::Format2),
            3 => Ok(PointFormat::Format3),
            4 => Ok(PointFormat::Format4),
            5 => Ok(PointFormat::Format5),
            6 => Ok(PointFormat::Format6),
            7 => Ok(PointFormat::Format7),
            8 => Ok(PointFormat::Format8),
            9 => Ok(PointFormat::Format9),
            10 => Ok(PointFormat::Format10),
            _ => Err(Error::UnsupportedPointFormat(value)),
        }
    }
}

/// 3D point with attributes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// Z coordinate
    pub z: f64,
    /// Intensity
    pub intensity: u16,
    /// Return number
    pub return_number: u8,
    /// Number of returns
    pub number_of_returns: u8,
    /// Classification
    pub classification: Classification,
    /// Scan angle
    pub scan_angle: i16,
    /// User data
    pub user_data: u8,
    /// Point source ID
    pub point_source_id: u16,
    /// GPS time (optional)
    pub gps_time: Option<f64>,
    /// RGB color (optional)
    pub color: Option<ColorRgb>,
    /// NIR value (optional)
    pub nir: Option<u16>,
}

impl Point {
    /// Create a new point with minimum attributes
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            intensity: 0,
            return_number: 1,
            number_of_returns: 1,
            classification: Classification::Unclassified,
            scan_angle: 0,
            user_data: 0,
            point_source_id: 0,
            gps_time: None,
            color: None,
            nir: None,
        }
    }

    /// Check if point is ground
    pub fn is_ground(&self) -> bool {
        self.classification == Classification::Ground
    }

    /// Check if point is vegetation
    pub fn is_vegetation(&self) -> bool {
        matches!(
            self.classification,
            Classification::LowVegetation
                | Classification::MediumVegetation
                | Classification::HighVegetation
        )
    }

    /// Check if point is building
    pub fn is_building(&self) -> bool {
        self.classification == Classification::Building
    }

    /// Distance to another point
    pub fn distance_to(&self, other: &Point) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// 2D distance (ignoring Z)
    pub fn distance_2d(&self, other: &Point) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

// Implement RTreeObject for spatial indexing
impl RTreeObject for Point {
    type Envelope = AABB<[f64; 3]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.x, self.y, self.z])
    }
}

/// LAS header information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LasHeader {
    /// Version (e.g., "1.4")
    pub version: String,
    /// Point format
    pub point_format: PointFormat,
    /// Number of points
    pub point_count: u64,
    /// Bounds
    pub bounds: Bounds3d,
    /// Scale factors
    pub scale: (f64, f64, f64),
    /// Offset values
    pub offset: (f64, f64, f64),
    /// System identifier
    pub system_identifier: String,
    /// Generating software
    pub generating_software: String,
}

/// Point cloud data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointCloud {
    /// Header information
    pub header: LasHeader,
    /// Points
    pub points: Vec<Point>,
}

impl PointCloud {
    /// Create a new point cloud
    pub fn new(header: LasHeader, points: Vec<Point>) -> Self {
        Self { header, points }
    }

    /// Get number of points
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Get bounds
    pub fn bounds(&self) -> Bounds3d {
        self.header.bounds
    }

    /// Filter points by classification
    pub fn filter_by_classification(&self, class: Classification) -> Vec<&Point> {
        self.points
            .iter()
            .filter(|p| p.classification == class)
            .collect()
    }

    /// Filter ground points
    pub fn ground_points(&self) -> Vec<&Point> {
        self.filter_by_classification(Classification::Ground)
    }

    /// Calculate actual bounds from points
    pub fn calculate_bounds(&self) -> Option<Bounds3d> {
        if self.points.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_z = f64::NEG_INFINITY;

        for point in &self.points {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
            min_z = min_z.min(point.z);
            max_z = max_z.max(point.z);
        }

        Some(Bounds3d::new(min_x, max_x, min_y, max_y, min_z, max_z))
    }
}

/// Spatial index for fast point queries
pub struct SpatialIndex {
    tree: RTree<Point>,
    points: Vec<Point>,
}

impl SpatialIndex {
    /// Create a new spatial index from points
    pub fn new(points: Vec<Point>) -> Self {
        let tree = RTree::bulk_load(points.clone());
        Self { tree, points }
    }

    /// Find nearest point
    pub fn nearest(&self, x: f64, y: f64, z: f64) -> Option<&Point> {
        let mut nearest: Option<&Point> = None;
        let mut min_dist_sq = f64::INFINITY;

        for point in &self.points {
            let dx = point.x - x;
            let dy = point.y - y;
            let dz = point.z - z;
            let dist_sq = dx * dx + dy * dy + dz * dz;

            if dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                nearest = Some(point);
            }
        }

        nearest
    }

    /// Find k nearest points
    pub fn nearest_k(&self, x: f64, y: f64, z: f64, k: usize) -> Vec<&Point> {
        let mut dists: Vec<(usize, f64)> = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let dx = p.x - x;
                let dy = p.y - y;
                let dz = p.z - z;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                (i, dist_sq)
            })
            .collect();

        dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        dists
            .iter()
            .take(k)
            .map(|(i, _)| &self.points[*i])
            .collect()
    }

    /// Find points within radius
    pub fn within_radius(&self, x: f64, y: f64, z: f64, radius: f64) -> Vec<&Point> {
        let radius_sq = radius * radius;
        self.points
            .iter()
            .filter(|p| {
                let dx = p.x - x;
                let dy = p.y - y;
                let dz = p.z - z;
                dx * dx + dy * dy + dz * dz <= radius_sq
            })
            .collect()
    }

    /// Find points within bounds
    pub fn within_bounds(&self, bounds: &Bounds3d) -> Vec<&Point> {
        let aabb = AABB::from_corners(
            [bounds.min_x, bounds.min_y, bounds.min_z],
            [bounds.max_x, bounds.max_y, bounds.max_z],
        );
        self.tree.locate_in_envelope(&aabb).collect()
    }
}

/// Point record for internal processing
#[derive(Debug, Clone)]
pub struct PointRecord {
    /// Raw LAS point
    pub raw: las::Point,
}

/// LAS file reader
pub struct LasReader {
    reader: las::Reader,
    header: LasHeader,
}

impl LasReader {
    /// Open a LAS/LAZ file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let buf_reader = BufReader::new(file);
        let reader = las::Reader::new(buf_reader)?;

        let las_header = reader.header();
        let version = format!(
            "{}.{}",
            las_header.version().major,
            las_header.version().minor
        );
        let fmt_u8 = las_header
            .point_format()
            .to_u8()
            .map_err(|_| Error::UnsupportedPointFormat(0))?;
        let point_format = PointFormat::try_from(fmt_u8)?;

        let bounds = Bounds3d::new(
            las_header.bounds().min.x,
            las_header.bounds().max.x,
            las_header.bounds().min.y,
            las_header.bounds().max.y,
            las_header.bounds().min.z,
            las_header.bounds().max.z,
        );

        let header = LasHeader {
            version,
            point_format,
            point_count: las_header.number_of_points(),
            bounds,
            scale: (
                las_header.transforms().x.scale,
                las_header.transforms().y.scale,
                las_header.transforms().z.scale,
            ),
            offset: (
                las_header.transforms().x.offset,
                las_header.transforms().y.offset,
                las_header.transforms().z.offset,
            ),
            system_identifier: las_header.system_identifier().to_string(),
            generating_software: las_header.generating_software().to_string(),
        };

        Ok(Self { reader, header })
    }

    /// Get header
    pub fn header(&self) -> &LasHeader {
        &self.header
    }

    /// Read all points
    pub fn read_all(&mut self) -> Result<PointCloud> {
        let mut points = Vec::new();

        for result in self.reader.points() {
            let las_point = result?;
            let point = Self::convert_point(&las_point)?;
            points.push(point);
        }

        Ok(PointCloud::new(self.header.clone(), points))
    }

    /// Read points with limit
    pub fn read_n(&mut self, n: usize) -> Result<Vec<Point>> {
        let mut points = Vec::with_capacity(n);

        for (i, result) in self.reader.points().enumerate() {
            if i >= n {
                break;
            }
            let las_point = result?;
            let point = Self::convert_point(&las_point)?;
            points.push(point);
        }

        Ok(points)
    }

    /// Convert LAS point to our Point structure
    fn convert_point(las_point: &las::Point) -> Result<Point> {
        let class_u8: u8 = las_point.classification.into();
        let classification = Classification::from(class_u8);

        let color = las_point.color.map(|c| ColorRgb {
            red: c.red,
            green: c.green,
            blue: c.blue,
        });

        let nir = las_point.nir;

        Ok(Point {
            x: las_point.x,
            y: las_point.y,
            z: las_point.z,
            intensity: las_point.intensity,
            return_number: las_point.return_number,
            number_of_returns: las_point.number_of_returns,
            classification,
            scan_angle: las_point.scan_angle as i16,
            user_data: las_point.user_data,
            point_source_id: las_point.point_source_id,
            gps_time: las_point.gps_time,
            color,
            nir,
        })
    }
}

/// LAS file writer
pub struct LasWriter {
    writer: las::Writer<BufWriter<File>>,
}

impl LasWriter {
    /// Create a new LAS file
    pub fn create<P: AsRef<Path>>(path: P, header: &LasHeader) -> Result<Self> {
        let mut builder = las::Builder::default();

        // Set version
        let version_parts: Vec<&str> = header.version.split('.').collect();
        if version_parts.len() == 2 {
            if let (Ok(major), Ok(minor)) = (
                version_parts[0].parse::<u8>(),
                version_parts[1].parse::<u8>(),
            ) {
                builder.version = las::Version::new(major, minor);
            }
        }

        // Set point format
        builder.point_format = las::point::Format::new(header.point_format as u8)
            .map_err(|e| Error::Las(e.to_string()))?;

        // Set transforms
        builder.transforms = las::Vector {
            x: las::Transform {
                scale: header.scale.0,
                offset: header.offset.0,
            },
            y: las::Transform {
                scale: header.scale.1,
                offset: header.offset.1,
            },
            z: las::Transform {
                scale: header.scale.2,
                offset: header.offset.2,
            },
        };

        // Set system identifier and generating software
        builder.system_identifier = header.system_identifier.clone();
        builder.generating_software = header.generating_software.clone();

        let file = File::create(path)?;
        let buf_writer = BufWriter::new(file);
        let writer = las::Writer::new(
            buf_writer,
            builder
                .into_header()
                .map_err(|e| Error::Las(e.to_string()))?,
        )?;

        Ok(Self { writer })
    }

    /// Write a point
    pub fn write_point(&mut self, point: &Point) -> Result<()> {
        let las_point = las::Point {
            x: point.x,
            y: point.y,
            z: point.z,
            intensity: point.intensity,
            return_number: point.return_number,
            number_of_returns: point.number_of_returns,
            classification: las::point::Classification::new(point.classification.into())
                .map_err(|e| Error::Las(e.to_string()))?,
            scan_angle: point.scan_angle as f32,
            user_data: point.user_data,
            point_source_id: point.point_source_id,
            gps_time: point.gps_time,
            color: point.color.map(|c| las::Color {
                red: c.red,
                green: c.green,
                blue: c.blue,
            }),
            nir: point.nir,
            ..Default::default()
        };

        self.writer.write_point(las_point)?;
        Ok(())
    }

    /// Write multiple points
    pub fn write_points(&mut self, points: &[Point]) -> Result<()> {
        for point in points {
            self.write_point(point)?;
        }
        Ok(())
    }

    /// Finish writing and close file
    pub fn close(mut self) -> Result<()> {
        self.writer.close()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_classification_conversion() {
        assert_eq!(Classification::from(2u8), Classification::Ground);
        assert_eq!(u8::from(Classification::Ground), 2);
    }

    #[test]
    fn test_point_creation() {
        let point = Point::new(1.0, 2.0, 3.0);
        assert_relative_eq!(point.x, 1.0);
        assert_relative_eq!(point.y, 2.0);
        assert_relative_eq!(point.z, 3.0);
        assert_eq!(point.classification, Classification::Unclassified);
    }

    #[test]
    fn test_point_classification_checks() {
        let mut point = Point::new(0.0, 0.0, 0.0);

        point.classification = Classification::Ground;
        assert!(point.is_ground());
        assert!(!point.is_building());

        point.classification = Classification::Building;
        assert!(point.is_building());
        assert!(!point.is_ground());

        point.classification = Classification::HighVegetation;
        assert!(point.is_vegetation());
    }

    #[test]
    fn test_point_distance() {
        let p1 = Point::new(0.0, 0.0, 0.0);
        let p2 = Point::new(3.0, 4.0, 0.0);

        assert_relative_eq!(p1.distance_to(&p2), 5.0);
        assert_relative_eq!(p1.distance_2d(&p2), 5.0);
    }

    #[test]
    fn test_bounds_contains() {
        let bounds = Bounds3d::new(0.0, 10.0, 0.0, 10.0, 0.0, 10.0);

        assert!(bounds.contains(5.0, 5.0, 5.0));
        assert!(!bounds.contains(15.0, 5.0, 5.0));
    }

    #[test]
    fn test_bounds_intersects() {
        let b1 = Bounds3d::new(0.0, 10.0, 0.0, 10.0, 0.0, 10.0);
        let b2 = Bounds3d::new(5.0, 15.0, 5.0, 15.0, 5.0, 15.0);
        let b3 = Bounds3d::new(20.0, 30.0, 20.0, 30.0, 20.0, 30.0);

        assert!(b1.intersects(&b2));
        assert!(!b1.intersects(&b3));
    }

    #[test]
    fn test_bounds_center() {
        let bounds = Bounds3d::new(0.0, 10.0, 0.0, 10.0, 0.0, 10.0);
        let (cx, cy, cz) = bounds.center();

        assert_relative_eq!(cx, 5.0);
        assert_relative_eq!(cy, 5.0);
        assert_relative_eq!(cz, 5.0);
    }

    #[test]
    fn test_point_cloud_creation() {
        let header = LasHeader {
            version: "1.4".to_string(),
            point_format: PointFormat::Format0,
            point_count: 0,
            bounds: Bounds3d::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            scale: (0.01, 0.01, 0.01),
            offset: (0.0, 0.0, 0.0),
            system_identifier: "OxiGDAL".to_string(),
            generating_software: "oxigdal-3d".to_string(),
        };

        let points = vec![Point::new(1.0, 2.0, 3.0), Point::new(4.0, 5.0, 6.0)];

        let cloud = PointCloud::new(header, points);
        assert_eq!(cloud.len(), 2);
        assert!(!cloud.is_empty());
    }

    #[test]
    fn test_point_cloud_bounds_calculation() {
        let header = LasHeader {
            version: "1.4".to_string(),
            point_format: PointFormat::Format0,
            point_count: 0,
            bounds: Bounds3d::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            scale: (0.01, 0.01, 0.01),
            offset: (0.0, 0.0, 0.0),
            system_identifier: "OxiGDAL".to_string(),
            generating_software: "oxigdal-3d".to_string(),
        };

        let points = vec![Point::new(0.0, 0.0, 0.0), Point::new(10.0, 10.0, 10.0)];

        let cloud = PointCloud::new(header, points);
        let bounds = cloud
            .calculate_bounds()
            .expect("Failed to calculate bounds");

        assert_relative_eq!(bounds.min_x, 0.0);
        assert_relative_eq!(bounds.max_x, 10.0);
    }

    #[test]
    fn test_spatial_index() {
        let points = vec![
            Point::new(0.0, 0.0, 0.0),
            Point::new(1.0, 1.0, 1.0),
            Point::new(2.0, 2.0, 2.0),
        ];

        let index = SpatialIndex::new(points);

        let nearest = index.nearest(0.5, 0.5, 0.5);
        assert!(nearest.is_some());

        let within = index.within_radius(0.0, 0.0, 0.0, 2.0);
        assert!(!within.is_empty());
    }
}
