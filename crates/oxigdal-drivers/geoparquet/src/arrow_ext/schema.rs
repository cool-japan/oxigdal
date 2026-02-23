//! Arrow schema utilities for GeoParquet

use crate::error::{GeoParquetError, Result};
use crate::metadata::{Crs, GeometryColumnMetadata};
use arrow_schema::{DataType, Field, Schema};
use std::collections::HashMap;
use std::sync::Arc;

/// Metadata key for GeoParquet geometry column marker
pub const GEO_COLUMN_MARKER: &str = "ARROW:extension:name";
/// GeoParquet extension name
pub const GEOPARQUET_EXTENSION_NAME: &str = "geoarrow.wkb";

/// Extension of Arrow Field for geometry columns
pub struct GeoArrowField {
    /// The underlying Arrow field
    field: Field,
    /// Geometry column metadata
    metadata: GeometryColumnMetadata,
}

impl GeoArrowField {
    /// Creates a new geometry field
    pub fn new(name: impl Into<String>, metadata: GeometryColumnMetadata) -> Self {
        let mut field = Field::new(name, DataType::Binary, true);

        // Add extension metadata
        let mut field_metadata = HashMap::new();
        field_metadata.insert(
            GEO_COLUMN_MARKER.to_string(),
            GEOPARQUET_EXTENSION_NAME.to_string(),
        );
        field = field.with_metadata(field_metadata);

        Self { field, metadata }
    }

    /// Returns the Arrow field
    pub fn field(&self) -> &Field {
        &self.field
    }

    /// Returns the geometry metadata
    pub fn metadata(&self) -> &GeometryColumnMetadata {
        &self.metadata
    }

    /// Consumes self and returns the Arrow field
    pub fn into_field(self) -> Field {
        self.field
    }
}

/// Schema builder with GeoParquet support
pub struct SchemaBuilder {
    fields: Vec<Field>,
    geometry_columns: HashMap<String, GeometryColumnMetadata>,
    primary_column: Option<String>,
}

impl SchemaBuilder {
    /// Creates a new schema builder
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            geometry_columns: HashMap::new(),
            primary_column: None,
        }
    }

    /// Adds a regular field
    pub fn add_field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// Adds a geometry column
    pub fn add_geometry_column(
        mut self,
        name: impl Into<String>,
        metadata: GeometryColumnMetadata,
        is_primary: bool,
    ) -> Self {
        let name_str = name.into();

        let geo_field = GeoArrowField::new(name_str.clone(), metadata.clone());
        self.fields.push(geo_field.into_field());
        self.geometry_columns.insert(name_str.clone(), metadata);

        if is_primary || self.primary_column.is_none() {
            self.primary_column = Some(name_str);
        }

        self
    }

    /// Builds the Arrow schema with GeoParquet metadata
    pub fn build(self) -> Result<(Schema, crate::metadata::GeoParquetMetadata)> {
        if self.geometry_columns.is_empty() {
            return Err(GeoParquetError::invalid_schema(
                "Schema must contain at least one geometry column",
            ));
        }

        let primary_column = self
            .primary_column
            .ok_or_else(|| GeoParquetError::invalid_schema("No primary geometry column set"))?;

        // Create GeoParquet metadata
        let mut geo_metadata = crate::metadata::GeoParquetMetadata::new(primary_column);
        for (name, metadata) in self.geometry_columns {
            geo_metadata.add_column(name, metadata);
        }

        // Create Arrow schema
        let schema = Schema::new(self.fields);

        Ok((schema, geo_metadata))
    }
}

impl Default for SchemaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks if a field is a geometry column
pub fn is_geometry_column(field: &Field) -> bool {
    field.data_type() == &DataType::Binary
        && field
            .metadata()
            .get(GEO_COLUMN_MARKER)
            .is_some_and(|v| v == GEOPARQUET_EXTENSION_NAME)
}

/// Adds a geometry column to an existing schema
pub fn add_geometry_column(
    schema: &Schema,
    name: impl Into<String>,
    metadata: GeometryColumnMetadata,
) -> Result<Schema> {
    let geo_field = GeoArrowField::new(name, metadata);

    let mut fields: Vec<Arc<Field>> = schema.fields().iter().cloned().collect();
    fields.push(Arc::new(geo_field.into_field()));

    Ok(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

/// Extracts geometry column metadata from a field
pub fn extract_geometry_metadata(field: &Field) -> Result<Option<GeometryColumnMetadata>> {
    if !is_geometry_column(field) {
        return Ok(None);
    }

    // For now, return a default WKB metadata
    // In a real implementation, this would be extracted from the schema metadata
    Ok(Some(GeometryColumnMetadata::new_wkb()))
}

/// Creates a simple schema with a single geometry column
pub fn create_simple_geometry_schema(
    geometry_column_name: impl Into<String>,
    crs: Option<Crs>,
) -> Result<(Schema, crate::metadata::GeoParquetMetadata)> {
    let name = geometry_column_name.into();

    let mut metadata = GeometryColumnMetadata::new_wkb();
    if let Some(crs) = crs {
        metadata = metadata.with_crs(crs);
    }

    SchemaBuilder::new()
        .add_geometry_column(name, metadata, true)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Crs;

    #[test]
    fn test_geo_arrow_field() {
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
        let geo_field = GeoArrowField::new("geometry", metadata);

        assert_eq!(geo_field.field().name(), "geometry");
        assert_eq!(geo_field.field().data_type(), &DataType::Binary);
        assert!(is_geometry_column(geo_field.field()));
    }

    #[test]
    fn test_schema_builder() {
        let metadata = GeometryColumnMetadata::new_wkb();

        let result = SchemaBuilder::new()
            .add_field(Field::new("id", DataType::Int64, false))
            .add_field(Field::new("name", DataType::Utf8, true))
            .add_geometry_column("geometry", metadata, true)
            .build();

        assert!(result.is_ok());
        let (schema, geo_meta) = result.expect("should build");
        assert_eq!(schema.fields().len(), 3);
        assert_eq!(geo_meta.primary_column, "geometry");
    }

    #[test]
    fn test_schema_builder_no_geometry() {
        let result = SchemaBuilder::new()
            .add_field(Field::new("id", DataType::Int64, false))
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_create_simple_geometry_schema() {
        let result = create_simple_geometry_schema("geom", Some(Crs::wgs84()));
        assert!(result.is_ok());

        let (schema, metadata) = result.expect("should create");
        assert_eq!(schema.fields().len(), 1);
        assert_eq!(metadata.primary_column, "geom");
    }

    #[test]
    fn test_is_geometry_column() {
        let metadata = GeometryColumnMetadata::new_wkb();
        let geo_field = GeoArrowField::new("geometry", metadata);
        assert!(is_geometry_column(geo_field.field()));

        let regular_field = Field::new("name", DataType::Utf8, true);
        assert!(!is_geometry_column(&regular_field));
    }
}
