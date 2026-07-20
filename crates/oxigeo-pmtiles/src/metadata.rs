//! Structured parsing of the PMTiles v3 JSON metadata block.
//!
//! The metadata section is an (optionally compressed) UTF-8 JSON object
//! stored between the root-directory region and the leaf-directory region.
//! Well-known fields (`name`, `description`, `format`, `bounds`, `center`,
//! `minzoom`, `maxzoom`, `attribution`) are mapped to typed struct fields;
//! any unknown keys are captured in the `extra` map for forward-compatibility.

use serde::{Deserialize, Serialize};

use crate::error::PmTilesError;
use crate::header::Compression;
use crate::pmtiles::decompress_data;

// ---------------------------------------------------------------------------
// PmTilesMetadata
// ---------------------------------------------------------------------------

/// Structured representation of the JSON metadata embedded in a PMTiles archive.
///
/// The structure mirrors the well-known fields defined in the PMTiles spec and
/// the MBTiles / TileJSON conventions used in practice.  Unknown fields from
/// the JSON object are captured verbatim in [`extra`](Self::extra) so that
/// consumer code can inspect them without loss.
///
/// # Serialisation
/// `None` fields are omitted from the JSON output (`skip_serializing_if`).
/// The `extra` map is flattened, so custom keys appear at the top level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PmTilesMetadata {
    /// Human-readable name for the tile set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Human-readable description of the tile set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Tile format identifier: `"pbf"`, `"png"`, `"jpg"`, `"webp"`, `"avif"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// Geographic bounding box `[west, south, east, north]` in WGS 84 decimal degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[f64; 4]>,

    /// Default map view centre `[longitude, latitude, zoom]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,

    /// Minimum zoom level present in the archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<u8>,

    /// Maximum zoom level present in the archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,

    /// Attribution / copyright string (may contain HTML).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,

    /// All extra (non-standard) fields present in the JSON object.
    ///
    /// This field is flattened during (de)serialisation so that each key
    /// appears at the top level of the JSON output.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PmTilesMetadata {
    /// Parse metadata from raw (possibly compressed) bytes.
    ///
    /// 1. Decompresses the bytes using `compression`.
    /// 2. Interprets the result as UTF-8 JSON.
    /// 3. Deserialises the JSON into [`PmTilesMetadata`].
    ///
    /// An empty byte slice is treated as `{}` and produces an all-`None`
    /// instance with an empty `extra` map.
    ///
    /// # Errors
    /// - [`PmTilesError::Decompression`] when decompression fails.
    /// - [`PmTilesError::JsonParse`] when the bytes are not valid UTF-8 JSON or
    ///   cannot be deserialised into this type.
    pub fn from_bytes(data: &[u8], compression: Compression) -> Result<Self, PmTilesError> {
        // Handle the empty-metadata case explicitly to avoid a JSON parse error
        // on an empty byte slice.
        if data.is_empty() {
            return Ok(Self::empty());
        }

        // Step 1 — decompress if required.
        let decompressed = decompress_data(data, &compression)?;

        // Step 2 — interpret as UTF-8.
        let json_str = std::str::from_utf8(&decompressed)
            .map_err(|e| PmTilesError::JsonParse(format!("Metadata is not valid UTF-8: {e}")))?;

        // Handle the case where the stored JSON is literally empty or whitespace.
        let trimmed = json_str.trim();
        if trimmed.is_empty() {
            return Ok(Self::empty());
        }

        // Step 3 — deserialise.
        serde_json::from_str(trimmed)
            .map_err(|e| PmTilesError::JsonParse(format!("Failed to parse metadata JSON: {e}")))
    }

    /// Construct an empty metadata instance (all fields `None`, no extra keys).
    pub fn empty() -> Self {
        Self {
            name: None,
            description: None,
            format: None,
            bounds: None,
            center: None,
            minzoom: None,
            maxzoom: None,
            attribution: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Serialise this metadata back to a compact JSON string.
    ///
    /// # Errors
    /// Returns [`PmTilesError::JsonParse`] if serialisation fails (should be
    /// infallible for well-formed data).
    pub fn to_json(&self) -> Result<String, PmTilesError> {
        serde_json::to_string(self)
            .map_err(|e| PmTilesError::JsonParse(format!("Failed to serialise metadata: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_bytes_produces_empty_metadata() {
        let meta = PmTilesMetadata::from_bytes(b"", Compression::None).expect("ok");
        assert!(meta.name.is_none());
        assert!(meta.extra.is_empty());
    }

    #[test]
    fn test_empty_json_object_parses() {
        let meta = PmTilesMetadata::from_bytes(b"{}", Compression::None).expect("ok");
        assert!(meta.name.is_none());
        assert!(meta.extra.is_empty());
    }

    #[test]
    fn test_name_and_format_parsed() {
        let json = br#"{"name":"roads","format":"pbf"}"#;
        let meta = PmTilesMetadata::from_bytes(json, Compression::None).expect("ok");
        assert_eq!(meta.name.as_deref(), Some("roads"));
        assert_eq!(meta.format.as_deref(), Some("pbf"));
    }

    #[test]
    fn test_bounds_parsed() {
        let json = br#"{"bounds":[-180.0,-90.0,180.0,90.0]}"#;
        let meta = PmTilesMetadata::from_bytes(json, Compression::None).expect("ok");
        assert_eq!(meta.bounds, Some([-180.0, -90.0, 180.0, 90.0]));
    }

    #[test]
    fn test_center_parsed() {
        let json = br#"{"center":[10.5,20.5,7]}"#;
        let meta = PmTilesMetadata::from_bytes(json, Compression::None).expect("ok");
        assert_eq!(meta.center, Some([10.5, 20.5, 7.0]));
    }

    #[test]
    fn test_extra_fields_captured() {
        let json = br#"{"name":"x","custom_key":42,"nested":{"a":1}}"#;
        let meta = PmTilesMetadata::from_bytes(json, Compression::None).expect("ok");
        assert_eq!(meta.name.as_deref(), Some("x"));
        assert_eq!(
            meta.extra.get("custom_key"),
            Some(&serde_json::Value::Number(42.into()))
        );
        assert!(meta.extra.contains_key("nested"));
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let result = PmTilesMetadata::from_bytes(b"not json", Compression::None);
        assert!(result.is_err());
        assert!(matches!(result, Err(PmTilesError::JsonParse(_))));
    }

    #[test]
    fn test_roundtrip_serialisation() {
        let json = r#"{"name":"test","format":"png","minzoom":0,"maxzoom":5}"#;
        let meta =
            PmTilesMetadata::from_bytes(json.as_bytes(), Compression::None).expect("parse ok");
        assert_eq!(meta.name.as_deref(), Some("test"));
        assert_eq!(meta.minzoom, Some(0));
        assert_eq!(meta.maxzoom, Some(5));
        // Re-serialise and re-parse.
        let back = meta.to_json().expect("serialise ok");
        let meta2 =
            PmTilesMetadata::from_bytes(back.as_bytes(), Compression::None).expect("re-parse ok");
        assert_eq!(meta2.name, meta.name);
        assert_eq!(meta2.maxzoom, meta.maxzoom);
    }
}
