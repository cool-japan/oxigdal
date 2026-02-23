//! Arrow schema extensions for GeoParquet
//!
//! This module provides utilities for working with Arrow schemas
//! that contain GeoParquet geometry columns.

mod array;
mod schema;

pub use array::{GeometryArray, GeometryArrayBuilder};
pub use schema::{GeoArrowField, SchemaBuilder, add_geometry_column, is_geometry_column};

use crate::error::Result;
use arrow_schema::{DataType, Field, Schema};

/// Creates an Arrow field for a geometry column
pub fn create_geometry_field(name: impl Into<String>, nullable: bool) -> Field {
    Field::new(name, DataType::Binary, nullable)
}

/// Extracts GeoParquet metadata from Arrow schema
pub fn extract_geoparquet_metadata(schema: &Schema) -> Result<Option<String>> {
    Ok(schema
        .metadata()
        .get(crate::metadata::GEOPARQUET_METADATA_KEY)
        .cloned())
}

/// Adds GeoParquet metadata to Arrow schema
pub fn add_geoparquet_metadata(schema: Schema, metadata_json: String) -> Result<Schema> {
    let mut metadata = schema.metadata().clone();
    metadata.insert(
        crate::metadata::GEOPARQUET_METADATA_KEY.to_string(),
        metadata_json,
    );
    Ok(schema.with_metadata(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_geometry_field() {
        let field = create_geometry_field("geometry", true);
        assert_eq!(field.name(), "geometry");
        assert_eq!(field.data_type(), &DataType::Binary);
        assert!(field.is_nullable());
    }

    #[test]
    fn test_metadata_operations() {
        let schema = Schema::new(vec![create_geometry_field("geom", true)]);

        let metadata_json = r#"{"version":"1.0.0","primary_column":"geom","columns":{}}"#;
        let schema_with_meta = add_geoparquet_metadata(schema, metadata_json.to_string());
        assert!(schema_with_meta.is_ok());

        let extracted = extract_geoparquet_metadata(&schema_with_meta.expect("should work"));
        assert!(extracted.is_ok());
        assert!(extracted.expect("should extract").is_some());
    }
}
