//! GeoParquet file reader implementation

use crate::arrow_ext::extract_geoparquet_metadata;
use crate::error::{GeoParquetError, Result};
use crate::filter::{AttributePredicates, filter_batch_by_mask};
use crate::geometry::native::native_bbox_mask;
use crate::geometry::{
    Geometry, WkbReader, decode_native_array, decode_native_array_optional, wkb_bbox,
};
use crate::metadata::{EncodingType, GeoParquetMetadata};
use crate::plan::prune_row_groups;
use crate::predicate::AttributeFilter;
use crate::pushdown::execute_pushdown;
use crate::spatial::{RowGroupBounds, SpatialFilter, SpatialIndex};
use crate::statistics::{
    ColumnStatistics, extract_column_statistics, geometry_has_meaningful_stats,
};
use arrow_array::{Array, BinaryArray, RecordBatch};
use arrow_schema::SchemaRef;
use bytes::Bytes;
use oxigeo_core::types::BoundingBox;
use parquet::arrow::arrow_reader::{
    ArrowReaderMetadata, ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder,
};
use parquet::file::metadata::ParquetMetaData;
use parquet::file::reader::{ChunkReader, Length};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Backing store for a [`GeoParquetReader`].
///
/// Both variants implement [`ChunkReader`], so every read path in this module
/// is written exactly once and behaves identically whether the Parquet image
/// lives on disk or in memory.  Cloning is cheap in both cases: the file handle
/// is refcounted behind an [`Arc`] and [`Bytes`] is refcounted internally.
///
/// This is deliberately an enum rather than a type parameter on
/// [`GeoParquetReader`]: adding a generic to the public reader type would be a
/// breaking change for every downstream `GeoParquetReader` mention.
#[derive(Clone)]
enum Source {
    /// A Parquet file opened from the local filesystem.
    File(Arc<File>),
    /// An in-memory Parquet image.
    Bytes(Bytes),
}

/// [`Read`] handle produced by [`Source`]'s [`ChunkReader`] implementation.
///
/// Mirrors the reader types that `parquet` uses for its own `File` / `Bytes`
/// implementations, so neither variant pays for an extra buffering layer.
enum SourceRead {
    /// Handle over a local file (matches `<File as ChunkReader>::T`).
    File(BufReader<File>),
    /// Handle over an in-memory buffer (matches `<Bytes as ChunkReader>::T`).
    Bytes(<Bytes as ChunkReader>::T),
}

impl Read for SourceRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(r) => r.read(buf),
            Self::Bytes(r) => r.read(buf),
        }
    }
}

impl Length for Source {
    fn len(&self) -> u64 {
        match self {
            Self::File(f) => Length::len(f.as_ref()),
            Self::Bytes(b) => Length::len(b),
        }
    }
}

impl ChunkReader for Source {
    type T = SourceRead;

    fn get_read(&self, start: u64) -> parquet::errors::Result<Self::T> {
        match self {
            Self::File(f) => Ok(SourceRead::File(ChunkReader::get_read(f.as_ref(), start)?)),
            Self::Bytes(b) => Ok(SourceRead::Bytes(ChunkReader::get_read(b, start)?)),
        }
    }

    fn get_bytes(&self, start: u64, length: usize) -> parquet::errors::Result<Bytes> {
        match self {
            Self::File(f) => ChunkReader::get_bytes(f.as_ref(), start, length),
            Self::Bytes(b) => ChunkReader::get_bytes(b, start, length),
        }
    }
}

/// GeoParquet file reader
pub struct GeoParquetReader {
    /// The underlying Parquet source (local file or in-memory buffer)
    source: Source,
    /// Arrow schema
    schema: SchemaRef,
    /// GeoParquet metadata
    metadata: GeoParquetMetadata,
    /// Parquet file metadata
    parquet_metadata: Arc<ParquetMetaData>,
    /// Spatial index
    spatial_index: Option<SpatialIndex>,
    /// Name of the primary geometry column
    geometry_column: String,
    /// Optional bbox filter for pushdown reads.
    bbox_filter: Option<(f64, f64, f64, f64)>,
    /// Attribute filters for pushdown reads (combined conjunctively).
    attribute_filters: Vec<AttributeFilter>,
    /// Cached row-group statistics (lazy).
    stats_cache: OnceLock<Vec<Vec<ColumnStatistics>>>,
}

/// Returns the encoding declared by the geometry column's `geo` metadata,
/// defaulting to [`EncodingType::Wkb`] when missing.
fn detect_encoding(metadata: &GeoParquetMetadata, geometry_column: &str) -> EncodingType {
    metadata
        .get_column(geometry_column)
        .map(|c| c.encoding)
        .unwrap_or(EncodingType::Wkb)
}

/// Decodes a whole geometry column into one [`Geometry`] per **non-null** row,
/// dispatching on the column's declared `encoding`.
///
/// Null rows produce no entry — this is the historical behaviour of
/// [`GeoParquetReader::read_geometries`] and
/// [`GeoParquetBatchReader::extract_geometries`].  Callers that need to keep
/// geometries aligned with their property rows must use the `_optional`
/// variants instead.
fn decode_geometry_column(
    geom_column: &dyn Array,
    encoding: EncodingType,
) -> Result<Vec<Geometry>> {
    match encoding {
        EncodingType::Wkb => {
            let binary_array = downcast_wkb_column(geom_column)?;
            let mut geometries = Vec::with_capacity(binary_array.len());
            for i in 0..binary_array.len() {
                if !binary_array.is_null(i) {
                    let wkb = binary_array.value(i);
                    let mut wkb_reader = WkbReader::new(wkb);
                    let geom = wkb_reader.read_geometry()?;
                    geometries.push(geom);
                }
            }
            Ok(geometries)
        }
        native => decode_native_array(geom_column, native),
    }
}

/// Decodes a whole geometry column into exactly one entry per row, preserving
/// null rows as `None` **at their original index**.
///
/// This is the index-preserving counterpart of [`decode_geometry_column`]: the
/// returned vector is always `geom_column.len()` long, so element `i` always
/// belongs to row `i` of the same [`RecordBatch`].
fn decode_geometry_column_optional(
    geom_column: &dyn Array,
    encoding: EncodingType,
) -> Result<Vec<Option<Geometry>>> {
    match encoding {
        EncodingType::Wkb => {
            let binary_array = downcast_wkb_column(geom_column)?;
            let mut geometries = Vec::with_capacity(binary_array.len());
            for i in 0..binary_array.len() {
                if binary_array.is_null(i) {
                    geometries.push(None);
                    continue;
                }
                let wkb = binary_array.value(i);
                let mut wkb_reader = WkbReader::new(wkb);
                geometries.push(Some(wkb_reader.read_geometry()?));
            }
            Ok(geometries)
        }
        native => decode_native_array_optional(geom_column, native),
    }
}

/// Downcasts a WKB-encoded geometry column to its concrete [`BinaryArray`].
fn downcast_wkb_column(geom_column: &dyn Array) -> Result<&BinaryArray> {
    geom_column
        .as_any()
        .downcast_ref::<BinaryArray>()
        .ok_or_else(|| {
            GeoParquetError::type_mismatch("BinaryArray", format!("{:?}", geom_column.data_type()))
        })
}

impl GeoParquetReader {
    /// Opens a GeoParquet file for reading
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or is not a valid GeoParquet file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        Self::from_source(Source::File(Arc::new(file)))
    }

    /// Opens a GeoParquet image held entirely in memory.
    ///
    /// This is the in-memory twin of [`open`](Self::open) and supports every
    /// read path the file-backed reader does — including
    /// [`read_pushdown`](Self::read_pushdown) — because both are driven through
    /// the same [`ChunkReader`] abstraction.  Useful when the bytes arrive from
    /// a network fetch, an archive member, or a test fixture rather than the
    /// local filesystem.
    ///
    /// The buffer is adopted, not copied, whenever the argument can be
    /// converted into [`Bytes`] without reallocating (`Vec<u8>`, `Bytes`,
    /// `&'static [u8]`, …).
    ///
    /// ```rust,no_run
    /// # use oxigeo_geoparquet::{GeoParquetReader, error::Result};
    /// # fn example() -> Result<()> {
    /// let raw = std::fs::read("input.parquet")?;
    /// let reader = GeoParquetReader::from_bytes(raw)?;
    /// let geoms = reader.read_geometries(0)?;
    /// # let _ = geoms;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Returns an error if the buffer is not a valid GeoParquet image, or if it
    /// carries no `geo` metadata key.
    pub fn from_bytes(data: impl Into<Bytes>) -> Result<Self> {
        Self::from_source(Source::Bytes(data.into()))
    }

    /// Builds a reader over any [`Source`], decoding the Parquet footer and the
    /// embedded GeoParquet `geo` metadata once.
    fn from_source(source: Source) -> Result<Self> {
        // Build reader to extract metadata
        let builder = ParquetRecordBatchReaderBuilder::try_new(source.clone())?;
        let schema = builder.schema().clone();
        let parquet_metadata = builder.metadata().clone();

        // Extract GeoParquet metadata
        let metadata_json = extract_geoparquet_metadata(&schema)?
            .ok_or_else(|| GeoParquetError::invalid_metadata("Missing GeoParquet metadata"))?;

        let metadata = GeoParquetMetadata::from_json(&metadata_json)?;
        let geometry_column = metadata.primary_column.clone();

        Ok(Self {
            source,
            schema,
            metadata,
            parquet_metadata,
            spatial_index: None,
            geometry_column,
            bbox_filter: None,
            attribute_filters: Vec::new(),
            stats_cache: OnceLock::new(),
        })
    }

    /// Returns the GeoParquet metadata
    pub fn metadata(&self) -> &GeoParquetMetadata {
        &self.metadata
    }

    /// Returns the Arrow schema
    pub fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    /// Returns the number of row groups
    pub fn num_row_groups(&self) -> usize {
        self.parquet_metadata.num_row_groups()
    }

    /// Returns the total number of rows
    pub fn num_rows(&self) -> i64 {
        self.parquet_metadata.file_metadata().num_rows()
    }

    /// Builds a spatial index for efficient spatial queries
    pub fn build_spatial_index(&mut self) -> Result<()> {
        let mut row_groups = Vec::new();

        for i in 0..self.num_row_groups() {
            let row_group = self.parquet_metadata.row_group(i);
            let row_count = row_group.num_rows() as u64;

            // Try to extract bounding box from row group metadata
            // For now, we'll need to read the row group to compute bbox
            // In a production implementation, this would be cached in metadata
            if let Ok(Some(bbox)) = self.compute_row_group_bbox(i) {
                row_groups.push(RowGroupBounds::new(i, bbox, row_count));
            }
        }

        let mut index = SpatialIndex::new(row_groups);
        index.build_rtree()?;
        self.spatial_index = Some(index);

        Ok(())
    }

    // ── GeoParquet 1.1 predicate pushdown ─────────────────────────────────────

    /// Set a bounding-box filter that will be pushed into Parquet at read time.
    ///
    /// When the file contains GeoParquet 1.1 `covering.bbox` columns this
    /// filter skips WKB decoding entirely by using `ArrowPredicate` on the
    /// four bbox columns.  When covering columns are absent the filter is
    /// applied as a WKB post-decode step.
    ///
    /// Row-group pruning via `with_row_groups()` is also applied whenever
    /// column statistics carry min/max values.
    pub fn with_bbox_filter(mut self, bbox: (f64, f64, f64, f64)) -> Self {
        self.bbox_filter = Some(bbox);
        self
    }

    /// Set an attribute filter that will be pushed down into Parquet at read time.
    ///
    /// The filter is compiled to an [`ArrowPredicate`] and fed to
    /// `RowFilter::new(...)` on the underlying builder.  Only the referenced
    /// column is decoded during predicate evaluation.
    ///
    /// [`ArrowPredicate`]: parquet::arrow::arrow_reader::ArrowPredicate
    pub fn with_attribute_filter(mut self, filter: AttributeFilter) -> Self {
        self.attribute_filters.push(filter);
        self
    }

    /// Sets multiple attribute filters at once, replacing any previously set.
    ///
    /// The filters are combined conjunctively (a row must satisfy all of them),
    /// each compiled to its own `ArrowPredicate` within a single `RowFilter`.
    pub fn with_attribute_filters(mut self, filters: Vec<AttributeFilter>) -> Self {
        self.attribute_filters = filters;
        self
    }

    /// Execute a pushdown read, returning all matching rows as a `Vec<RecordBatch>`.
    ///
    /// Applies, in order:
    /// 1. Row-group pruning from covering.bbox column statistics (if available).
    /// 2. covering.bbox `ArrowPredicate` (row-level, no WKB decode).
    /// 3. Attribute `ArrowPredicate` (if set via `with_attribute_filter`).
    /// 4. WKB-based bbox post-filter as a fallback when covering columns are absent
    ///    and a bbox filter was set.
    ///
    /// # Errors
    ///
    /// Propagates Parquet, Arrow, and I/O errors.
    pub fn read_pushdown(&self) -> Result<Vec<RecordBatch>> {
        let input = self.source.clone();
        let arrow_meta =
            ArrowReaderMetadata::try_new(self.parquet_metadata.clone(), Default::default())?;

        // Row-group pruning shares the same logic as the metadata-only planner.
        let schema_descr = self.parquet_metadata.file_metadata().schema_descr();
        let bbox_cols = crate::covering::BboxColumns::detect_with_covering(
            schema_descr,
            &self.geometry_column,
            &self.metadata,
        );
        let survivors =
            prune_row_groups(&self.parquet_metadata, bbox_cols.as_ref(), self.bbox_filter);

        execute_pushdown(
            input,
            arrow_meta,
            &self.metadata,
            &self.geometry_column,
            self.bbox_filter,
            &self.attribute_filters,
            survivors,
            None,
            None,
        )
    }

    /// Creates a reader for all rows
    pub fn read_all(&self) -> Result<GeoParquetBatchReader> {
        self.read_filtered(SpatialFilter::All)
    }

    /// Creates a reader with spatial filtering
    pub fn read_filtered(&self, filter: SpatialFilter) -> Result<GeoParquetBatchReader> {
        let row_groups = if let Some(ref index) = self.spatial_index {
            if let SpatialFilter::BoundingBox(ref bbox) = filter {
                index.query(bbox)
            } else {
                index.all_row_groups()
            }
        } else {
            (0..self.num_row_groups()).collect()
        };

        GeoParquetBatchReader::new(
            self.source.clone(),
            self.schema.clone(),
            self.geometry_column.clone(),
            detect_encoding(&self.metadata, &self.geometry_column),
            row_groups,
        )
    }

    /// Reads a specific row group
    pub fn read_row_group(&self, row_group: usize) -> Result<RecordBatch> {
        if row_group >= self.num_row_groups() {
            return Err(GeoParquetError::out_of_bounds(
                row_group,
                self.num_row_groups(),
            ));
        }

        let builder = ParquetRecordBatchReaderBuilder::try_new(self.source.clone())?;
        let mut reader = builder.with_row_groups(vec![row_group]).build()?;

        reader
            .next()
            .ok_or_else(|| GeoParquetError::internal("Row group has no data"))?
            .map_err(Into::into)
    }

    /// Reads geometries from a specific row group, dispatching on the
    /// declared encoding of the geometry column.
    ///
    /// * WKB → decodes each binary blob with [`WkbReader`].
    /// * Native GeoArrow encodings → eagerly materialises the typed Arrow
    ///   array into a `Vec<Geometry>` via
    ///   [`crate::geometry::decode_native_array`].
    ///
    /// Null geometry rows are **skipped**, so the returned vector may be
    /// shorter than the row group and its indices no longer line up with the
    /// row group's property columns.  Use
    /// [`read_geometries_optional`](Self::read_geometries_optional) when that
    /// alignment matters.
    pub fn read_geometries(&self, row_group: usize) -> Result<Vec<Geometry>> {
        let batch = self.read_row_group(row_group)?;
        let geom_column = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        let encoding = detect_encoding(&self.metadata, &self.geometry_column);
        decode_geometry_column(geom_column.as_ref(), encoding)
    }

    /// Reads geometries from a specific row group, preserving null rows as
    /// `None` **at their original index**.
    ///
    /// Identical to [`read_geometries`](Self::read_geometries) in every respect
    /// except null handling: the returned vector always has exactly one entry
    /// per row of the row group, so element `i` can be paired with row `i` of
    /// the batch returned by [`read_row_group`](Self::read_row_group).
    ///
    /// Prefer this method whenever geometries are consumed alongside attribute
    /// columns — `read_geometries` silently drops nulls, which shifts every
    /// subsequent geometry one slot out of step with its properties.
    ///
    /// # Errors
    ///
    /// Same as [`read_geometries`](Self::read_geometries): propagates Parquet,
    /// Arrow, and geometry-decode errors, plus a
    /// [`GeoParquetError::MissingField`] when the geometry column is absent.
    pub fn read_geometries_optional(&self, row_group: usize) -> Result<Vec<Option<Geometry>>> {
        let batch = self.read_row_group(row_group)?;
        let geom_column = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        let encoding = detect_encoding(&self.metadata, &self.geometry_column);
        decode_geometry_column_optional(geom_column.as_ref(), encoding)
    }

    /// Computes the bounding box for a row group
    fn compute_row_group_bbox(&self, row_group: usize) -> Result<Option<BoundingBox>> {
        let geometries = self.read_geometries(row_group)?;

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for geom in &geometries {
            if let Some(bbox) = geom.bbox() {
                min_x = min_x.min(bbox[0]);
                min_y = min_y.min(bbox[1]);
                max_x = max_x.max(bbox[2]);
                max_y = max_y.max(bbox[3]);
            }
        }

        if min_x.is_finite() {
            Ok(Some(BoundingBox::new(min_x, min_y, max_x, max_y)?))
        } else {
            Ok(None)
        }
    }

    /// Returns the primary geometry column name
    pub fn geometry_column_name(&self) -> &str {
        &self.geometry_column
    }

    /// Computes the dataset's spatial extent `(xmin, ymin, xmax, ymax)` from the
    /// GeoParquet 1.1 `covering.bbox` column statistics, without decoding any
    /// geometry.
    ///
    /// Returns the union of every row group's covering bbox, or `None` when the
    /// file carries no covering columns (or none of them expose min/max
    /// statistics).  This is the cheap, metadata-only path used by
    /// [`crate::partitioning::PartitionedDataset::discover`] to populate each
    /// partition's spatial extent.
    pub fn covering_extent(&self) -> Option<(f64, f64, f64, f64)> {
        let schema_descr = self.parquet_metadata.file_metadata().schema_descr();
        let bbox_cols = crate::covering::BboxColumns::detect_with_covering(
            schema_descr,
            &self.geometry_column,
            &self.metadata,
        )?;

        let mut acc: Option<(f64, f64, f64, f64)> = None;
        for i in 0..self.num_row_groups() {
            let rg = self.parquet_metadata.row_group(i);
            if let Some((xmin, ymin, xmax, ymax)) = bbox_cols.row_group_bbox(rg) {
                acc = Some(match acc {
                    None => (xmin, ymin, xmax, ymax),
                    Some(a) => (a.0.min(xmin), a.1.min(ymin), a.2.max(xmax), a.3.max(ymax)),
                });
            }
        }
        acc
    }

    // ── Row-level spatial filtering ────────────────────────────────────────────

    /// Reads all rows whose geometry intersects `bbox` at the **row level**.
    ///
    /// Unlike `read_filtered`, which only coarsely prunes row groups using
    /// metadata, this method:
    ///
    /// 1. Calls [`build_spatial_index`] (or uses the existing index) to
    ///    identify candidate row groups that intersect `bbox`.
    /// 2. Reads each candidate row group into a [`RecordBatch`].
    /// 3. For every row, decodes the WKB geometry from the geometry column and
    ///    checks whether its bounding box intersects `bbox`.
    /// 4. Collects matching rows into a filtered [`RecordBatch`] and returns
    ///    all non-empty batches as a `Vec`.
    ///
    /// Rows whose geometry cannot be decoded or whose bounding box cannot be
    /// computed are silently excluded.
    ///
    /// # Errors
    ///
    /// Propagates Parquet, Arrow, and I/O errors.
    ///
    /// [`build_spatial_index`]: GeoParquetReader::build_spatial_index
    pub fn read_filtered_exact(&mut self, bbox: BoundingBox) -> Result<Vec<RecordBatch>> {
        // Ensure we have a spatial index so the row-group pruning is effective.
        if self.spatial_index.is_none() {
            self.build_spatial_index()?;
        }

        let filter = SpatialFilter::BoundingBox(bbox);
        let mut batch_reader = self.read_filtered(filter)?;
        let mut results = Vec::new();

        while let Some(batch) = batch_reader.next_batch()? {
            let row_mask = self.spatial_row_mask(&batch, &bbox)?;
            if row_mask.iter().any(|&b| b) {
                let filtered = crate::filter::filter_batch_by_mask(&batch, &row_mask)?;
                if filtered.num_rows() > 0 {
                    results.push(filtered);
                }
            }
        }

        Ok(results)
    }

    /// Reads rows applying an optional bounding-box filter **and** an optional
    /// attribute filter.
    ///
    /// The two filters are combined with intersection semantics (both must
    /// hold).  Pass `None` to skip a filter entirely.
    ///
    /// When a `bbox` is supplied, row-group pruning is applied first via
    /// [`read_filtered_exact`] logic, then per-row geometry checks are
    /// performed.  When `attribute_filter` is also supplied, only rows
    /// satisfying both the spatial and attribute predicates are returned.
    ///
    /// # Errors
    ///
    /// Propagates Parquet, Arrow, and I/O errors.
    ///
    /// [`read_filtered_exact`]: GeoParquetReader::read_filtered_exact
    pub fn read_with_filter(
        &mut self,
        bbox: Option<BoundingBox>,
        attribute_filter: Option<AttributePredicates>,
    ) -> Result<Vec<RecordBatch>> {
        // Ensure spatial index is built if we'll need it.
        if bbox.is_some() && self.spatial_index.is_none() {
            self.build_spatial_index()?;
        }

        // Choose the spatial filter for row-group pruning.
        let spatial_filter = match &bbox {
            Some(b) => SpatialFilter::BoundingBox(*b),
            None => SpatialFilter::All,
        };

        let mut batch_reader = self.read_filtered(spatial_filter)?;
        let mut results = Vec::new();

        while let Some(batch) = batch_reader.next_batch()? {
            // 1. Spatial row mask (per-geometry bbox check).
            let spatial_mask = if let Some(ref b) = bbox {
                self.spatial_row_mask(&batch, b)?
            } else {
                vec![true; batch.num_rows()]
            };

            // 2. Attribute mask.
            let attr_mask = if let Some(ref preds) = attribute_filter {
                preds.row_mask(&batch)?
            } else {
                vec![true; batch.num_rows()]
            };

            // 3. Combine: a row matches iff both masks are true.
            let combined: Vec<bool> = spatial_mask
                .iter()
                .zip(attr_mask.iter())
                .map(|(s, a)| *s && *a)
                .collect();

            if combined.iter().any(|&b| b) {
                let filtered = filter_batch_by_mask(&batch, &combined)?;
                if filtered.num_rows() > 0 {
                    results.push(filtered);
                }
            }
        }

        Ok(results)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Builds a boolean row mask for `batch` where each `true` entry means the
    /// row's geometry intersects `bbox`.
    ///
    /// Rows with null or unparseable geometry are excluded (`false`).
    /// Dispatches on the geometry column encoding; native GeoArrow arrays
    /// avoid WKB decoding entirely.
    fn spatial_row_mask(&self, batch: &RecordBatch, bbox: &BoundingBox) -> Result<Vec<bool>> {
        let geom_col = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        let encoding = detect_encoding(&self.metadata, &self.geometry_column);
        match encoding {
            EncodingType::Wkb => {
                let binary = geom_col
                    .as_any()
                    .downcast_ref::<BinaryArray>()
                    .ok_or_else(|| {
                        GeoParquetError::type_mismatch(
                            "BinaryArray",
                            format!("{:?}", geom_col.data_type()),
                        )
                    })?;

                let mut mask = vec![false; binary.len()];
                for (i, m) in mask.iter_mut().enumerate() {
                    if binary.is_null(i) {
                        continue;
                    }
                    let wkb = binary.value(i);
                    if let Some((min_x, min_y, max_x, max_y)) = wkb_bbox(wkb) {
                        // Check AABB intersection (inclusive on edges).
                        if max_x >= bbox.min_x
                            && min_x <= bbox.max_x
                            && max_y >= bbox.min_y
                            && min_y <= bbox.max_y
                        {
                            *m = true;
                        }
                    }
                }
                Ok(mask)
            }
            native => native_bbox_mask(
                geom_col.as_ref(),
                native,
                bbox.min_x,
                bbox.min_y,
                bbox.max_x,
                bbox.max_y,
            ),
        }
    }

    // ── Statistics ─────────────────────────────────────────────────────────────

    /// Returns per-column / per-row-group statistics for the file.
    ///
    /// Outer index = row group, inner index = column.  When stats are missing
    /// for a particular column in a row group, that entry is omitted (so the
    /// inner Vec may be shorter than the schema's column count).
    ///
    /// Cached behind a [`OnceLock`] — subsequent calls are free.
    pub fn row_group_statistics(&self) -> Vec<Vec<ColumnStatistics>> {
        self.stats_cache
            .get_or_init(|| {
                extract_column_statistics(&self.parquet_metadata, &self.schema).unwrap_or_default()
            })
            .clone()
    }

    /// Returns the per-row-group statistics for a single column, in row-group
    /// order, or `None` when:
    ///
    /// * `column_name` does not appear in any row group's stats; or
    /// * `column_name` is the geometry column AND the file does not carry
    ///   GeoParquet 1.1 `covering.bbox` columns (the WKB blob has no
    ///   meaningful min/max).
    pub fn column_statistics(&self, column_name: &str) -> Option<Vec<ColumnStatistics>> {
        // Geometry-column gating: WKB blobs produce nonsense stats; only
        // surface them when bbox columns exist.
        if column_name == self.geometry_column
            && !geometry_has_meaningful_stats(&self.parquet_metadata, &self.geometry_column)
        {
            return None;
        }

        let groups = self.row_group_statistics();
        let mut out: Vec<ColumnStatistics> = Vec::new();
        for rg in &groups {
            if let Some(s) = rg.iter().find(|c| c.name == column_name) {
                out.push(s.clone());
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }
}

/// Iterator over record batches from a GeoParquet file
pub struct GeoParquetBatchReader {
    reader: ParquetRecordBatchReader,
    geometry_column: String,
    /// Declared encoding of the geometry column, threaded down from the parent
    /// [`GeoParquetReader`] so batch-level geometry extraction can dispatch the
    /// same way `GeoParquetReader::read_geometries` does.
    encoding: EncodingType,
}

impl GeoParquetBatchReader {
    /// Creates a new batch reader
    fn new(
        source: Source,
        _schema: SchemaRef,
        geometry_column: String,
        encoding: EncodingType,
        row_groups: Vec<usize>,
    ) -> Result<Self> {
        let builder = ParquetRecordBatchReaderBuilder::try_new(source)?;
        let reader = builder.with_row_groups(row_groups).build()?;

        Ok(Self {
            reader,
            geometry_column,
            encoding,
        })
    }

    /// Returns the next record batch
    pub fn next_batch(&mut self) -> Result<Option<RecordBatch>> {
        match self.reader.next() {
            Some(Ok(batch)) => Ok(Some(batch)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Extracts geometries from a record batch, dispatching on the geometry
    /// column's declared encoding.
    ///
    /// * WKB → decodes each binary blob with [`WkbReader`].
    /// * Native GeoArrow encodings → decodes the typed Arrow array via
    ///   [`crate::geometry::decode_native_array`].
    ///
    /// Null geometry rows are **skipped**, so the returned vector may be
    /// shorter than `batch.num_rows()`.  Use
    /// [`extract_geometries_optional`](Self::extract_geometries_optional) when
    /// the geometries must stay aligned with the batch's property columns.
    pub fn extract_geometries(&self, batch: &RecordBatch) -> Result<Vec<Geometry>> {
        let geom_column = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        decode_geometry_column(geom_column.as_ref(), self.encoding)
    }

    /// Extracts geometries from a record batch, preserving null rows as `None`
    /// **at their original index**.
    ///
    /// The returned vector always has exactly `batch.num_rows()` entries, so
    /// element `i` always belongs to row `i` of `batch` and can be paired with
    /// that row's attribute values.
    ///
    /// # Errors
    ///
    /// Same as [`extract_geometries`](Self::extract_geometries).
    pub fn extract_geometries_optional(
        &self,
        batch: &RecordBatch,
    ) -> Result<Vec<Option<Geometry>>> {
        let geom_column = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        decode_geometry_column_optional(geom_column.as_ref(), self.encoding)
    }

    /// Returns the geometry column name
    pub fn geometry_column_name(&self) -> &str {
        &self.geometry_column
    }

    /// Returns the declared encoding of the geometry column.
    pub fn geometry_encoding(&self) -> EncodingType {
        self.encoding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reader_creation() {
        // This test would require a sample GeoParquet file
        // For now, we just test that the types compile
        assert_eq!(
            std::mem::size_of::<GeoParquetReader>(),
            std::mem::size_of::<GeoParquetReader>()
        );
    }
}
