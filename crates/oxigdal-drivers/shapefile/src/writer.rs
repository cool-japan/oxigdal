//! Shapefile writer - coordinates writing to .shp, .dbf, and .shx files
//!
//! This module provides a high-level interface for writing Shapefiles,
//! creating geometry in .shp, attributes in .dbf, and spatial index in .shx.

use crate::dbf::{DbfRecord, DbfWriter, FieldDescriptor, FieldType, FieldValue};
use crate::error::{Result, ShapefileError};
use crate::reader::ShapefileFeature;
use crate::shp::header::BoundingBox;
use crate::shp::shapes::{Point, ShapeType};
use crate::shp::{Shape, ShpWriter};
use crate::shx::ShxWriter;
use oxigdal_core::vector::{Feature, Geometry, PropertyValue};
use std::fs::File;
use std::path::{Path, PathBuf};

/// Shapefile writer that coordinates .shp, .dbf, and .shx files
pub struct ShapefileWriter {
    /// Base path (without extension)
    base_path: PathBuf,
    /// Shape type for the Shapefile
    shape_type: ShapeType,
    /// Field descriptors for attributes
    field_descriptors: Vec<FieldDescriptor>,
    /// Bounding box (will be updated as features are added)
    bbox: BoundingBox,
}

impl ShapefileWriter {
    /// Creates a new Shapefile writer
    pub fn new<P: AsRef<Path>>(
        base_path: P,
        shape_type: ShapeType,
        field_descriptors: Vec<FieldDescriptor>,
    ) -> Result<Self> {
        // Initialize with empty bounding box (will be updated)
        let bbox = BoundingBox::new_2d(0.0, 0.0, 0.0, 0.0)?;

        Ok(Self {
            base_path: base_path.as_ref().to_path_buf(),
            shape_type,
            field_descriptors,
            bbox,
        })
    }

    /// Writes features to the Shapefile
    pub fn write_features(&mut self, features: &[ShapefileFeature]) -> Result<()> {
        if features.is_empty() {
            return Err(ShapefileError::invalid_geometry(
                "cannot write empty feature collection",
            ));
        }

        // Calculate bounding box from all features
        self.bbox = Self::calculate_bbox(features)?;

        // Open output files
        let shp_path = Self::with_extension(&self.base_path, "shp");
        let dbf_path = Self::with_extension(&self.base_path, "dbf");
        let shx_path = Self::with_extension(&self.base_path, "shx");

        let shp_file = File::create(&shp_path)?;
        let mut shp_writer = ShpWriter::new(shp_file, self.shape_type, self.bbox.clone());

        let dbf_file = File::create(&dbf_path)?;
        let mut dbf_writer = DbfWriter::new(dbf_file, self.field_descriptors.clone())?;

        let shx_file = File::create(&shx_path)?;
        let mut shx_writer = ShxWriter::new(shx_file, self.shape_type, self.bbox.clone());

        // Write headers
        shp_writer.write_header()?;
        dbf_writer.write_header()?;

        // Write features
        let mut current_offset = 50; // Header is 100 bytes = 50 words

        for feature in features {
            // Convert geometry to Shape
            let shape = Self::geometry_to_shape(&feature.geometry)?;

            // Calculate content length (includes shape type)
            let content_length = 2 + shape.content_length(); // +2 for shape type

            // Add to index (content_length should match .shp record header)
            shx_writer.add_entry(current_offset, content_length);

            // Write shape
            shp_writer.write_record(shape)?;

            // Convert attributes to DBF record
            let dbf_record = Self::attributes_to_dbf(&feature.attributes, &self.field_descriptors)?;
            dbf_writer.write_record(&dbf_record)?;

            // Update offset (record header is 8 bytes = 4 words)
            current_offset += 4 + content_length;
        }

        // Flush all writers before updating headers
        shp_writer.flush()?;
        dbf_writer.flush()?;

        // Update file lengths in headers
        shp_writer.update_file_length()?;
        dbf_writer.update_record_count()?;

        // Finalize and flush remaining files
        shx_writer.write_all()?;
        shx_writer.flush()?;

        // DBF finalize consumes the writer
        dbf_writer.finalize()?;

        // Explicitly drop remaining writers to ensure all data is written
        drop(shp_writer);
        drop(shx_writer);

        Ok(())
    }

    /// Writes OxiGDAL features to the Shapefile
    pub fn write_oxigdal_features(&mut self, features: &[Feature]) -> Result<()> {
        let shapefile_features: Vec<ShapefileFeature> = features
            .iter()
            .enumerate()
            .map(|(i, feature)| {
                let geometry = feature.geometry.clone();
                let attributes: std::collections::HashMap<String, PropertyValue> = feature
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                ShapefileFeature::new((i + 1) as i32, geometry, attributes)
            })
            .collect();

        self.write_features(&shapefile_features)
    }

    /// Calculates the bounding box from features
    fn calculate_bbox(features: &[ShapefileFeature]) -> Result<BoundingBox> {
        let mut x_min = f64::INFINITY;
        let mut y_min = f64::INFINITY;
        let mut x_max = f64::NEG_INFINITY;
        let mut y_max = f64::NEG_INFINITY;

        for feature in features {
            if let Some(geometry) = &feature.geometry {
                match geometry {
                    Geometry::Point(point) => {
                        x_min = x_min.min(point.coord.x);
                        y_min = y_min.min(point.coord.y);
                        x_max = x_max.max(point.coord.x);
                        y_max = y_max.max(point.coord.y);
                    }
                    Geometry::LineString(linestring) => {
                        for coord in &linestring.coords {
                            x_min = x_min.min(coord.x);
                            y_min = y_min.min(coord.y);
                            x_max = x_max.max(coord.x);
                            y_max = y_max.max(coord.y);
                        }
                    }
                    Geometry::Polygon(polygon) => {
                        for coord in &polygon.exterior.coords {
                            x_min = x_min.min(coord.x);
                            y_min = y_min.min(coord.y);
                            x_max = x_max.max(coord.x);
                            y_max = y_max.max(coord.y);
                        }
                        for interior in &polygon.interiors {
                            for coord in &interior.coords {
                                x_min = x_min.min(coord.x);
                                y_min = y_min.min(coord.y);
                                x_max = x_max.max(coord.x);
                                y_max = y_max.max(coord.y);
                            }
                        }
                    }
                    Geometry::MultiPoint(multipoint) => {
                        for point in &multipoint.points {
                            x_min = x_min.min(point.coord.x);
                            y_min = y_min.min(point.coord.y);
                            x_max = x_max.max(point.coord.x);
                            y_max = y_max.max(point.coord.y);
                        }
                    }
                    Geometry::MultiLineString(multilinestring) => {
                        for linestring in &multilinestring.line_strings {
                            for coord in &linestring.coords {
                                x_min = x_min.min(coord.x);
                                y_min = y_min.min(coord.y);
                                x_max = x_max.max(coord.x);
                                y_max = y_max.max(coord.y);
                            }
                        }
                    }
                    Geometry::MultiPolygon(multipolygon) => {
                        for polygon in &multipolygon.polygons {
                            for coord in &polygon.exterior.coords {
                                x_min = x_min.min(coord.x);
                                y_min = y_min.min(coord.y);
                                x_max = x_max.max(coord.x);
                                y_max = y_max.max(coord.y);
                            }
                            for interior in &polygon.interiors {
                                for coord in &interior.coords {
                                    x_min = x_min.min(coord.x);
                                    y_min = y_min.min(coord.y);
                                    x_max = x_max.max(coord.x);
                                    y_max = y_max.max(coord.y);
                                }
                            }
                        }
                    }
                    Geometry::GeometryCollection(collection) => {
                        for geom in &collection.geometries {
                            if let Some((gx_min, gy_min, gx_max, gy_max)) = geom.bounds() {
                                x_min = x_min.min(gx_min);
                                y_min = y_min.min(gy_min);
                                x_max = x_max.max(gx_max);
                                y_max = y_max.max(gy_max);
                            }
                        }
                    }
                }
            }
        }

        if x_min.is_infinite() {
            return Err(ShapefileError::invalid_geometry(
                "could not calculate bounding box",
            ));
        }

        BoundingBox::new_2d(x_min, y_min, x_max, y_max)
    }

    /// Converts an OxiGDAL Geometry to a Shape
    fn geometry_to_shape(geometry: &Option<Geometry>) -> Result<Shape> {
        match geometry {
            None => Ok(Shape::Null),
            Some(Geometry::Point(point)) => {
                let shp_point = Point::new(point.coord.x, point.coord.y);
                Ok(Shape::Point(shp_point))
            }
            Some(Geometry::LineString(linestring)) => {
                let points: Vec<Point> = linestring
                    .coords
                    .iter()
                    .map(|coord| Point::new(coord.x, coord.y))
                    .collect();

                if points.is_empty() {
                    return Err(ShapefileError::invalid_geometry(
                        "LineString must have at least one point",
                    ));
                }

                let parts = vec![0]; // Single part
                let multi_part = crate::shp::shapes::MultiPartShape::new(parts, points)?;
                Ok(Shape::PolyLine(multi_part))
            }
            Some(Geometry::Polygon(polygon)) => {
                let mut all_points = Vec::new();
                let mut parts = Vec::new();

                // Add exterior ring
                parts.push(all_points.len() as i32);
                for coord in &polygon.exterior.coords {
                    all_points.push(Point::new(coord.x, coord.y));
                }

                // Add interior rings (holes)
                for interior in &polygon.interiors {
                    parts.push(all_points.len() as i32);
                    for coord in &interior.coords {
                        all_points.push(Point::new(coord.x, coord.y));
                    }
                }

                if all_points.is_empty() {
                    return Err(ShapefileError::invalid_geometry(
                        "Polygon must have at least one point",
                    ));
                }

                let multi_part = crate::shp::shapes::MultiPartShape::new(parts, all_points)?;
                Ok(Shape::Polygon(multi_part))
            }
            Some(Geometry::MultiPoint(multipoint)) => {
                let points: Vec<Point> = multipoint
                    .points
                    .iter()
                    .map(|pt| Point::new(pt.coord.x, pt.coord.y))
                    .collect();

                if points.is_empty() {
                    return Err(ShapefileError::invalid_geometry(
                        "MultiPoint must have at least one point",
                    ));
                }

                let parts: Vec<i32> = (0..points.len() as i32).collect();
                let multi_part = crate::shp::shapes::MultiPartShape::new(parts, points)?;
                Ok(Shape::MultiPoint(multi_part))
            }
            Some(Geometry::MultiLineString(multilinestring)) => {
                let mut all_points = Vec::new();
                let mut parts = Vec::new();

                for linestring in &multilinestring.line_strings {
                    parts.push(all_points.len() as i32);
                    for coord in &linestring.coords {
                        all_points.push(Point::new(coord.x, coord.y));
                    }
                }

                if all_points.is_empty() {
                    return Err(ShapefileError::invalid_geometry(
                        "MultiLineString must have at least one point",
                    ));
                }

                let multi_part = crate::shp::shapes::MultiPartShape::new(parts, all_points)?;
                Ok(Shape::PolyLine(multi_part))
            }
            Some(Geometry::MultiPolygon(multipolygon)) => {
                let mut all_points = Vec::new();
                let mut parts = Vec::new();

                for polygon in &multipolygon.polygons {
                    // Add exterior ring
                    parts.push(all_points.len() as i32);
                    for coord in &polygon.exterior.coords {
                        all_points.push(Point::new(coord.x, coord.y));
                    }

                    // Add interior rings (holes)
                    for interior in &polygon.interiors {
                        parts.push(all_points.len() as i32);
                        for coord in &interior.coords {
                            all_points.push(Point::new(coord.x, coord.y));
                        }
                    }
                }

                if all_points.is_empty() {
                    return Err(ShapefileError::invalid_geometry(
                        "MultiPolygon must have at least one point",
                    ));
                }

                let multi_part = crate::shp::shapes::MultiPartShape::new(parts, all_points)?;
                Ok(Shape::Polygon(multi_part))
            }
            Some(Geometry::GeometryCollection(_)) => Err(ShapefileError::invalid_geometry(
                "GeometryCollection is not supported in Shapefile format",
            )),
        }
    }

    /// Converts attributes to a DBF record
    fn attributes_to_dbf(
        attributes: &std::collections::HashMap<String, PropertyValue>,
        field_descriptors: &[FieldDescriptor],
    ) -> Result<DbfRecord> {
        let mut values = Vec::with_capacity(field_descriptors.len());

        for field in field_descriptors {
            let value = attributes
                .get(&field.name)
                .cloned()
                .unwrap_or(PropertyValue::Null);

            let dbf_value = match value {
                PropertyValue::String(s) => FieldValue::String(s),
                PropertyValue::Integer(i) => FieldValue::Integer(i),
                PropertyValue::Float(f) => FieldValue::Float(f),
                PropertyValue::Bool(b) => FieldValue::Boolean(b),
                PropertyValue::Null => FieldValue::Null,
                PropertyValue::UInteger(u) => FieldValue::Integer(u as i64),
                PropertyValue::Array(_) | PropertyValue::Object(_) => FieldValue::Null,
            };

            values.push(dbf_value);
        }

        Ok(DbfRecord::new(values))
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

/// Builder for creating Shapefile field descriptors
pub struct ShapefileSchemaBuilder {
    fields: Vec<FieldDescriptor>,
}

impl ShapefileSchemaBuilder {
    /// Creates a new schema builder
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Adds a character field
    pub fn add_character_field(mut self, name: &str, length: u8) -> Result<Self> {
        let field = FieldDescriptor::new(name.to_string(), FieldType::Character, length, 0)?;
        self.fields.push(field);
        Ok(self)
    }

    /// Adds a numeric field
    pub fn add_numeric_field(mut self, name: &str, length: u8, decimals: u8) -> Result<Self> {
        let field = FieldDescriptor::new(name.to_string(), FieldType::Number, length, decimals)?;
        self.fields.push(field);
        Ok(self)
    }

    /// Adds a logical field
    pub fn add_logical_field(mut self, name: &str) -> Result<Self> {
        let field = FieldDescriptor::new(name.to_string(), FieldType::Logical, 1, 0)?;
        self.fields.push(field);
        Ok(self)
    }

    /// Adds a date field
    pub fn add_date_field(mut self, name: &str) -> Result<Self> {
        let field = FieldDescriptor::new(name.to_string(), FieldType::Date, 8, 0)?;
        self.fields.push(field);
        Ok(self)
    }

    /// Builds the field descriptors
    pub fn build(self) -> Vec<FieldDescriptor> {
        self.fields
    }
}

impl Default for ShapefileSchemaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::assertions_on_constants)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_schema_builder() {
        let schema = ShapefileSchemaBuilder::new()
            .add_character_field("NAME", 50)
            .expect("Failed to add NAME field")
            .add_numeric_field("VALUE", 10, 2)
            .expect("Failed to add VALUE field")
            .add_logical_field("ACTIVE")
            .expect("Failed to add ACTIVE field")
            .build();

        assert_eq!(schema.len(), 3);
        assert_eq!(schema[0].name, "NAME");
        assert_eq!(schema[0].field_type, FieldType::Character);
        assert_eq!(schema[1].name, "VALUE");
        assert_eq!(schema[2].name, "ACTIVE");
    }

    #[test]
    fn test_bbox_calculation() {
        let features = vec![
            ShapefileFeature::new(
                1,
                Some(Geometry::Point(oxigdal_core::vector::Point::new(
                    10.0, 20.0,
                ))),
                HashMap::new(),
            ),
            ShapefileFeature::new(
                2,
                Some(Geometry::Point(oxigdal_core::vector::Point::new(
                    30.0, 40.0,
                ))),
                HashMap::new(),
            ),
            ShapefileFeature::new(
                3,
                Some(Geometry::Point(oxigdal_core::vector::Point::new(
                    -5.0, 15.0,
                ))),
                HashMap::new(),
            ),
        ];

        let bbox = ShapefileWriter::calculate_bbox(&features).expect("Failed to calculate bbox");
        assert!((bbox.x_min - (-5.0)).abs() < f64::EPSILON);
        assert!((bbox.y_min - 15.0).abs() < f64::EPSILON);
        assert!((bbox.x_max - 30.0).abs() < f64::EPSILON);
        assert!((bbox.y_max - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_geometry_to_shape() {
        let geometry = Some(Geometry::Point(oxigdal_core::vector::Point::new(
            10.5, 20.3,
        )));
        let shape =
            ShapefileWriter::geometry_to_shape(&geometry).expect("Failed to convert geometry");

        if let Shape::Point(point) = shape {
            assert!((point.x - 10.5).abs() < f64::EPSILON);
            assert!((point.y - 20.3).abs() < f64::EPSILON);
        } else {
            assert!(false, "Expected Point shape");
        }
    }
}
