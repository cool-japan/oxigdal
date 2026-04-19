//! GeoJSON Text Sequences (RFC 8142) reader and writer.
//!
//! Each record is a GeoJSON object on its own line, preceded by U+001E (RS)
//! as a record separator.
//!
//! Two variants are supported:
//!
//! - **GeoJSON-seq** (strict RFC 8142): RS + JSON + LF per record
//! - **Newline-delimited GeoJSON** (NDJSON): JSON + LF per record (no RS)
//!
//! # Example
//!
//! ```
//! use oxigdal_geojson_stream::seq::{SeqReader, SeqWriter};
//! use oxigdal_geojson_stream::types::GeoJsonGeometry;
//! use oxigdal_geojson_stream::types::GeoJsonFeature;
//!
//! // Write features as GeoJSON-seq
//! let mut writer = SeqWriter::new();
//! let feature = GeoJsonFeature {
//!     id: None,
//!     geometry: Some(GeoJsonGeometry::Point([1.0, 2.0])),
//!     properties: None,
//! };
//! writer.write_feature(&feature);
//! let output = writer.finish();
//! assert!(!output.is_empty());
//!
//! // Read them back
//! let reader = SeqReader::new();
//! let features = reader.read_all(output.as_bytes()).expect("valid");
//! assert_eq!(features.len(), 1);
//! ```

use crate::error::GeoJsonError;
use crate::parser::GeoJsonParser;
use crate::types::GeoJsonFeature;
use crate::writer::GeoJsonWriter;

/// ASCII Record Separator (U+001E), used by RFC 8142.
const RS: u8 = 0x1E;

// ─── SeqWriter ──────────────────────────────────────────────────────────────

/// Writes GeoJSON features as GeoJSON Text Sequences (RFC 8142) or NDJSON.
#[derive(Debug)]
pub struct SeqWriter {
    buf: String,
    inner_writer: GeoJsonWriter,
    use_rs: bool,
}

impl SeqWriter {
    /// Create a writer emitting strict RFC 8142 (RS-delimited) records.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            inner_writer: GeoJsonWriter::compact(),
            use_rs: true,
        }
    }

    /// Create a writer emitting NDJSON (no RS prefix).
    #[must_use]
    pub fn ndjson() -> Self {
        Self {
            buf: String::new(),
            inner_writer: GeoJsonWriter::compact(),
            use_rs: false,
        }
    }

    /// Append a feature record.
    pub fn write_feature(&mut self, feature: &GeoJsonFeature) {
        let json = self.inner_writer.write_feature(feature);
        if self.use_rs {
            self.buf.push(RS as char);
        }
        self.buf.push_str(&json);
        self.buf.push('\n');
    }

    /// Consume the writer and return the accumulated output.
    #[must_use]
    pub fn finish(self) -> String {
        self.buf
    }

    /// Return the current buffer contents without consuming the writer.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.buf
    }

    /// Reset the buffer for reuse.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Returns the number of bytes written so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns whether the buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Default for SeqWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── SeqReader ──────────────────────────────────────────────────────────────

/// Reads GeoJSON Text Sequences (RFC 8142) or NDJSON.
///
/// Handles both RS-delimited (RFC 8142) and plain newline-delimited streams.
#[derive(Debug, Default)]
pub struct SeqReader {
    _private: (),
}

impl SeqReader {
    /// Create a new reader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse all features from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if any line contains invalid JSON or is not a valid
    /// GeoJSON Feature.
    pub fn read_all(&self, data: &[u8]) -> Result<Vec<GeoJsonFeature>, GeoJsonError> {
        let text = core::str::from_utf8(data)
            .map_err(|e| GeoJsonError::InvalidCoordinates(format!("Invalid UTF-8: {e}")))?;
        let parser = GeoJsonParser::new();
        let mut features = Vec::new();

        for (line_num, line) in text.lines().enumerate() {
            let trimmed = line.trim().trim_start_matches(RS as char);
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
                GeoJsonError::InvalidCoordinates(format!("Line {}: {e}", line_num + 1))
            })?;
            let feature = parser.parse_feature(&value)?;
            features.push(feature);
        }

        Ok(features)
    }

    /// Iterate features lazily from a byte slice.
    ///
    /// Each call to `next()` parses the next line. Invalid lines yield `Err`.
    pub fn iter<'a>(&'a self, data: &'a [u8]) -> SeqIterator<'a> {
        let text = core::str::from_utf8(data).unwrap_or("");
        SeqIterator {
            lines: text.lines(),
            line_num: 0,
            parser: GeoJsonParser::new(),
            _reader: self,
        }
    }
}

/// Lazy iterator over GeoJSON-seq / NDJSON records.
pub struct SeqIterator<'a> {
    lines: core::str::Lines<'a>,
    line_num: usize,
    parser: GeoJsonParser,
    _reader: &'a SeqReader,
}

impl<'a> Iterator for SeqIterator<'a> {
    type Item = Result<GeoJsonFeature, GeoJsonError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = self.lines.next()?;
            self.line_num += 1;
            let trimmed = line.trim().trim_start_matches(RS as char);
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    return Some(Err(GeoJsonError::InvalidCoordinates(format!(
                        "Line {}: {e}",
                        self.line_num
                    ))));
                }
            };
            return Some(self.parser.parse_feature(&value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GeoJsonGeometry;

    fn sample_feature(lon: f64, lat: f64) -> GeoJsonFeature {
        GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([lon, lat])),
            properties: Some(serde_json::json!({"name": "test"})),
        }
    }

    #[test]
    fn test_seq_writer_roundtrip() {
        let mut writer = SeqWriter::new();
        writer.write_feature(&sample_feature(1.0, 2.0));
        writer.write_feature(&sample_feature(3.0, 4.0));
        let output = writer.finish();

        // Should contain RS characters
        assert!(output.contains('\x1E'));

        let reader = SeqReader::new();
        let features = reader.read_all(output.as_bytes()).expect("valid");
        assert_eq!(features.len(), 2);
    }

    #[test]
    fn test_ndjson_writer_roundtrip() {
        let mut writer = SeqWriter::ndjson();
        writer.write_feature(&sample_feature(10.0, 20.0));
        writer.write_feature(&sample_feature(30.0, 40.0));
        let output = writer.finish();

        // Should NOT contain RS
        assert!(!output.contains('\x1E'));

        let reader = SeqReader::new();
        let features = reader.read_all(output.as_bytes()).expect("valid");
        assert_eq!(features.len(), 2);
    }

    #[test]
    fn test_seq_reader_skips_blank_lines() {
        let input = "\n\n{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[1,2]},\"properties\":null}\n\n";
        let reader = SeqReader::new();
        let features = reader.read_all(input.as_bytes()).expect("valid");
        assert_eq!(features.len(), 1);
    }

    #[test]
    fn test_seq_reader_handles_rs_prefix() {
        let input = "\x1E{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[5,6]},\"properties\":null}\n";
        let reader = SeqReader::new();
        let features = reader.read_all(input.as_bytes()).expect("valid");
        assert_eq!(features.len(), 1);
    }

    #[test]
    fn test_seq_reader_invalid_json() {
        let input = "not json\n";
        let reader = SeqReader::new();
        assert!(reader.read_all(input.as_bytes()).is_err());
    }

    #[test]
    fn test_seq_iterator_lazy() {
        let mut writer = SeqWriter::ndjson();
        for i in 0..5 {
            writer.write_feature(&sample_feature(i as f64, 0.0));
        }
        let output = writer.finish();

        let reader = SeqReader::new();
        let mut iter = reader.iter(output.as_bytes());
        let first = iter.next().expect("should have first").expect("valid");
        assert!(first.geometry.is_some());

        let count = 1 + iter.count(); // count consumes remaining
        assert_eq!(count, 5);
    }

    #[test]
    fn test_seq_writer_clear() {
        let mut writer = SeqWriter::new();
        writer.write_feature(&sample_feature(0.0, 0.0));
        assert!(!writer.is_empty());

        writer.clear();
        assert!(writer.is_empty());
        assert_eq!(writer.len(), 0);
    }

    #[test]
    fn test_seq_writer_len() {
        let writer = SeqWriter::new();
        assert_eq!(writer.len(), 0);
    }
}
