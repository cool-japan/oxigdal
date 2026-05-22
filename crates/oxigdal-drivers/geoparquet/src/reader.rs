//! GeoParquet file reader implementation

use crate::arrow_ext::extract_geoparquet_metadata;
use crate::covering::BboxColumns;
use crate::error::{GeoParquetError, Result};
use crate::filter::{AttributePredicates, filter_batch_by_mask};
use crate::geometry::native::native_bbox_mask;
use crate::geometry::{Geometry, WkbReader, decode_native_array, wkb_bbox};
use crate::metadata::{EncodingType, GeoParquetMetadata};
use crate::predicate::{AttributeFilter, CoveringBboxPredicate};
use crate::spatial::{RowGroupBounds, SpatialFilter, SpatialIndex};
use crate::statistics::{
    ColumnStatistics, extract_column_statistics, geometry_has_meaningful_stats,
};
use arrow_array::{Array, BinaryArray, RecordBatch};
use arrow_schema::SchemaRef;
use oxigdal_core::types::BoundingBox;
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{
    ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder, RowFilter,
};
use parquet::file::metadata::ParquetMetaData;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// GeoParquet file reader
pub struct GeoParquetReader {
    /// The underlying file
    file: Arc<File>,
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
    /// Optional attribute filter for pushdown reads.
    attribute_filter: Option<AttributeFilter>,
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

impl GeoParquetReader {
    /// Opens a GeoParquet file for reading
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or is not a valid GeoParquet file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref())?;

        // Build reader to extract metadata
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let schema = builder.schema().clone();
        let parquet_metadata = builder.metadata().clone();

        // Extract GeoParquet metadata
        let metadata_json = extract_geoparquet_metadata(&schema)?
            .ok_or_else(|| GeoParquetError::invalid_metadata("Missing GeoParquet metadata"))?;

        let metadata = GeoParquetMetadata::from_json(&metadata_json)?;
        let geometry_column = metadata.primary_column.clone();

        // Reopen file for actual reading
        let file = File::open(path.as_ref())?;

        Ok(Self {
            file: Arc::new(file),
            schema,
            metadata,
            parquet_metadata,
            spatial_index: None,
            geometry_column,
            bbox_filter: None,
            attribute_filter: None,
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
        self.attribute_filter = Some(filter);
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
        let file = self.file.try_clone()?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        let parquet_schema = builder.parquet_schema().clone();

        // Detect covering bbox columns.
        let bbox_cols = BboxColumns::detect(&parquet_schema, &self.geometry_column);

        // ── 1. Row-group pruning ──────────────────────────────────────────────
        let survivor_row_groups: Vec<usize> =
            if let Some((qxmin, qymin, qxmax, qymax)) = self.bbox_filter {
                (0..self.parquet_metadata.num_row_groups())
                    .filter(|&rg_idx| {
                        let rg = self.parquet_metadata.row_group(rg_idx);
                        if let Some(bc) = bbox_cols.as_ref() {
                            if let Some((rxmin, rymin, rxmax, rymax)) = bc.row_group_bbox(rg) {
                                // AABB intersection (inclusive).
                                return rxmax >= qxmin
                                    && rxmin <= qxmax
                                    && rymax >= qymin
                                    && rymin <= qymax;
                            }
                        }
                        // No stats → can't prune, keep the row group.
                        true
                    })
                    .collect()
            } else {
                (0..self.parquet_metadata.num_row_groups()).collect()
            };

        let builder = builder.with_row_groups(survivor_row_groups);

        // ── 2 & 3. RowFilter predicates ──────────────────────────────────────
        let mut predicates: Vec<Box<dyn parquet::arrow::arrow_reader::ArrowPredicate>> = Vec::new();

        // 2a. Covering bbox predicate (GeoParquet 1.1 fast-path).
        let has_covering_bbox = bbox_cols.is_some();
        if let (Some((qxmin, qymin, qxmax, qymax)), Some(bc)) = (self.bbox_filter, &bbox_cols) {
            // Build column name strings from leaf paths.
            let xmin_name = col_name_from_leaf(&parquet_schema, bc.xmin_col);
            let ymin_name = col_name_from_leaf(&parquet_schema, bc.ymin_col);
            let xmax_name = col_name_from_leaf(&parquet_schema, bc.xmax_col);
            let ymax_name = col_name_from_leaf(&parquet_schema, bc.ymax_col);

            let bbox_proj = ProjectionMask::leaves(
                &parquet_schema,
                [bc.xmin_col, bc.ymin_col, bc.xmax_col, bc.ymax_col],
            );
            let bbox_pred = CoveringBboxPredicate::new(
                xmin_name, ymin_name, xmax_name, ymax_name, qxmin, qymin, qxmax, qymax, bbox_proj,
            );
            predicates.push(Box::new(bbox_pred));
        }

        // 2b. Attribute predicate.
        if let Some(ref attr_filter) = self.attribute_filter {
            let pred = attr_filter
                .clone()
                .to_arrow_predicate(self.schema.clone(), &parquet_schema)?;
            predicates.push(pred);
        }

        let builder = if predicates.is_empty() {
            builder
        } else {
            builder.with_row_filter(RowFilter::new(predicates))
        };

        let mut parquet_reader = builder.build()?;

        // ── 3. Collect + optional WKB post-filter fallback ───────────────────
        let needs_wkb_fallback = self.bbox_filter.is_some() && !has_covering_bbox;

        let wkb_bbox_tuple = if needs_wkb_fallback {
            self.bbox_filter
        } else {
            None
        };

        // Encoding dispatch for the bbox-mask fallback: WKB columns use the
        // legacy `wkb_bbox_mask`; native columns use `native_bbox_mask`.
        let encoding = detect_encoding(&self.metadata, &self.geometry_column);

        let mut results = Vec::new();
        for batch_result in &mut parquet_reader {
            let batch = batch_result?;
            if let Some((qxmin, qymin, qxmax, qymax)) = wkb_bbox_tuple {
                let mask = match encoding {
                    EncodingType::Wkb => {
                        wkb_bbox_mask(&batch, &self.geometry_column, qxmin, qymin, qxmax, qymax)?
                    }
                    native => {
                        let geom_col = batch
                            .column_by_name(&self.geometry_column)
                            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;
                        native_bbox_mask(geom_col.as_ref(), native, qxmin, qymin, qxmax, qymax)?
                    }
                };
                if mask.iter().any(|&b| b) {
                    let filtered = filter_batch_by_mask(&batch, &mask)?;
                    if filtered.num_rows() > 0 {
                        results.push(filtered);
                    }
                }
            } else if batch.num_rows() > 0 {
                results.push(batch);
            }
        }

        Ok(results)
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
            self.file.clone(),
            self.schema.clone(),
            self.geometry_column.clone(),
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

        let file = self.file.try_clone()?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
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
    pub fn read_geometries(&self, row_group: usize) -> Result<Vec<Geometry>> {
        let batch = self.read_row_group(row_group)?;
        let geom_column = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        let encoding = detect_encoding(&self.metadata, &self.geometry_column);
        match encoding {
            EncodingType::Wkb => {
                let binary_array = geom_column
                    .as_any()
                    .downcast_ref::<arrow_array::BinaryArray>()
                    .ok_or_else(|| {
                        GeoParquetError::type_mismatch(
                            "BinaryArray",
                            format!("{:?}", geom_column.data_type()),
                        )
                    })?;

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
            native => decode_native_array(geom_column.as_ref(), native),
        }
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
}

impl GeoParquetBatchReader {
    /// Creates a new batch reader
    fn new(
        file: Arc<File>,
        _schema: SchemaRef,
        geometry_column: String,
        row_groups: Vec<usize>,
    ) -> Result<Self> {
        let file_clone = file.try_clone()?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file_clone)?;
        let reader = builder.with_row_groups(row_groups).build()?;

        Ok(Self {
            reader,
            geometry_column,
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

    /// Extracts geometries from a record batch
    pub fn extract_geometries(&self, batch: &RecordBatch) -> Result<Vec<Geometry>> {
        let geom_column = batch
            .column_by_name(&self.geometry_column)
            .ok_or_else(|| GeoParquetError::missing_field(&self.geometry_column))?;

        let binary_array = geom_column
            .as_any()
            .downcast_ref::<arrow_array::BinaryArray>()
            .ok_or_else(|| {
                GeoParquetError::type_mismatch(
                    "BinaryArray",
                    format!("{:?}", geom_column.data_type()),
                )
            })?;

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

    /// Returns the geometry column name
    pub fn geometry_column_name(&self) -> &str {
        &self.geometry_column
    }
}

// ── Module-level helpers ──────────────────────────────────────────────────────────

/// Returns the last path component (leaf name) for a leaf column at `leaf_idx`.
///
/// For a flat column `geometry_bbox_xmin` the path has one part: `"geometry_bbox_xmin"`.
/// For a struct column `geometry_bbox.xmin` the path has two parts; we return the last: `"xmin"`.
fn col_name_from_leaf(
    schema: &parquet::schema::types::SchemaDescriptor,
    leaf_idx: usize,
) -> String {
    let col = schema.column(leaf_idx);
    col.path()
        .parts()
        .last()
        .cloned()
        .unwrap_or_else(|| col.name().to_owned())
}

/// Builds a WKB-based boolean mask checking each row's geometry bbox against
/// `(qxmin, qymin, qxmax, qymax)`.
fn wkb_bbox_mask(
    batch: &RecordBatch,
    geom_col: &str,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    let col = batch
        .column_by_name(geom_col)
        .ok_or_else(|| GeoParquetError::missing_field(geom_col))?;

    let binary = col.as_any().downcast_ref::<BinaryArray>().ok_or_else(|| {
        GeoParquetError::type_mismatch("BinaryArray", format!("{:?}", col.data_type()))
    })?;

    let mut mask = vec![false; binary.len()];
    for (i, m) in mask.iter_mut().enumerate() {
        if binary.is_null(i) {
            continue;
        }
        let wkb = binary.value(i);
        if let Some((xmin, ymin, xmax, ymax)) = wkb_bbox(wkb) {
            if xmax >= qxmin && xmin <= qxmax && ymax >= qymin && ymin <= qymax {
                *m = true;
            }
        }
    }
    Ok(mask)
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
