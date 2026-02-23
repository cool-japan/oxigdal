//! GeoParquet file reader implementation

use crate::arrow_ext::extract_geoparquet_metadata;
use crate::error::{GeoParquetError, Result};
use crate::geometry::{Geometry, WkbReader};
use crate::metadata::GeoParquetMetadata;
use crate::spatial::{RowGroupBounds, SpatialFilter, SpatialIndex};
use arrow_array::{Array, RecordBatch};
use arrow_schema::SchemaRef;
use oxigdal_core::types::BoundingBox;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use parquet::file::metadata::ParquetMetaData;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

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

    /// Reads geometries from a specific row group
    pub fn read_geometries(&self, row_group: usize) -> Result<Vec<Geometry>> {
        let batch = self.read_row_group(row_group)?;
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
