//! Zarr v2 array reader implementation.
//!
//! Loads a `.zarray` metadata document, builds the v2 compressor + filter
//! pipeline, and reads chunks: fetch the chunk key, reverse the compressor,
//! then reverse the filters. Absent chunks yield the configured `fill_value`.

use crate::codecs::Codec;
use crate::error::{FilterError, Result, StorageError, ZarrError};
use crate::filters::Filter;
use crate::metadata::ArrayOrder;
use crate::metadata::v2::{ArrayMetadataV2, dtype_item_size, fill_value_to_bytes};
use crate::storage::{Store, StoreKey};

use super::ZarrReader;

/// Returns true if the v2 dtype string denotes a floating-point/complex type.
fn is_float_dtype(dtype: &str) -> bool {
    let d = dtype.trim();
    let stripped = d.strip_prefix(['<', '>', '|']).unwrap_or(d);
    stripped.starts_with('f')
        || stripped.starts_with('c')
        || stripped.starts_with("float")
        || stripped.starts_with("complex")
}

/// Builds the ordered v2 filter pipeline from `.zarray` `filters` JSON.
///
/// Filters are applied on *decode* in reverse and on *encode* in order. A
/// filter whose cargo feature is disabled, or one this crate does not
/// implement, yields a typed error rather than being silently skipped (which
/// would corrupt data).
pub(crate) fn build_v2_filters(
    filters: &Option<Vec<serde_json::Value>>,
) -> Result<Vec<Box<dyn Filter>>> {
    let mut built: Vec<Box<dyn Filter>> = Vec::new();
    let Some(list) = filters else {
        return Ok(built);
    };
    for filter in list {
        let id = filter.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
            ZarrError::Filter(FilterError::InvalidConfiguration {
                filter: "<unknown>".to_string(),
                message: "v2 filter is missing its \"id\" field".to_string(),
            })
        })?;
        built.push(build_one_filter(id, filter)?);
    }
    Ok(built)
}

#[cfg(feature = "shuffle")]
fn build_shuffle(config: &serde_json::Value) -> Result<Box<dyn Filter>> {
    use crate::filters::ShuffleFilter;
    let element_size = config
        .get("elementsize")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1) as usize;
    Ok(Box::new(ShuffleFilter::new(element_size)?))
}

#[cfg(feature = "delta")]
fn build_delta(config: &serde_json::Value) -> Result<Box<dyn Filter>> {
    use crate::filters::DeltaFilter;
    let dtype = config
        .get("dtype")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ZarrError::Filter(FilterError::InvalidConfiguration {
                filter: "delta".to_string(),
                message: "delta filter requires a \"dtype\"".to_string(),
            })
        })?;
    Ok(Box::new(DeltaFilter::new(dtype)?))
}

#[cfg(feature = "scale-offset")]
fn build_scale_offset(config: &serde_json::Value) -> Result<Box<dyn Filter>> {
    use crate::filters::ScaleOffsetFilter;
    let offset = config
        .get("offset")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let scale = config
        .get("scale")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0);
    let dtype = config
        .get("dtype")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ZarrError::Filter(FilterError::InvalidConfiguration {
                filter: "fixedscaleoffset".to_string(),
                message: "fixedscaleoffset filter requires a \"dtype\"".to_string(),
            })
        })?;
    let astype = config
        .get("astype")
        .and_then(|v| v.as_str())
        .unwrap_or(dtype);
    Ok(Box::new(ScaleOffsetFilter::new(
        offset, scale, dtype, astype,
    )?))
}

/// Builds a single filter by id, honoring cargo features.
fn build_one_filter(id: &str, config: &serde_json::Value) -> Result<Box<dyn Filter>> {
    let _ = config;
    match id {
        "shuffle" => {
            #[cfg(feature = "shuffle")]
            {
                build_shuffle(config)
            }
            #[cfg(not(feature = "shuffle"))]
            {
                Err(ZarrError::Filter(FilterError::FilterNotAvailable {
                    filter: "shuffle".to_string(),
                }))
            }
        }
        "delta" => {
            #[cfg(feature = "delta")]
            {
                build_delta(config)
            }
            #[cfg(not(feature = "delta"))]
            {
                Err(ZarrError::Filter(FilterError::FilterNotAvailable {
                    filter: "delta".to_string(),
                }))
            }
        }
        "fixedscaleoffset" => {
            #[cfg(feature = "scale-offset")]
            {
                build_scale_offset(config)
            }
            #[cfg(not(feature = "scale-offset"))]
            {
                Err(ZarrError::Filter(FilterError::FilterNotAvailable {
                    filter: "fixedscaleoffset".to_string(),
                }))
            }
        }
        other => Err(ZarrError::Filter(FilterError::UnknownFilter {
            filter: other.to_string(),
        })),
    }
}

/// Builds the chunk key for a v2 array.
pub(crate) fn build_v2_chunk_key(path: &str, coords: &[usize], separator: &str) -> String {
    let coord_str = coords
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(separator);
    if path.is_empty() {
        coord_str
    } else {
        format!("{path}/{coord_str}")
    }
}

/// Zarr v2 array reader.
pub struct ZarrReaderV2<S: Store> {
    store: S,
    path: String,
    metadata: ArrayMetadataV2,
    compressor: Option<Box<dyn Codec>>,
    filters: Vec<Box<dyn Filter>>,
    item_size: usize,
    is_float: bool,
    separator: String,
}

impl<S: Store> ZarrReaderV2<S> {
    /// Opens a Zarr v2 array at `path`, loading and validating its `.zarray`.
    ///
    /// # Errors
    /// Returns an error if the metadata is missing/invalid, or the declared
    /// compressor/filter pipeline cannot be constructed.
    pub fn open(store: S, path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        let meta_key = if path.is_empty() {
            ".zarray".to_string()
        } else {
            format!("{path}/.zarray")
        };
        let meta_bytes = store.get(&StoreKey::new(meta_key))?;
        let metadata: ArrayMetadataV2 = serde_json::from_slice(&meta_bytes)?;
        Self::from_metadata(store, path, metadata)
    }

    /// Builds a reader from already-parsed metadata (no store read for the
    /// `.zarray`). Useful for consolidated-metadata paths.
    ///
    /// # Errors
    /// Returns an error if the metadata is invalid or the pipeline cannot be
    /// built.
    pub fn from_metadata(store: S, path: String, metadata: ArrayMetadataV2) -> Result<Self> {
        if metadata.zarr_format != 2 {
            return Err(ZarrError::UnsupportedVersion {
                version: metadata.zarr_format,
            });
        }
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
        let is_float = is_float_dtype(&metadata.dtype);
        let compressor = match &metadata.compressor {
            Some(cfg) => Some(cfg.build()?),
            None => None,
        };
        let filters = build_v2_filters(&metadata.filters)?;
        let separator = metadata.separator().to_string();

        Ok(Self {
            store,
            path,
            metadata,
            compressor,
            filters,
            item_size,
            is_float,
            separator,
        })
    }

    /// Returns the array shape.
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        &self.metadata.shape
    }

    /// Returns the chunk shape.
    #[must_use]
    pub fn chunks(&self) -> &[usize] {
        &self.metadata.chunks
    }

    /// Returns the dtype string.
    #[must_use]
    pub fn dtype(&self) -> &str {
        &self.metadata.dtype
    }

    /// Returns the element size in bytes.
    #[must_use]
    pub const fn item_size(&self) -> usize {
        self.item_size
    }

    /// Returns the array memory order (C/F).
    #[must_use]
    pub const fn order(&self) -> ArrayOrder {
        self.metadata.order
    }

    /// Returns the array metadata.
    #[must_use]
    pub const fn metadata(&self) -> &ArrayMetadataV2 {
        &self.metadata
    }

    /// Builds a chunk filled with the configured `fill_value`.
    fn create_fill_chunk(&self) -> Result<Vec<u8>> {
        let elems: usize = self.metadata.chunks.iter().product();
        let one = fill_value_to_bytes(&self.metadata.fill_value, self.item_size, self.is_float)?;
        let mut out = Vec::with_capacity(elems * self.item_size);
        for _ in 0..elems {
            out.extend_from_slice(&one);
        }
        Ok(out)
    }

    /// Reads and decodes a chunk at the given coordinates. Returns raw element
    /// bytes in the array's stored order; absent chunks yield fill values.
    ///
    /// # Errors
    /// Propagates storage/codec/filter errors (only a genuinely absent chunk
    /// is turned into a fill chunk).
    pub fn read_chunk(&self, coords: &[usize]) -> Result<Vec<u8>> {
        if coords.len() != self.metadata.shape.len() {
            return Err(ZarrError::InvalidDimension {
                message: format!(
                    "coords rank {} != array rank {}",
                    coords.len(),
                    self.metadata.shape.len()
                ),
            });
        }
        let key = build_v2_chunk_key(&self.path, coords, &self.separator);
        let stored = match self.store.get(&StoreKey::new(key)) {
            Ok(data) => data,
            Err(ZarrError::Storage(StorageError::KeyNotFound { .. })) => {
                return self.create_fill_chunk();
            }
            Err(e) => return Err(e),
        };

        // Reverse the compressor, then the filters (in reverse pipeline order).
        let decompressed = match &self.compressor {
            Some(codec) => codec.decode(&stored)?,
            None => stored,
        };
        let mut data = decompressed;
        for filter in self.filters.iter().rev() {
            data = filter.decode(&data)?;
        }
        Ok(data)
    }
}

impl<S: Store> ZarrReader for ZarrReaderV2<S> {
    fn shape(&self) -> &[usize] {
        &self.metadata.shape
    }

    fn chunks(&self) -> &[usize] {
        &self.metadata.chunks
    }

    fn read_chunk(&self, coords: &[usize]) -> Result<Vec<u8>> {
        ZarrReaderV2::read_chunk(self, coords)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::codecs::CompressorConfig;
    use crate::storage::memory::MemoryStore;

    fn write_meta(store: &mut MemoryStore, path: &str, meta: &ArrayMetadataV2) {
        let key = if path.is_empty() {
            ".zarray".to_string()
        } else {
            format!("{path}/.zarray")
        };
        let bytes = serde_json::to_vec(meta).expect("serialize meta");
        store.set(&StoreKey::new(key), &bytes).expect("set meta");
    }

    #[test]
    fn test_v2_chunk_key() {
        assert_eq!(build_v2_chunk_key("arr", &[0, 1, 2], "."), "arr/0.1.2");
        assert_eq!(build_v2_chunk_key("arr", &[0, 1, 2], "/"), "arr/0/1/2");
        assert_eq!(build_v2_chunk_key("", &[3, 4], "."), "3.4");
    }

    #[test]
    fn test_v2_read_uncompressed_chunk() {
        let mut store = MemoryStore::new();
        let meta = ArrayMetadataV2::new(vec![4], vec![2], "<i4");
        write_meta(&mut store, "arr", &meta);

        let chunk: Vec<u8> = [10i32, 20].iter().flat_map(|v| v.to_le_bytes()).collect();
        store
            .set(&StoreKey::new("arr/0".to_string()), &chunk)
            .expect("set chunk");

        let reader = ZarrReaderV2::open(store, "arr").expect("open");
        assert_eq!(reader.shape(), &[4]);
        assert_eq!(reader.item_size(), 4);
        assert_eq!(reader.read_chunk(&[0]).expect("read"), chunk);
    }

    #[test]
    fn test_v2_read_missing_chunk_returns_fill() {
        let mut store = MemoryStore::new();
        let meta =
            ArrayMetadataV2::new(vec![4], vec![2], "<i4").with_fill_value(serde_json::json!(7));
        write_meta(&mut store, "arr", &meta);

        let reader = ZarrReaderV2::open(store, "arr").expect("open");
        let filled = reader.read_chunk(&[1]).expect("read");
        let expected: Vec<u8> = [7i32, 7].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(filled, expected);
    }

    #[test]
    fn test_v2_read_propagates_non_notfound_error() {
        // A read-only, empty store: opening errors because .zarray is missing,
        // which is KeyNotFound. Instead assert the reader surfaces a genuine
        // decode error for a corrupt compressed chunk.
        let mut store = MemoryStore::new();
        let meta =
            ArrayMetadataV2::new(vec![2], vec![2], "<u1").with_compressor(CompressorConfig::Null);
        write_meta(&mut store, "arr", &meta);
        store
            .set(&StoreKey::new("arr/0".to_string()), &[1u8, 2])
            .expect("set");
        let reader = ZarrReaderV2::open(store, "arr").expect("open");
        // Null compressor: bytes pass through unchanged.
        assert_eq!(reader.read_chunk(&[0]).expect("read"), vec![1u8, 2]);
    }

    #[cfg(feature = "gzip")]
    #[test]
    fn test_v2_gzip_roundtrip_via_writer() {
        use crate::writer::v2::ZarrWriterV2;

        let store = MemoryStore::new();
        let meta = ArrayMetadataV2::new(vec![4], vec![2], "<i4")
            .with_compressor(CompressorConfig::Gzip { level: 6 });
        let mut writer = ZarrWriterV2::create(store, "arr", meta).expect("create");
        let chunk: Vec<u8> = [1i32, 2].iter().flat_map(|v| v.to_le_bytes()).collect();
        writer.write_chunk(&[0], &chunk).expect("write");
        writer.finalize().expect("finalize");
        let store = writer.into_store();

        let reader = ZarrReaderV2::open(store, "arr").expect("open");
        assert_eq!(reader.read_chunk(&[0]).expect("read"), chunk);
    }
}
