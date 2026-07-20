//! GeoParquet file writer implementation

use crate::arrow_ext::{
    GeometryArrayBuilder, add_geoparquet_metadata, create_geometry_field, create_geometry_field_for,
};
use crate::compression::CompressionType;
use crate::error::{GeoParquetError, Result};
use crate::geometry::{Geometry, encode_native_array};
use crate::metadata::{
    CoordDim, EncodingType, GeoParquetMetadata, GeometryColumnMetadata, GeometryStatistics,
};
use crate::spatial::PartitionStrategy;
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{Field, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;
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
    /// Geometry column encoding selected at construction.
    encoding: EncodingType,
    /// Coordinate dimensionality used for native encodings.
    coord_dim: CoordDim,
}

impl GeoParquetWriter {
    /// Creates a new GeoParquet writer with uncompressed output.
    ///
    /// Use [`GeoParquetWriterBuilder`] when you need a specific compression codec.
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
        Self::new_with_compression(
            path,
            geometry_column,
            metadata,
            CompressionType::Uncompressed,
        )
    }

    /// Creates a new GeoParquet writer with the given compression codec.
    ///
    /// Defaults to WKB encoding with 2-D coordinates.  For native (GeoArrow)
    /// encodings, use [`Self::new_with_encoding`] or the
    /// [`GeoParquetWriterBuilder`] API.
    ///
    /// # Arguments
    /// * `path` - Output file path
    /// * `geometry_column` - Name of the geometry column
    /// * `metadata` - Geometry column metadata
    /// * `compression` - Compression codec to use
    ///
    /// # Errors
    /// Returns an error if the file cannot be created
    pub fn new_with_compression<P: AsRef<Path>>(
        path: P,
        geometry_column: impl Into<String>,
        metadata: GeometryColumnMetadata,
        compression: CompressionType,
    ) -> Result<Self> {
        let encoding = metadata.encoding;
        Self::new_with_encoding(
            path,
            geometry_column,
            metadata,
            compression,
            encoding,
            CoordDim::Xy,
        )
    }

    /// Creates a new GeoParquet writer with explicit encoding and coordinate
    /// dimensionality.
    ///
    /// `encoding` *must* match the encoding declared in `metadata.encoding`;
    /// passing different values is an error.  When `encoding` is a native
    /// GeoArrow shape, `coord_dim` selects the per-coordinate arity (2 / 3 /
    /// 4) and the schema is built via [`create_geometry_field_for`] rather
    /// than the legacy `Binary` field.
    pub fn new_with_encoding<P: AsRef<Path>>(
        path: P,
        geometry_column: impl Into<String>,
        metadata: GeometryColumnMetadata,
        compression: CompressionType,
        encoding: EncodingType,
        coord_dim: CoordDim,
    ) -> Result<Self> {
        if metadata.encoding != encoding {
            return Err(GeoParquetError::invalid_encoding(format!(
                "metadata.encoding ({:?}) and writer encoding ({:?}) must match",
                metadata.encoding, encoding
            )));
        }
        let geometry_column = geometry_column.into();

        // Build the appropriate Arrow field for this encoding.
        let geom_field = match encoding {
            EncodingType::Wkb => create_geometry_field(&geometry_column, true),
            _ => create_geometry_field_for(
                &geometry_column,
                encoding,
                coord_dim,
                true,
                metadata.crs.as_ref(),
            ),
        };
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
            .set_compression(compression.to_parquet())
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
            encoding,
            coord_dim,
        })
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

    /// Flushes the current batch to a row group.
    ///
    /// Dispatches on the writer's `encoding`:
    ///
    /// * [`EncodingType::Wkb`] — encodes each geometry to WKB and stores the
    ///   bytes in a `BinaryArray`.
    /// * Native GeoArrow encodings — validates that every buffered geometry
    ///   matches `encoding` (mixed types in a native column are forbidden by
    ///   spec) and emits a structured Arrow array via
    ///   [`encode_native_array`].
    fn flush_batch(&mut self) -> Result<()> {
        if self.current_batch.is_empty() {
            return Ok(());
        }

        let geom_array: ArrayRef = match self.encoding {
            EncodingType::Wkb => {
                let mut geom_builder =
                    GeometryArrayBuilder::with_capacity(self.current_batch.len());
                for geom in &self.current_batch {
                    geom_builder.append_geometry(geom)?;
                }
                geom_builder.finish_arc()
            }
            _ => {
                // Native encoding: validate uniformity then encode through
                // `encode_native_array`.  The encoder itself returns
                // `InvalidEncoding` for mixed types — this guard provides a
                // clearer error tied to the writer's column metadata.
                validate_uniform_native_types(self.encoding, &self.current_batch)?;
                encode_native_array(&self.current_batch, self.encoding, self.coord_dim)?
            }
        };

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

/// Validates that every geometry in `batch` is compatible with the declared
/// native `encoding`.  Mixed types in a native column are forbidden by the
/// GeoParquet 1.1 spec.
fn validate_uniform_native_types(encoding: EncodingType, batch: &[Geometry]) -> Result<()> {
    let expected_name = match encoding {
        EncodingType::Wkb => return Ok(()),
        EncodingType::Point => "Point",
        EncodingType::LineString => "LineString",
        EncodingType::Polygon => "Polygon",
        EncodingType::MultiPoint => "MultiPoint",
        EncodingType::MultiLineString => "MultiLineString",
        EncodingType::MultiPolygon => "MultiPolygon",
    };
    for (i, g) in batch.iter().enumerate() {
        if g.type_name() != expected_name {
            return Err(GeoParquetError::invalid_encoding(format!(
                "row {i}: native column declared as {expected_name} but geometry is {}",
                g.type_name()
            )));
        }
    }
    Ok(())
}

/// Builder for creating a GeoParquet writer with advanced options
pub struct GeoParquetWriterBuilder {
    geometry_column: String,
    metadata: GeometryColumnMetadata,
    batch_size: usize,
    compression: CompressionType,
    partition_strategy: Option<PartitionStrategy>,
    additional_fields: Vec<Field>,
    encoding: EncodingType,
    coord_dim: CoordDim,
}

impl GeoParquetWriterBuilder {
    /// Creates a new writer builder.
    ///
    /// The encoding defaults to whatever is declared in `metadata.encoding`
    /// (typically [`EncodingType::Wkb`] for back-compat).  Coordinate
    /// dimensionality defaults to [`CoordDim::Xy`].
    pub fn new(geometry_column: impl Into<String>, metadata: GeometryColumnMetadata) -> Self {
        let encoding = metadata.encoding;
        Self {
            geometry_column: geometry_column.into(),
            metadata,
            batch_size: 1000,
            compression: CompressionType::default(),
            partition_strategy: None,
            additional_fields: Vec::new(),
            encoding,
            coord_dim: CoordDim::Xy,
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

    /// Selects the geometry column encoding (WKB or one of the GeoArrow
    /// native shapes).  Updates the embedded
    /// [`GeometryColumnMetadata::encoding`] in lockstep so the file's `geo`
    /// metadata blob reflects the choice.
    pub fn encoding(mut self, e: EncodingType) -> Self {
        self.encoding = e;
        self.metadata.encoding = e;
        self
    }

    /// Selects the coordinate dimensionality used by native encodings.
    /// Ignored when the writer encoding is WKB.
    pub fn coord_dim(mut self, d: CoordDim) -> Self {
        self.coord_dim = d;
        self
    }

    /// Builds the writer
    pub fn build<P: AsRef<Path>>(self, path: P) -> Result<GeoParquetWriter> {
        let mut writer = GeoParquetWriter::new_with_encoding(
            path,
            self.geometry_column,
            self.metadata,
            self.compression,
            self.encoding,
            self.coord_dim,
        )?;
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
