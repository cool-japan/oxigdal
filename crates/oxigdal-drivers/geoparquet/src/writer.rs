//! GeoParquet file writer implementation

use crate::arrow_ext::{GeometryArrayBuilder, add_geoparquet_metadata, create_geometry_field};
use crate::compression::CompressionType;
use crate::error::{GeoParquetError, Result};
use crate::geometry::Geometry;
use crate::metadata::{GeoParquetMetadata, GeometryColumnMetadata, GeometryStatistics};
use crate::spatial::PartitionStrategy;
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{Field, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// GeoParquet file writer
pub struct GeoParquetWriter {
    /// Arrow writer
    writer: ArrowWriter<File>,
    /// Arrow schema
    schema: SchemaRef,
    /// GeoParquet metadata
    metadata: GeoParquetMetadata,
    /// Geometry column name
    geometry_column: String,
    /// Current batch of geometries
    current_batch: Vec<Geometry>,
    /// Batch size for row groups
    batch_size: usize,
    /// Statistics collector
    stats: GeometryStatistics,
    /// Partitioning strategy
    partition_strategy: Option<PartitionStrategy>,
    /// Additional fields beyond geometry
    additional_fields: Vec<Field>,
    /// Additional field data
    additional_data: Vec<Vec<ArrayRef>>,
}

impl GeoParquetWriter {
    /// Creates a new GeoParquet writer
    ///
    /// # Arguments
    /// * `path` - Output file path
    /// * `geometry_column` - Name of the geometry column
    /// * `metadata` - Geometry column metadata
    ///
    /// # Errors
    /// Returns an error if the file cannot be created
    pub fn new<P: AsRef<Path>>(
        path: P,
        geometry_column: impl Into<String>,
        metadata: GeometryColumnMetadata,
    ) -> Result<Self> {
        let geometry_column = geometry_column.into();

        // Create schema with geometry column
        let geom_field = create_geometry_field(&geometry_column, true);
        let schema = Arc::new(Schema::new(vec![geom_field]));

        // Create GeoParquet metadata
        let mut geo_metadata = GeoParquetMetadata::new(&geometry_column);
        geo_metadata.add_column(&geometry_column, metadata);

        // Add metadata to schema
        let metadata_json = geo_metadata.to_json()?;
        let schema = add_geoparquet_metadata((*schema).clone(), metadata_json)?;
        let schema = Arc::new(schema);

        // Create file and writer
        let file = File::create(path.as_ref())?;
        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();

        let writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;

        Ok(Self {
            writer,
            schema: schema.clone(),
            metadata: geo_metadata,
            geometry_column,
            current_batch: Vec::new(),
            batch_size: 1000,
            stats: GeometryStatistics::new(),
            partition_strategy: None,
            additional_fields: Vec::new(),
            additional_data: Vec::new(),
        })
    }

    /// Sets the compression type
    ///
    /// Note: This method currently cannot change compression after writer creation.
    /// Compression must be set at creation time through the builder pattern.
    pub fn with_compression(self, _compression: CompressionType) -> Result<Self> {
        // Note: Changing compression after writer creation is not supported
        // Compression is set during writer creation
        Ok(self)
    }

    /// Sets the batch size (number of rows per row group)
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Sets the spatial partitioning strategy
    pub fn with_partitioning(mut self, strategy: PartitionStrategy) -> Self {
        self.partition_strategy = Some(strategy);
        self
    }

    /// Adds a non-geometry field to the schema
    pub fn add_field(mut self, field: Field) -> Result<Self> {
        if field.name() == &self.geometry_column {
            return Err(GeoParquetError::invalid_schema(
                "Field name conflicts with geometry column",
            ));
        }

        self.additional_fields.push(field);
        Ok(self)
    }

    /// Adds a geometry to the current batch
    pub fn add_geometry(&mut self, geometry: &Geometry) -> Result<()> {
        // Update statistics
        self.stats
            .update(Some(geometry.type_name()), geometry.bbox().as_deref());

        self.current_batch.push(geometry.clone());

        // Flush if batch is full
        if self.current_batch.len() >= self.batch_size {
            self.flush_batch()?;
        }

        Ok(())
    }

    /// Adds multiple geometries
    pub fn add_geometries(&mut self, geometries: &[Geometry]) -> Result<()> {
        for geom in geometries {
            self.add_geometry(geom)?;
        }
        Ok(())
    }

    /// Adds a geometry with associated attribute data
    pub fn add_row(&mut self, geometry: &Geometry, _attributes: &[ArrayRef]) -> Result<()> {
        // For now, just add the geometry
        // Full implementation would handle attributes
        self.add_geometry(geometry)
    }

    /// Flushes the current batch to a row group
    fn flush_batch(&mut self) -> Result<()> {
        if self.current_batch.is_empty() {
            return Ok(());
        }

        // Build geometry array
        let mut geom_builder = GeometryArrayBuilder::with_capacity(self.current_batch.len());
        for geom in &self.current_batch {
            geom_builder.append_geometry(geom)?;
        }
        let geom_array = geom_builder.finish_arc();

        // Create record batch
        let batch = RecordBatch::try_new(self.schema.clone(), vec![geom_array])?;

        // Write batch
        self.writer.write(&batch)?;

        // Clear current batch
        self.current_batch.clear();

        Ok(())
    }

    /// Finalizes the file and writes footer
    pub fn finish(mut self) -> Result<()> {
        // Flush remaining geometries
        self.flush_batch()?;

        // Update metadata with statistics
        if let Ok(column_metadata) = self.metadata.primary_column_metadata() {
            let mut updated_metadata = column_metadata.clone();
            if let Some(bbox) = &self.stats.bbox {
                updated_metadata = updated_metadata.with_bbox(bbox.clone());
            }
            updated_metadata =
                updated_metadata.with_geometry_types(self.stats.geometry_types.clone());

            self.metadata
                .columns
                .insert(self.geometry_column.clone(), updated_metadata);
        }

        // Close the writer
        self.writer.close()?;

        Ok(())
    }

    /// Returns the current statistics
    pub fn statistics(&self) -> &GeometryStatistics {
        &self.stats
    }

    /// Returns the number of geometries written so far
    pub fn count(&self) -> u64 {
        self.stats.count
    }

    /// Returns the geometry column name
    pub fn geometry_column_name(&self) -> &str {
        &self.geometry_column
    }
}

/// Builder for creating a GeoParquet writer with advanced options
pub struct GeoParquetWriterBuilder {
    geometry_column: String,
    metadata: GeometryColumnMetadata,
    batch_size: usize,
    compression: CompressionType,
    partition_strategy: Option<PartitionStrategy>,
    additional_fields: Vec<Field>,
}

impl GeoParquetWriterBuilder {
    /// Creates a new writer builder
    pub fn new(geometry_column: impl Into<String>, metadata: GeometryColumnMetadata) -> Self {
        Self {
            geometry_column: geometry_column.into(),
            metadata,
            batch_size: 1000,
            compression: CompressionType::default(),
            partition_strategy: None,
            additional_fields: Vec::new(),
        }
    }

    /// Sets the batch size
    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Sets the compression type
    pub fn compression(mut self, compression: CompressionType) -> Self {
        self.compression = compression;
        self
    }

    /// Sets the partitioning strategy
    pub fn partitioning(mut self, strategy: PartitionStrategy) -> Self {
        self.partition_strategy = Some(strategy);
        self
    }

    /// Adds an additional field
    pub fn add_field(mut self, field: Field) -> Self {
        self.additional_fields.push(field);
        self
    }

    /// Builds the writer
    pub fn build<P: AsRef<Path>>(self, path: P) -> Result<GeoParquetWriter> {
        let mut writer = GeoParquetWriter::new(path, self.geometry_column, self.metadata)?;
        writer = writer.with_batch_size(self.batch_size);

        if let Some(strategy) = self.partition_strategy {
            writer = writer.with_partitioning(strategy);
        }

        for field in self.additional_fields {
            writer = writer.add_field(field)?;
        }

        Ok(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::metadata::Crs;

    #[test]
    fn test_writer_creation() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_geoparquet.parquet");

        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
        let result = GeoParquetWriter::new(&path, "geometry", metadata);

        assert!(result.is_ok());
        if result.is_ok() {
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn test_writer_builder() {
        let metadata = GeometryColumnMetadata::new_wkb();
        let builder = GeoParquetWriterBuilder::new("geom", metadata)
            .batch_size(500)
            .compression(CompressionType::Gzip);

        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_builder.parquet");

        let result = builder.build(&path);
        assert!(result.is_ok());
        if result.is_ok() {
            let _ = std::fs::remove_file(&path);
        }
    }
}
