//! GeoParquet file writer implementation
//!
//! The writer supports three optional column groups beyond the mandatory
//! geometry column, all wired into the actual Arrow schema and every emitted
//! [`RecordBatch`]:
//!
//! * **Attribute columns** — declared with [`GeoParquetWriter::add_field`] (or
//!   [`GeoParquetWriterBuilder::add_field`]) and populated per row with
//!   [`GeoParquetWriter::add_row`].  Each `add_row` supplies one single-element
//!   Arrow array per declared field; the values are buffered and concatenated
//!   into full attribute columns at flush time.
//! * **GeoParquet 1.1 `covering.bbox` columns** — enabled with
//!   [`GeoParquetWriterBuilder::with_covering_bbox`] (or
//!   [`GeoParquetWriter::with_covering_bbox`]).  A per-row bounding box is
//!   computed alongside each geometry and written as a struct column named
//!   `bbox` with `xmin` / `ymin` / `xmax` / `ymax` `Float64` children, and the
//!   file's `geo` metadata declares the matching `covering.bbox` object so this
//!   crate's own row-group pruning fast path becomes available for files it
//!   writes.
//!
//! Because the Arrow schema must be fixed before the underlying
//! [`ArrowWriter`] is constructed, the writer defers `ArrowWriter` creation
//! until the first flush (or [`finish`](GeoParquetWriter::finish)).  This lets
//! attribute fields and the covering option be declared after construction but
//! before any data is written; attempts to declare them once the schema has
//! been frozen return a typed error rather than silently dropping data.

use crate::arrow_ext::{
    GeometryArrayBuilder, add_geoparquet_metadata, create_geometry_field, create_geometry_field_for,
};
use crate::compression::CompressionType;
use crate::error::{GeoParquetError, Result};
use crate::geometry::{Geometry, encode_native_array};
use crate::metadata::{
    CoordDim, Covering, EncodingType, GeoParquetMetadata, GeometryColumnMetadata,
    GeometryStatistics,
};
use crate::spatial::PartitionStrategy;
use arrow_array::builder::Float64Builder;
use arrow_array::{Array, ArrayRef, RecordBatch, StructArray};
use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// Struct-column name used for the GeoParquet 1.1 `covering.bbox` columns
/// emitted by [`GeoParquetWriter`].  Matches the common VIDA / GeoParquet 1.1
/// convention of a struct column literally named `bbox`.
const COVERING_BBOX_COLUMN: &str = "bbox";

/// Returns the Arrow [`Fields`] for the covering bbox struct: four nullable
/// `Float64` extent columns.
fn covering_bbox_fields() -> Fields {
    Fields::from(vec![
        Field::new("xmin", DataType::Float64, true),
        Field::new("ymin", DataType::Float64, true),
        Field::new("xmax", DataType::Float64, true),
        Field::new("ymax", DataType::Float64, true),
    ])
}

/// Returns the Arrow [`Field`] for the covering bbox struct column.
fn covering_bbox_field() -> Field {
    Field::new(
        COVERING_BBOX_COLUMN,
        DataType::Struct(covering_bbox_fields()),
        false,
    )
}

/// Lazily-constructed output sink.
///
/// The `ArrowWriter` cannot exist before the schema is finalised (which in
/// turn depends on attribute fields / covering being declared), so the file is
/// opened eagerly at construction — surfacing path errors early — but the
/// `ArrowWriter` is only built on the first flush.
enum WriterInner {
    /// File created; `ArrowWriter` not yet constructed (schema still open).
    Pending {
        /// The created output file.
        file: File,
        /// Compression codec to apply when the writer is activated.
        compression: CompressionType,
    },
    /// `ArrowWriter` active; schema is now frozen.
    Active(Box<ArrowWriter<File>>),
}

/// GeoParquet file writer
pub struct GeoParquetWriter {
    /// Lazily-activated output sink.
    inner: Option<WriterInner>,
    /// Finalised Arrow schema (built on first flush, then cached).
    output_schema: Option<SchemaRef>,
    /// Geometry column Arrow field (built at construction).
    geom_field: Field,
    /// Base geometry column metadata supplied at construction.
    column_metadata: GeometryColumnMetadata,
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
    /// Additional (attribute) fields beyond geometry, in column order.
    additional_fields: Vec<Field>,
    /// Buffered per-row attribute values, parallel to `additional_fields`.
    /// `attribute_columns[col]` holds one single-element array per buffered
    /// row; concatenated into a full column at flush.
    attribute_columns: Vec<Vec<ArrayRef>>,
    /// Whether to emit GeoParquet 1.1 `covering.bbox` columns.
    covering_enabled: bool,
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
    ///
    /// The output file is created immediately (so path errors surface here),
    /// but the underlying [`ArrowWriter`] and the final Arrow schema are only
    /// constructed on the first flush — allowing attribute columns
    /// ([`add_field`](Self::add_field)) and covering bbox columns
    /// ([`with_covering_bbox`](Self::with_covering_bbox)) to be declared before
    /// any data is written.
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

        // Create the output file eagerly so an invalid path fails now.
        let file = File::create(path.as_ref())?;

        Ok(Self {
            inner: Some(WriterInner::Pending { file, compression }),
            output_schema: None,
            geom_field,
            column_metadata: metadata,
            geometry_column,
            current_batch: Vec::new(),
            batch_size: 1000,
            stats: GeometryStatistics::new(),
            partition_strategy: None,
            additional_fields: Vec::new(),
            attribute_columns: Vec::new(),
            covering_enabled: false,
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

    /// Enables (or disables) emission of GeoParquet 1.1 `covering.bbox` columns.
    ///
    /// When enabled, every flushed batch gains a struct column named `bbox`
    /// (children `xmin` / `ymin` / `xmax` / `ymax`, `Float64`) holding each
    /// row's geometry bounding box, and the file's `geo` metadata declares the
    /// matching `covering.bbox` object.  This lets the crate's own reader take
    /// the row-group-pruning fast path on files it wrote, instead of decoding
    /// every geometry.
    ///
    /// Must be called before any geometry is written; changing it after the
    /// schema has been frozen has no effect on the already-written schema.
    pub fn with_covering_bbox(mut self, enabled: bool) -> Self {
        self.covering_enabled = enabled;
        self
    }

    /// Adds a non-geometry (attribute) field to the output schema.
    ///
    /// Attribute values must subsequently be supplied for **every** row via
    /// [`add_row`](Self::add_row); once at least one attribute field is
    /// declared, [`add_geometry`](Self::add_geometry) is rejected because it
    /// cannot supply the attribute values.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema has already been frozen (any data
    /// written), if `field` collides with the geometry column, the covering
    /// bbox column, or another already-declared field.
    pub fn add_field(mut self, field: Field) -> Result<Self> {
        if self.output_schema.is_some() || matches!(self.inner, Some(WriterInner::Active(_))) {
            return Err(GeoParquetError::invalid_schema(
                "cannot add a field after the schema has been frozen (writes already started)",
            ));
        }
        if field.name() == &self.geometry_column {
            return Err(GeoParquetError::invalid_schema(
                "Field name conflicts with geometry column",
            ));
        }
        if self.covering_enabled && field.name() == COVERING_BBOX_COLUMN {
            return Err(GeoParquetError::invalid_schema(
                "Field name conflicts with the covering bbox column ('bbox')",
            ));
        }
        if self
            .additional_fields
            .iter()
            .any(|f| f.name() == field.name())
        {
            return Err(GeoParquetError::invalid_schema(format!(
                "duplicate attribute field name '{}'",
                field.name()
            )));
        }

        self.additional_fields.push(field);
        self.attribute_columns.push(Vec::new());
        Ok(self)
    }

    /// Adds a geometry to the current batch.
    ///
    /// # Errors
    ///
    /// Returns an error when attribute fields have been declared (via
    /// [`add_field`](Self::add_field)) — in that case every row must be added
    /// with [`add_row`](Self::add_row) so its attribute values are supplied.
    pub fn add_geometry(&mut self, geometry: &Geometry) -> Result<()> {
        if !self.additional_fields.is_empty() {
            return Err(GeoParquetError::invalid_schema(
                "this writer has attribute columns declared via add_field; use add_row(geometry, attributes) to supply a value for every column",
            ));
        }
        self.record_geometry(geometry);
        self.maybe_flush()
    }

    /// Adds multiple geometries
    pub fn add_geometries(&mut self, geometries: &[Geometry]) -> Result<()> {
        for geom in geometries {
            self.add_geometry(geom)?;
        }
        Ok(())
    }

    /// Adds a geometry together with its attribute values.
    ///
    /// `attributes` must contain exactly one entry per field declared with
    /// [`add_field`](Self::add_field), in the same order.  Each entry is a
    /// **single-element** Arrow array whose data type matches the declared
    /// field; the value is buffered and written into the corresponding
    /// attribute column when the batch is flushed.
    ///
    /// When no attribute fields are declared, pass an empty slice — this is
    /// then equivalent to [`add_geometry`](Self::add_geometry).
    ///
    /// # Errors
    ///
    /// Returns an error if the number of attribute arrays does not match the
    /// number of declared fields, if any array is not exactly one element long,
    /// or if an array's data type does not match its field.
    pub fn add_row(&mut self, geometry: &Geometry, attributes: &[ArrayRef]) -> Result<()> {
        if attributes.len() != self.additional_fields.len() {
            return Err(GeoParquetError::invalid_schema(format!(
                "add_row: expected {} attribute array(s) (one per declared field), got {}",
                self.additional_fields.len(),
                attributes.len()
            )));
        }

        for (j, attr) in attributes.iter().enumerate() {
            let field = &self.additional_fields[j];
            if attr.len() != 1 {
                return Err(GeoParquetError::invalid_schema(format!(
                    "add_row: attribute for column '{}' must be a single-element array (one value per row), got length {}",
                    field.name(),
                    attr.len()
                )));
            }
            if attr.data_type() != field.data_type() {
                return Err(GeoParquetError::type_mismatch(
                    format!("{:?} (column '{}')", field.data_type(), field.name()),
                    format!("{:?}", attr.data_type()),
                ));
            }
        }

        // Only mutate state once all attributes have validated, so a rejected
        // row leaves the buffers consistent.
        for (j, attr) in attributes.iter().enumerate() {
            self.attribute_columns[j].push(attr.clone());
        }
        self.record_geometry(geometry);
        self.maybe_flush()
    }

    /// Records a geometry into the current batch and updates statistics.
    fn record_geometry(&mut self, geometry: &Geometry) {
        self.stats
            .update(Some(geometry.type_name()), geometry.bbox().as_deref());
        self.current_batch.push(geometry.clone());
    }

    /// Flushes the current batch when it reaches the configured batch size.
    fn maybe_flush(&mut self) -> Result<()> {
        if self.current_batch.len() >= self.batch_size {
            self.flush_batch()?;
        }
        Ok(())
    }

    /// Builds (once) and caches the finalised output schema, including any
    /// attribute fields and the covering bbox struct column, plus the embedded
    /// `geo` metadata blob (carrying the `covering.bbox` object when enabled).
    fn ensure_schema(&mut self) -> Result<SchemaRef> {
        if let Some(schema) = &self.output_schema {
            return Ok(schema.clone());
        }

        let mut fields: Vec<Field> = Vec::with_capacity(
            1 + self.additional_fields.len() + usize::from(self.covering_enabled),
        );
        fields.push(self.geom_field.clone());
        fields.extend(self.additional_fields.iter().cloned());
        if self.covering_enabled {
            fields.push(covering_bbox_field());
        }
        let schema = Schema::new(fields);

        // Build the `geo` metadata, attaching the covering.bbox declaration
        // when covering columns are being emitted.
        let mut geo_metadata = GeoParquetMetadata::new(&self.geometry_column);
        let mut col_meta = self.column_metadata.clone();
        if self.covering_enabled {
            col_meta = col_meta.with_covering(Covering::bbox_struct(COVERING_BBOX_COLUMN));
        }
        geo_metadata.add_column(&self.geometry_column, col_meta);
        let metadata_json = geo_metadata.to_json()?;

        let schema = add_geoparquet_metadata(schema, metadata_json)?;
        let schema = Arc::new(schema);
        self.output_schema = Some(schema.clone());
        Ok(schema)
    }

    /// Transitions the sink from `Pending` to `Active`, constructing the
    /// `ArrowWriter` from the finalised schema.  Idempotent once active.
    fn activate(&mut self) -> Result<()> {
        if matches!(self.inner, Some(WriterInner::Active(_))) {
            return Ok(());
        }
        let schema = self.ensure_schema()?;
        match self.inner.take() {
            Some(WriterInner::Pending { file, compression }) => {
                let props = WriterProperties::builder()
                    .set_compression(compression.to_parquet())
                    .build();
                let writer = ArrowWriter::try_new(file, schema, Some(props))?;
                self.inner = Some(WriterInner::Active(Box::new(writer)));
                Ok(())
            }
            Some(other) => {
                self.inner = Some(other);
                Ok(())
            }
            None => Err(GeoParquetError::internal(
                "GeoParquet writer has already been finished",
            )),
        }
    }

    /// Encodes the buffered geometries into the geometry column array.
    fn build_geometry_array(&self) -> Result<ArrayRef> {
        match self.encoding {
            EncodingType::Wkb => {
                let mut geom_builder =
                    GeometryArrayBuilder::with_capacity(self.current_batch.len());
                for geom in &self.current_batch {
                    geom_builder.append_geometry(geom)?;
                }
                Ok(geom_builder.finish_arc())
            }
            _ => {
                // Native encoding: validate uniformity then encode.  The
                // encoder itself rejects mixed types; this guard produces a
                // clearer error tied to the writer's column metadata.
                validate_uniform_native_types(self.encoding, &self.current_batch)?;
                encode_native_array(&self.current_batch, self.encoding, self.coord_dim)
            }
        }
    }

    /// Builds the covering bbox struct column for the buffered geometries.
    ///
    /// Each row's four extents come from [`Geometry::bbox`]; geometries with no
    /// computable bbox (e.g. empty) contribute null extents.
    fn build_covering_array(&self) -> Result<ArrayRef> {
        let n = self.current_batch.len();
        let mut xmin = Float64Builder::with_capacity(n);
        let mut ymin = Float64Builder::with_capacity(n);
        let mut xmax = Float64Builder::with_capacity(n);
        let mut ymax = Float64Builder::with_capacity(n);

        for geom in &self.current_batch {
            match geom.bbox() {
                Some(bbox) if bbox.len() >= 4 => {
                    xmin.append_value(bbox[0]);
                    ymin.append_value(bbox[1]);
                    xmax.append_value(bbox[2]);
                    ymax.append_value(bbox[3]);
                }
                _ => {
                    xmin.append_null();
                    ymin.append_null();
                    xmax.append_null();
                    ymax.append_null();
                }
            }
        }

        let arrays: Vec<ArrayRef> = vec![
            Arc::new(xmin.finish()),
            Arc::new(ymin.finish()),
            Arc::new(xmax.finish()),
            Arc::new(ymax.finish()),
        ];
        let struct_array = StructArray::try_new(covering_bbox_fields(), arrays, None)?;
        Ok(Arc::new(struct_array))
    }

    /// Assembles the full column set (geometry, attribute columns, optional
    /// covering bbox struct) for the buffered batch.
    fn build_columns(&self) -> Result<Vec<ArrayRef>> {
        let n = self.current_batch.len();
        let mut columns: Vec<ArrayRef> = Vec::with_capacity(
            1 + self.additional_fields.len() + usize::from(self.covering_enabled),
        );

        columns.push(self.build_geometry_array()?);

        for (j, field) in self.additional_fields.iter().enumerate() {
            let parts = &self.attribute_columns[j];
            if parts.len() != n {
                return Err(GeoParquetError::internal(format!(
                    "attribute column '{}' has {} buffered values but the batch has {} rows",
                    field.name(),
                    parts.len(),
                    n
                )));
            }
            let refs: Vec<&dyn Array> = parts.iter().map(|a| a.as_ref()).collect();
            let column = arrow::compute::concat(&refs)?;
            columns.push(column);
        }

        if self.covering_enabled {
            columns.push(self.build_covering_array()?);
        }

        Ok(columns)
    }

    /// Flushes the current batch to a row group.
    ///
    /// Builds the geometry column (WKB or native GeoArrow), any attribute
    /// columns (concatenating the per-row buffered arrays), and — when enabled
    /// — the covering bbox struct column, then writes them as one
    /// [`RecordBatch`].  Lazily activates the underlying [`ArrowWriter`] on the
    /// first flush.
    fn flush_batch(&mut self) -> Result<()> {
        if self.current_batch.is_empty() {
            return Ok(());
        }

        let schema = self.ensure_schema()?;
        let columns = self.build_columns()?;
        let batch = RecordBatch::try_new(schema, columns)?;

        self.activate()?;
        match self.inner.as_mut() {
            Some(WriterInner::Active(writer)) => {
                writer.write(&batch)?;
                // Finalize the current row group so each flushed batch maps to
                // exactly one row group.  This honours the documented
                // "batch_size rows per row group" contract and makes the
                // covering.bbox row-group pruning fast path effective on files
                // this writer produces.
                writer.flush()?;
            }
            _ => {
                return Err(GeoParquetError::internal(
                    "writer sink is not active after activation",
                ));
            }
        }

        self.current_batch.clear();
        for column in &mut self.attribute_columns {
            column.clear();
        }

        Ok(())
    }

    /// Finalizes the file and writes footer.
    ///
    /// Any buffered rows are flushed first.  An empty writer still produces a
    /// valid, schema-carrying (but row-less) Parquet file.
    pub fn finish(mut self) -> Result<()> {
        // Flush remaining geometries.
        self.flush_batch()?;

        // Ensure the file has a writer even when zero rows were written, so the
        // output is a valid Parquet file carrying the declared schema.
        self.activate()?;

        match self.inner.take() {
            Some(WriterInner::Active(writer)) => {
                writer.close()?;
                Ok(())
            }
            _ => Err(GeoParquetError::internal(
                "GeoParquet writer has already been finished",
            )),
        }
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
    covering: bool,
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
            covering: false,
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

    /// Adds an additional (attribute) field.
    ///
    /// Values for the field must be supplied for every row via
    /// [`GeoParquetWriter::add_row`].
    pub fn add_field(mut self, field: Field) -> Self {
        self.additional_fields.push(field);
        self
    }

    /// Enables emission of GeoParquet 1.1 `covering.bbox` columns (see
    /// [`GeoParquetWriter::with_covering_bbox`]).
    pub fn with_covering_bbox(mut self, enabled: bool) -> Self {
        self.covering = enabled;
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
        // Enable covering before declaring attribute fields so the field-name
        // conflict check against the covering column is active.
        writer = writer.with_covering_bbox(self.covering);

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
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use crate::geometry::Point;
    use crate::metadata::Crs;
    use crate::reader::GeoParquetReader;
    use arrow_array::{Float64Array, Int64Array, StringArray};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "oxigeo_geoparquet_writer_{}_{}.parquet",
            name,
            std::process::id()
        ));
        p
    }

    #[test]
    fn test_writer_creation() {
        let path = temp_path("creation");
        let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
        let result = GeoParquetWriter::new(&path, "geometry", metadata);

        assert!(result.is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_writer_builder() {
        let metadata = GeometryColumnMetadata::new_wkb();
        let builder = GeoParquetWriterBuilder::new("geom", metadata)
            .batch_size(500)
            .compression(CompressionType::Gzip);

        let path = temp_path("builder");
        let result = builder.build(&path);
        assert!(result.is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_empty_writer_produces_valid_file() {
        let path = temp_path("empty");
        let metadata = GeometryColumnMetadata::new_wkb();
        let writer = GeoParquetWriter::new(&path, "geometry", metadata).expect("writer");
        writer.finish().expect("finish empty");

        let reader = GeoParquetReader::open(&path).expect("open");
        assert_eq!(reader.num_rows(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_attribute_columns_roundtrip() {
        let path = temp_path("attrs");
        let metadata = GeometryColumnMetadata::new_wkb();
        let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
            .add_field(Field::new("id", DataType::Int64, false))
            .add_field(Field::new("name", DataType::Utf8, true))
            .build(&path)
            .expect("build");

        for i in 0..3i64 {
            let geom = Geometry::Point(Point::new_2d(i as f64, i as f64 + 0.5));
            let id: ArrayRef = Arc::new(Int64Array::from(vec![i]));
            let name: ArrayRef = Arc::new(StringArray::from(vec![format!("feature-{i}")]));
            writer.add_row(&geom, &[id, name]).expect("add_row");
        }
        writer.finish().expect("finish");

        let reader = GeoParquetReader::open(&path).expect("open");
        assert_eq!(reader.num_rows(), 3);
        let batch = reader.read_row_group(0).expect("row group");
        assert_eq!(batch.num_columns(), 3, "geometry + id + name");

        let id_col = batch
            .column_by_name("id")
            .expect("id column")
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("int64");
        assert_eq!(id_col.values(), &[0, 1, 2]);

        let name_col = batch
            .column_by_name("name")
            .expect("name column")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("utf8");
        assert_eq!(name_col.value(0), "feature-0");
        assert_eq!(name_col.value(2), "feature-2");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_add_geometry_rejected_when_attributes_declared() {
        let path = temp_path("reject_geom");
        let metadata = GeometryColumnMetadata::new_wkb();
        let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
            .add_field(Field::new("id", DataType::Int64, false))
            .build(&path)
            .expect("build");

        let err = writer.add_geometry(&Geometry::Point(Point::new_2d(0.0, 0.0)));
        assert!(
            err.is_err(),
            "add_geometry must be rejected with attribute columns"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_add_row_wrong_attribute_count_errors() {
        let path = temp_path("wrong_count");
        let metadata = GeometryColumnMetadata::new_wkb();
        let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
            .add_field(Field::new("id", DataType::Int64, false))
            .build(&path)
            .expect("build");

        let geom = Geometry::Point(Point::new_2d(0.0, 0.0));
        // No attributes supplied though one field declared.
        assert!(writer.add_row(&geom, &[]).is_err());
        // Wrong type.
        let bad: ArrayRef = Arc::new(Float64Array::from(vec![1.0]));
        assert!(writer.add_row(&geom, &[bad]).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_covering_bbox_columns_written() {
        let path = temp_path("covering");
        let metadata = GeometryColumnMetadata::new_wkb();
        let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
            .with_covering_bbox(true)
            .build(&path)
            .expect("build");

        writer
            .add_geometry(&Geometry::Point(Point::new_2d(1.0, 2.0)))
            .expect("add");
        writer
            .add_geometry(&Geometry::Point(Point::new_2d(5.0, 6.0)))
            .expect("add");
        writer.finish().expect("finish");

        let reader = GeoParquetReader::open(&path).expect("open");
        // The geo metadata must declare covering.bbox.
        let col_meta = reader
            .metadata()
            .primary_column_metadata()
            .expect("col meta");
        assert!(
            col_meta.covering.is_some(),
            "covering.bbox must be declared in geo metadata"
        );

        // The struct bbox column must be present and populated.
        let batch = reader.read_row_group(0).expect("row group");
        let bbox = batch
            .column_by_name("bbox")
            .expect("bbox column")
            .as_any()
            .downcast_ref::<StructArray>()
            .expect("struct");
        let xmin = bbox
            .column_by_name("xmin")
            .expect("xmin")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("f64");
        assert_eq!(xmin.value(0), 1.0);
        assert_eq!(xmin.value(1), 5.0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_covering_bbox_enables_row_group_pruning() {
        let path = temp_path("covering_prune");
        let metadata = GeometryColumnMetadata::new_wkb();
        // batch_size = 1 forces one row group per geometry, so row-group
        // pruning via the covering columns is observable.
        let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
            .with_covering_bbox(true)
            .batch_size(1)
            .build(&path)
            .expect("build");
        writer
            .add_geometry(&Geometry::Point(Point::new_2d(10.0, 10.0)))
            .expect("add");
        writer
            .add_geometry(&Geometry::Point(Point::new_2d(500.0, 500.0)))
            .expect("add");
        writer.finish().expect("finish");

        let reader = GeoParquetReader::open(&path).expect("open");
        assert_eq!(
            reader.num_row_groups(),
            2,
            "batch_size=1 gives 2 row groups"
        );

        // A bbox filter around the first point must, via covering-column
        // row-group pruning + the bbox predicate, return exactly that row.
        let batches = reader
            .with_bbox_filter((8.0, 8.0, 12.0, 12.0))
            .read_pushdown()
            .expect("pushdown");
        let total: usize = batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total, 1, "only the first point intersects the query bbox");

        let _ = std::fs::remove_file(&path);
    }
}
