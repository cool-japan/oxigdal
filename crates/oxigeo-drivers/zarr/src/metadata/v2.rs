//! Zarr v2 metadata structures
//!
//! This module provides metadata types specific to Zarr v2 specification.

use super::ArrayOrder;
use crate::codecs::CompressorConfig;
use crate::error::{MetadataError, Result, ZarrError};
use serde::{Deserialize, Serialize};

/// Parses a Zarr v2 (numpy-style) dtype string to its element size in bytes.
///
/// Accepts numpy typestrings (`<f4`, `>i8`, `|u1`, `|b1`) and plain names
/// (`float32`, `int64`, `bool`).
///
/// # Errors
/// Returns [`MetadataError::UnsupportedDataType`] for unrecognised dtypes.
pub fn dtype_item_size(dtype: &str) -> Result<usize> {
    let d = dtype.trim();
    let stripped = d.strip_prefix(['<', '>', '|']).unwrap_or(d);

    // numpy typestring form: kind letter + byte count, e.g. "f4", "i8".
    if let Some(size_str) = stripped.get(1..)
        && !size_str.is_empty()
        && let Ok(size) = size_str.parse::<usize>()
    {
        return Ok(size);
    }

    let size = match stripped {
        "bool" | "b1" => 1,
        "int8" | "uint8" | "i1" | "u1" => 1,
        "int16" | "uint16" | "i2" | "u2" => 2,
        "int32" | "uint32" | "float32" | "i4" | "u4" | "f4" => 4,
        "int64" | "uint64" | "float64" | "i8" | "u8" | "f8" => 8,
        "float16" | "f2" => 2,
        "complex64" | "c8" => 8,
        "complex128" | "c16" => 16,
        _ => {
            return Err(ZarrError::Metadata(MetadataError::UnsupportedDataType {
                dtype: dtype.to_string(),
            }));
        }
    };
    Ok(size)
}

/// Converts a Zarr v2 JSON `fill_value` to exactly `item_size` little-endian
/// bytes. Numbers are encoded as int/float according to `is_float`; `null`
/// and unrepresentable values yield zero-fill.
///
/// # Errors
/// Never errors currently, but returns `Result` for forward compatibility.
pub fn fill_value_to_bytes(
    fill_value: &serde_json::Value,
    item_size: usize,
    is_float: bool,
) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; item_size];
    match fill_value {
        serde_json::Value::Null => {}
        serde_json::Value::Bool(b) => {
            if *b && !bytes.is_empty() {
                bytes[0] = 1;
            }
        }
        serde_json::Value::Number(n) => {
            if is_float {
                if let Some(f) = n.as_f64() {
                    match item_size {
                        4 => bytes.copy_from_slice(&(f as f32).to_le_bytes()),
                        8 => bytes.copy_from_slice(&f.to_le_bytes()),
                        _ => {
                            let src = f.to_le_bytes();
                            let copy = item_size.min(src.len());
                            bytes[..copy].copy_from_slice(&src[..copy]);
                        }
                    }
                }
            } else if let Some(i) = n.as_i64() {
                let src = i.to_le_bytes();
                let copy = item_size.min(src.len());
                bytes[..copy].copy_from_slice(&src[..copy]);
            } else if let Some(u) = n.as_u64() {
                let src = u.to_le_bytes();
                let copy = item_size.min(src.len());
                bytes[..copy].copy_from_slice(&src[..copy]);
            }
        }
        serde_json::Value::String(s) => {
            // v2 encodes NaN/Infinity and raw bytes as strings; handle the
            // common float sentinels, otherwise leave as zero-fill.
            if is_float {
                let sentinel = match s.as_str() {
                    "NaN" => Some(f64::NAN),
                    "Infinity" => Some(f64::INFINITY),
                    "-Infinity" => Some(f64::NEG_INFINITY),
                    _ => None,
                };
                if let Some(f) = sentinel {
                    match item_size {
                        4 => bytes.copy_from_slice(&(f as f32).to_le_bytes()),
                        8 => bytes.copy_from_slice(&f.to_le_bytes()),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    Ok(bytes)
}

/// Zarr v2 array metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrayMetadataV2 {
    /// Array shape
    pub shape: Vec<usize>,
    /// Chunk shape
    pub chunks: Vec<usize>,
    /// Data type
    pub dtype: String,
    /// Compressor configuration
    pub compressor: Option<CompressorConfig>,
    /// Fill value
    pub fill_value: serde_json::Value,
    /// Array order
    pub order: ArrayOrder,
    /// Filters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<serde_json::Value>>,
    /// Dimension separator for chunk keys (default ".")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_separator: Option<String>,
    /// Zarr format version (always 2)
    pub zarr_format: u8,
}

impl ArrayMetadataV2 {
    /// Creates new v2 array metadata
    #[must_use]
    pub fn new(shape: Vec<usize>, chunks: Vec<usize>, dtype: impl Into<String>) -> Self {
        Self {
            shape,
            chunks,
            dtype: dtype.into(),
            compressor: None,
            fill_value: serde_json::Value::Null,
            order: ArrayOrder::C,
            filters: None,
            dimension_separator: None,
            zarr_format: 2,
        }
    }

    /// Sets the compressor configuration.
    #[must_use]
    pub fn with_compressor(mut self, compressor: CompressorConfig) -> Self {
        self.compressor = Some(compressor);
        self
    }

    /// Sets the fill value.
    #[must_use]
    pub fn with_fill_value(mut self, fill_value: serde_json::Value) -> Self {
        self.fill_value = fill_value;
        self
    }

    /// Sets the array order.
    #[must_use]
    pub const fn with_order(mut self, order: ArrayOrder) -> Self {
        self.order = order;
        self
    }

    /// Sets the chunk-key dimension separator (`.` or `/`).
    #[must_use]
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.dimension_separator = Some(separator.into());
        self
    }

    /// Returns the effective dimension separator (defaults to `"."`).
    #[must_use]
    pub fn separator(&self) -> &str {
        self.dimension_separator.as_deref().unwrap_or(".")
    }
}
