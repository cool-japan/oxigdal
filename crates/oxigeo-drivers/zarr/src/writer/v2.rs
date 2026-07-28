//! Zarr v2 array writer implementation.
//!
//! Writes a `.zarray` metadata document and encodes chunks through the v2
//! filter pipeline (in order) followed by the compressor, mirroring the
//! reader's inverse pipeline.

use crate::codecs::Codec;
use crate::error::{Result, ZarrError};
use crate::filters::Filter;
use crate::metadata::v2::{ArrayMetadataV2, dtype_item_size};
use crate::reader::v2::{build_v2_chunk_key, build_v2_filters};
use crate::storage::{Store, StoreKey};

use super::ZarrWriter;

/// Zarr v2 array writer.
pub struct ZarrWriterV2<S: Store> {
    store: S,
    path: String,
    metadata: ArrayMetadataV2,
    compressor: Option<Box<dyn Codec>>,
    filters: Vec<Box<dyn Filter>>,
    item_size: usize,
    separator: String,
}

impl<S: Store> ZarrWriterV2<S> {
    /// Creates a Zarr v2 array at `path`, building the pipeline and writing
    /// the `.zarray` metadata immediately.
    ///
    /// # Errors
    /// Returns an error if the metadata is invalid or the compressor/filter
    /// pipeline cannot be built or the metadata write fails.
    pub fn create(store: S, path: impl Into<String>, metadata: ArrayMetadataV2) -> Result<Self> {
        let path = path.into();

        if metadata.shape.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "v2 array shape cannot be empty".to_string(),
            });
        }
        if metadata.chunks.len() != metadata.shape.len() {
            return Err(ZarrError::InvalidDimension {
                message: format!(
                    "chunks rank {} != shape rank {}",
                    metadata.chunks.len(),
                    metadata.shape.len()
                ),
            });
        }
        if metadata.chunks.contains(&0) {
            return Err(ZarrError::InvalidDimension {
                message: "v2 chunk dimensions must all be non-zero".to_string(),
            });
        }

        let item_size = dtype_item_size(&metadata.dtype)?;
        let compressor = match &metadata.compressor {
            Some(cfg) => Some(cfg.build()?),
            None => None,
        };
        let filters = build_v2_filters(&metadata.filters)?;
        let separator = metadata.separator().to_string();

        let mut writer = Self {
            store,
            path,
            metadata,
            compressor,
            filters,
            item_size,
            separator,
        };
        writer.write_metadata()?;
        Ok(writer)
    }

    /// Writes the `.zarray` metadata document.
    fn write_metadata(&mut self) -> Result<()> {
        let key = if self.path.is_empty() {
            ".zarray".to_string()
        } else {
            format!("{}/.zarray", self.path)
        };
        let bytes = serde_json::to_vec_pretty(&self.metadata)?;
        self.store.set(&StoreKey::new(key), &bytes)
    }

    /// Encodes and writes a chunk at the given coordinates.
    ///
    /// The chunk `data` must be exactly `chunk_elems * item_size` bytes.
    ///
    /// # Errors
    /// Returns an error on rank/size mismatch or a filter/codec failure.
    pub fn write_chunk(&mut self, coords: &[usize], data: &[u8]) -> Result<()> {
        if coords.len() != self.metadata.shape.len() {
            return Err(ZarrError::InvalidDimension {
                message: format!(
                    "coords rank {} != array rank {}",
                    coords.len(),
                    self.metadata.shape.len()
                ),
            });
        }
        let elems: usize = self.metadata.chunks.iter().product();
        let expected = elems * self.item_size;
        if data.len() != expected {
            return Err(ZarrError::InvalidShape {
                expected: vec![expected],
                actual: vec![data.len()],
            });
        }

        // Filters first (in pipeline order), then the compressor.
        let mut encoded = data.to_vec();
        for filter in &self.filters {
            encoded = filter.encode(&encoded)?;
        }
        let stored = match &self.compressor {
            Some(codec) => codec.encode(&encoded)?,
            None => encoded,
        };

        let key = build_v2_chunk_key(&self.path, coords, &self.separator);
        self.store.set(&StoreKey::new(key), &stored)
    }

    /// Flushes any pending writes.
    ///
    /// # Errors
    /// Returns an error if the underlying store flush fails.
    pub fn finalize(&mut self) -> Result<()> {
        self.store.flush()
    }

    /// Consumes the writer and returns the underlying store.
    #[must_use]
    pub fn into_store(self) -> S {
        self.store
    }

    /// Returns the array metadata.
    #[must_use]
    pub const fn metadata(&self) -> &ArrayMetadataV2 {
        &self.metadata
    }
}

impl<S: Store> ZarrWriter for ZarrWriterV2<S> {
    fn write_chunk(&mut self, coords: &[usize], data: &[u8]) -> Result<()> {
        ZarrWriterV2::write_chunk(self, coords, data)
    }

    fn finalize(&mut self) -> Result<()> {
        ZarrWriterV2::finalize(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::metadata::v2::ArrayMetadataV2;
    use crate::reader::v2::ZarrReaderV2;
    use crate::storage::memory::MemoryStore;

    #[test]
    fn test_v2_writer_roundtrip_uncompressed() {
        let store = MemoryStore::new();
        let meta = ArrayMetadataV2::new(vec![4, 4], vec![2, 2], "<f4");
        let mut writer = ZarrWriterV2::create(store, "arr", meta).expect("create");

        let values = [1.0f32, 2.0, 3.0, 4.0];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        writer.write_chunk(&[0, 0], &data).expect("write");
        writer.finalize().expect("finalize");
        let store = writer.into_store();

        let reader = ZarrReaderV2::open(store, "arr").expect("open");
        assert_eq!(reader.read_chunk(&[0, 0]).expect("read"), data);
    }

    #[test]
    fn test_v2_writer_rejects_wrong_size() {
        let store = MemoryStore::new();
        let meta = ArrayMetadataV2::new(vec![4], vec![2], "<f4");
        let mut writer = ZarrWriterV2::create(store, "arr", meta).expect("create");
        // chunk of 2 f4 = 8 bytes expected, provide 4.
        assert!(writer.write_chunk(&[0], &[0u8; 4]).is_err());
    }

    #[cfg(feature = "delta")]
    #[test]
    fn test_v2_writer_with_delta_filter_roundtrip() {
        let store = MemoryStore::new();
        let meta = ArrayMetadataV2 {
            filters: Some(vec![serde_json::json!({"id":"delta","dtype":"<i4"})]),
            ..ArrayMetadataV2::new(vec![4], vec![4], "<i4")
        };
        let mut writer = ZarrWriterV2::create(store, "arr", meta).expect("create");
        let values = [100i32, 101, 103, 106];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        writer.write_chunk(&[0], &data).expect("write");
        writer.finalize().expect("finalize");
        let store = writer.into_store();

        let reader = ZarrReaderV2::open(store, "arr").expect("open");
        assert_eq!(reader.read_chunk(&[0]).expect("read"), data);
    }
}
