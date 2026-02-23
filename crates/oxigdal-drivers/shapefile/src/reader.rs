//! Shapefile reader - coordinates reading from .shp, .dbf, and .shx files
//!
//! This module provides a high-level interface for reading Shapefiles,
//! combining geometry from .shp, attributes from .dbf, and spatial index from .shx.

use crate::dbf::{DbfReader, FieldDescriptor};
use crate::error::{Result, ShapefileError};
use crate::shp::{Shape, ShapefileHeader, ShpReader};
use crate::shx::{IndexEntry, ShxReader};
use oxigdal_core::vector::{
    Coordinate, Feature, Geometry, LineString as CoreLineString,
    MultiLineString as CoreMultiLineString, MultiPoint as CoreMultiPoint, Point as CorePoint,
    Polygon as CorePolygon, PropertyValue,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// A complete Shapefile feature (geometry + attributes)
#[derive(Debug, Clone)]
pub struct ShapefileFeature {
    /// Record number (1-based)
    pub record_number: i32,
    /// Geometry
    pub geometry: Option<Geometry>,
    /// Attributes (field name -> value)
    pub attributes: HashMap<String, PropertyValue>,
}

impl ShapefileFeature {
    /// Creates a new Shapefile feature
    pub fn new(
        record_number: i32,
        geometry: Option<Geometry>,
        attributes: HashMap<String, PropertyValue>,
    ) -> Self {
        Self {
            record_number,
            geometry,
            attributes,
        }
    }

    /// Converts to an OxiGDAL Feature
    pub fn to_oxigdal_feature(&self) -> Result<Feature> {
        let geometry = self
            .geometry
            .clone()
            .ok_or_else(|| ShapefileError::invalid_geometry("feature has no geometry"))?;

        let mut feature = Feature::new(geometry);

        // Convert attributes
        for (key, value) in &self.attributes {
            feature.set_property(key, value.clone());
        }

        Ok(feature)
    }
}

/// Shapefile reader that coordinates .shp, .dbf, and optionally .shx files
pub struct ShapefileReader {
    /// Base path (without extension)
    base_path: PathBuf,
    /// .shp file header
    header: ShapefileHeader,
    /// Field descriptors from .dbf
    field_descriptors: Vec<FieldDescriptor>,
    /// Index entries from .shx (if available)
    index_entries: Option<Vec<IndexEntry>>,
}

impl ShapefileReader {
    /// Opens a Shapefile from a base path (without extension)
    ///
    /// Reads the .shp, .dbf, and optionally .shx files.
    pub fn open<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        let base_path = base_path.as_ref();

        // Construct file paths
        let shp_path = Self::with_extension(base_path, "shp");
        let dbf_path = Self::with_extension(base_path, "dbf");
        let shx_path = Self::with_extension(base_path, "shx");

        // Open .shp file
        let shp_file = File::open(&shp_path).map_err(|_| ShapefileError::MissingFile {
            file_type: ".shp".to_string(),
        })?;
        let shp_reader = BufReader::new(shp_file);
        let shp_reader = ShpReader::new(shp_reader)?;
        let header = shp_reader.header().clone();

        // Open .dbf file
        let dbf_file = File::open(&dbf_path).map_err(|_| ShapefileError::MissingFile {
            file_type: ".dbf".to_string(),
        })?;
        let dbf_reader = BufReader::new(dbf_file);
        let dbf_reader = DbfReader::new(dbf_reader)?;
        let field_descriptors = dbf_reader.field_descriptors().to_vec();

        // Open .shx file (optional)
        let index_entries = if shx_path.exists() {
            let shx_file = File::open(&shx_path).ok();
            if let Some(file) = shx_file {
                let shx_reader = BufReader::new(file);
                let mut shx_reader = ShxReader::new(shx_reader)?;
                Some(shx_reader.read_all_entries()?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            base_path: base_path.to_path_buf(),
            header,
            field_descriptors,
            index_entries,
        })
    }

    /// Returns the Shapefile header
    pub fn header(&self) -> &ShapefileHeader {
        &self.header
    }

    /// Returns the field descriptors
    pub fn field_descriptors(&self) -> &[FieldDescriptor] {
        &self.field_descriptors
    }

    /// Returns the index entries (if .shx was loaded)
    pub fn index_entries(&self) -> Option<&[IndexEntry]> {
        self.index_entries.as_deref()
    }

    /// Reads all features from the Shapefile
    pub fn read_features(&self) -> Result<Vec<ShapefileFeature>> {
        // Open files
        let shp_path = Self::with_extension(&self.base_path, "shp");
        let dbf_path = Self::with_extension(&self.base_path, "dbf");

        let shp_file = File::open(&shp_path)?;
        let shp_reader = BufReader::new(shp_file);
        let mut shp_reader = ShpReader::new(shp_reader)?;

        let dbf_file = File::open(&dbf_path)?;
        let dbf_reader = BufReader::new(dbf_file);
        let mut dbf_reader = DbfReader::new(dbf_reader)?;

        // Read all shape records
        let shape_records = shp_reader.read_all_records()?;

        // Read all DBF records
        let dbf_records = dbf_reader.read_all_records()?;

        // Verify record counts match
        if shape_records.len() != dbf_records.len() {
            return Err(ShapefileError::RecordMismatch {
                shp_count: shape_records.len(),
                dbf_count: dbf_records.len(),
            });
        }

        // Combine into features
        let mut features = Vec::with_capacity(shape_records.len());
        for (shape_record, dbf_record) in shape_records.iter().zip(dbf_records.iter()) {
            let geometry = Self::shape_to_geometry(&shape_record.shape)?;

            // Convert DBF record to attributes
            let attributes = Self::dbf_to_attributes(dbf_record, &self.field_descriptors);

            features.push(ShapefileFeature::new(
                shape_record.record_number,
                geometry,
                attributes,
            ));
        }

        Ok(features)
    }

    /// Converts a Shape to an OxiGDAL Geometry
    fn shape_to_geometry(shape: &Shape) -> Result<Option<Geometry>> {
        match shape {
            Shape::Null => Ok(None),
            Shape::Point(point) => {
                let oxigdal_point = CorePoint::new(point.x, point.y);
                Ok(Some(Geometry::Point(oxigdal_point)))
            }
            Shape::PointZ(point) => {
                // For now, just use X/Y (could extend OxiGDAL to support Z)
                let oxigdal_point = CorePoint::new(point.x, point.y);
                Ok(Some(Geometry::Point(oxigdal_point)))
            }
            Shape::PointM(point) => {
                let oxigdal_point = CorePoint::new(point.x, point.y);
                Ok(Some(Geometry::Point(oxigdal_point)))
            }
            Shape::PolyLine(multi_part) => {
                if multi_part.parts.len() == 1 {
                    // Single part - convert to LineString
                    let coords: Vec<Coordinate> = multi_part
                        .points
                        .iter()
                        .map(|p| Coordinate::new_2d(p.x, p.y))
                        .collect();

                    if coords.len() < 2 {
                        return Ok(None);
                    }

                    let linestring = CoreLineString::new(coords).map_err(|e| {
                        ShapefileError::invalid_geometry(format!("Invalid LineString: {}", e))
                    })?;
                    Ok(Some(Geometry::LineString(linestring)))
                } else {
                    // Multiple parts - convert to MultiLineString
                    let mut linestrings = Vec::new();

                    for i in 0..multi_part.parts.len() {
                        let start_idx = multi_part.parts[i] as usize;
                        let end_idx = if i + 1 < multi_part.parts.len() {
                            multi_part.parts[i + 1] as usize
                        } else {
                            multi_part.points.len()
                        };

                        let coords: Vec<Coordinate> = multi_part.points[start_idx..end_idx]
                            .iter()
                            .map(|p| Coordinate::new_2d(p.x, p.y))
                            .collect();

                        if coords.len() >= 2 {
                            if let Ok(linestring) = CoreLineString::new(coords) {
                                linestrings.push(linestring);
                            }
                        }
                    }

                    if linestrings.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(Geometry::MultiLineString(CoreMultiLineString::new(
                            linestrings,
                        ))))
                    }
                }
            }
            Shape::Polygon(multi_part) => {
                if multi_part.parts.is_empty() {
                    return Ok(None);
                }

                // First part is exterior ring
                let exterior_start = multi_part.parts[0] as usize;
                let exterior_end = if multi_part.parts.len() > 1 {
                    multi_part.parts[1] as usize
                } else {
                    multi_part.points.len()
                };

                let exterior_coords: Vec<Coordinate> = multi_part.points
                    [exterior_start..exterior_end]
                    .iter()
                    .map(|p| Coordinate::new_2d(p.x, p.y))
                    .collect();

                if exterior_coords.len() < 4 {
                    return Ok(None);
                }

                let exterior = CoreLineString::new(exterior_coords).map_err(|e| {
                    ShapefileError::invalid_geometry(format!("Invalid exterior ring: {}", e))
                })?;

                // Remaining parts are interior rings (holes)
                let mut interiors = Vec::new();
                for i in 1..multi_part.parts.len() {
                    let start_idx = multi_part.parts[i] as usize;
                    let end_idx = if i + 1 < multi_part.parts.len() {
                        multi_part.parts[i + 1] as usize
                    } else {
                        multi_part.points.len()
                    };

                    let interior_coords: Vec<Coordinate> = multi_part.points[start_idx..end_idx]
                        .iter()
                        .map(|p| Coordinate::new_2d(p.x, p.y))
                        .collect();

                    if interior_coords.len() >= 4 {
                        if let Ok(interior) = CoreLineString::new(interior_coords) {
                            interiors.push(interior);
                        }
                    }
                }

                let polygon = CorePolygon::new(exterior, interiors).map_err(|e| {
                    ShapefileError::invalid_geometry(format!("Invalid polygon: {}", e))
                })?;

                Ok(Some(Geometry::Polygon(polygon)))
            }
            Shape::MultiPoint(multi_part) => {
                let points: Vec<CorePoint> = multi_part
                    .points
                    .iter()
                    .map(|p| CorePoint::new(p.x, p.y))
                    .collect();

                if points.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Geometry::MultiPoint(CoreMultiPoint::new(points))))
                }
            }
            // Z variants: use the base 2D shape data (Z values are not
            // directly representable in the current OxiGDAL Geometry model)
            Shape::PolyLineZ(shape_z) => {
                Self::shape_to_geometry(&Shape::PolyLine(shape_z.base.clone()))
            }
            Shape::PolygonZ(shape_z) => {
                Self::shape_to_geometry(&Shape::Polygon(shape_z.base.clone()))
            }
            Shape::MultiPointZ(shape_z) => {
                Self::shape_to_geometry(&Shape::MultiPoint(shape_z.base.clone()))
            }
            // M variants: use the base 2D shape data (M values are not
            // directly representable in the current OxiGDAL Geometry model)
            Shape::PolyLineM(shape_m) => {
                Self::shape_to_geometry(&Shape::PolyLine(shape_m.base.clone()))
            }
            Shape::PolygonM(shape_m) => {
                Self::shape_to_geometry(&Shape::Polygon(shape_m.base.clone()))
            }
            Shape::MultiPointM(shape_m) => {
                Self::shape_to_geometry(&Shape::MultiPoint(shape_m.base.clone()))
            }
        }
    }

    /// Converts a DBF record to PropertyValue attributes
    fn dbf_to_attributes(
        dbf_record: &crate::dbf::DbfRecord,
        field_descriptors: &[FieldDescriptor],
    ) -> HashMap<String, PropertyValue> {
        let mut attributes = HashMap::new();

        for (field, value) in field_descriptors.iter().zip(&dbf_record.values) {
            let property_value = match value {
                crate::dbf::FieldValue::String(s) => PropertyValue::String(s.clone()),
                crate::dbf::FieldValue::Integer(i) => PropertyValue::Integer(*i),
                crate::dbf::FieldValue::Float(f) => PropertyValue::Float(*f),
                crate::dbf::FieldValue::Boolean(b) => PropertyValue::Bool(*b),
                crate::dbf::FieldValue::Date(d) => PropertyValue::String(d.clone()),
                crate::dbf::FieldValue::Null => PropertyValue::Null,
            };

            attributes.insert(field.name.clone(), property_value);
        }

        attributes
    }

    /// Helper to add extension to base path
    fn with_extension<P: AsRef<Path>>(base_path: P, ext: &str) -> PathBuf {
        let base = base_path.as_ref();

        // If base already has an extension, replace it
        if base.extension().is_some() {
            base.with_extension(ext)
        } else {
            // Otherwise, add the extension
            let mut path = base.to_path_buf();
            path.set_extension(ext);
            path
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_extension_helper() {
        let base = PathBuf::from("/tmp/test");
        assert_eq!(
            ShapefileReader::with_extension(&base, "shp"),
            PathBuf::from("/tmp/test.shp")
        );

        let base = PathBuf::from("/tmp/test.shp");
        assert_eq!(
            ShapefileReader::with_extension(&base, "dbf"),
            PathBuf::from("/tmp/test.dbf")
        );
    }

    #[test]
    fn test_shapefile_feature_creation() {
        let mut attributes = HashMap::new();
        attributes.insert(
            "name".to_string(),
            PropertyValue::String("Test".to_string()),
        );
        attributes.insert("value".to_string(), PropertyValue::Integer(42));

        let geometry = Some(Geometry::Point(CorePoint::new(10.0, 20.0)));

        let feature = ShapefileFeature::new(1, geometry, attributes);
        assert_eq!(feature.record_number, 1);
        assert!(feature.geometry.is_some());
        assert_eq!(feature.attributes.len(), 2);
    }
}
