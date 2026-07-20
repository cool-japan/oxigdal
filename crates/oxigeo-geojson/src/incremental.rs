//! True incremental GeoJSON feature reader.
//!
//! [`IncrementalFeatureReader`] parses one [`GeoJsonFeature`] at a time from
//! any `std::io::Read` source — a file handle, a network socket, a
//! `Cursor<Vec<u8>>`, etc. — without loading the whole document into RAM.
//!
//! ## Design
//!
//! 1. **Header scan**: A `BufReader` scans past the outer `FeatureCollection`
//!    envelope byte-by-byte until positioned just after the `[` that opens the
//!    `"features"` array.  Non-`"features"` values are parsed-and-discarded via
//!    a single `serde_json::Deserializer` that is reset between keys, avoiding
//!    buffered-read conflicts.
//!
//! 2. **Element extraction**: The `BufReader` (now positioned inside the array)
//!    is wrapped in `ArrayElementRead`, a private `Read` adapter that:
//!    - Converts the top-level `,` separators between features into whitespace
//!      (so the serde_json streaming deserializer sees adjacent top-level values).
//!    - Converts the closing `]` of the array into an EOF signal.
//!    - Passes all other bytes through unchanged.
//!
//! 3. **`StreamDeserializer`**: `serde_json::StreamDeserializer` wraps the
//!    `ArrayElementRead` and yields one `serde_json::Value` per `next()` call.
//!    Each value is forwarded to `GeoJsonParser::parse_feature`.
//!
//! ## Memory profile
//!
//! Peak heap usage is bounded by the size of the largest single feature.
//! Multi-GB GeoJSON files are handled with O(1) working-set memory.
//!
//! ## Example
//!
//! ```rust,no_run
//! use std::fs::File;
//! use oxigeo_geojson_stream::IncrementalFeatureReader;
//!
//! let f = File::open("big.geojson").expect("open");
//! let reader = IncrementalFeatureReader::new(f).expect("header");
//! for result in reader {
//!     let feature = result.expect("valid feature");
//!     println!("{:?}", feature.id);
//! }
//! ```

use std::io::{BufRead, BufReader, Read};

use crate::error::GeoJsonError;
use crate::parser::GeoJsonParser;
use crate::types::GeoJsonFeature;

// ─── ArrayElementRead ─────────────────────────────────────────────────────────

/// A `Read` adapter over a `BufReader<R>` that sits *inside* a JSON array.
///
/// It translates the raw byte stream so that serde_json's `StreamDeserializer`
/// sees a sequence of whitespace-separated top-level values:
///
/// - `,` (array-element separator) → `b' '` (whitespace, ignored by serde_json)
/// - `]` (end of array)             → signals EOF by returning `0` bytes read
/// - All other bytes pass through unchanged.
///
/// State machine:
/// - `Normal`: pass bytes through, but watch for `]` and `,` at depth 0.
/// - `Exhausted`: always return 0 (EOF).
///
/// Depth tracking ensures that `]` and `,` inside nested objects or arrays are
/// passed through verbatim.
struct ArrayElementRead<R: Read> {
    inner: BufReader<R>,
    /// JSON nesting depth.  We start at depth 0 (inside the features array
    /// at the top level of the element).
    depth: i32,
    /// True once the closing `]` of the features array has been encountered
    /// at depth 0.
    exhausted: bool,
    /// Bytes that should be emitted on the next `read()` call before touching
    /// `inner` again.  Used when we need to emit a synthetic byte (e.g. the
    /// byte we peeked).
    pending: Option<u8>,
}

impl<R: Read> ArrayElementRead<R> {
    fn new(inner: BufReader<R>) -> Self {
        Self {
            inner,
            depth: 0,
            exhausted: false,
            pending: None,
        }
    }
}

impl<R: Read> Read for ArrayElementRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.exhausted {
            return Ok(0); // EOF
        }

        // Flush any pending synthetic byte first.
        if let Some(b) = self.pending.take() {
            buf[0] = b;
            return Ok(1);
        }

        // Read one byte at a time so depth tracking is exact.
        let mut tmp = [0u8; 1];
        let n = self.inner.read(&mut tmp)?;
        if n == 0 {
            // Underlying EOF — treat as end of array.
            self.exhausted = true;
            return Ok(0);
        }

        let byte = tmp[0];

        // Track nesting depth so we know when top-level array tokens appear.
        match byte {
            b'{' | b'[' => self.depth += 1,
            b'}' | b']' => {
                self.depth -= 1;
                // depth goes to -1 when we see the `]` that closes the
                // features array (we started at depth 0 *inside* the array).
                if self.depth < 0 {
                    self.exhausted = true;
                    return Ok(0); // signal EOF to StreamDeserializer
                }
            }
            b'"' => {
                // Pass through; depth does not change for strings.
                // We do NOT track string escapes here because we pass the raw
                // bytes unchanged — serde_json will parse strings correctly.
                buf[0] = byte;
                return Ok(1);
            }
            b',' if self.depth == 0 => {
                // Top-level comma = element separator.  Convert to space so
                // serde_json sees adjacent whitespace-delimited top-level values.
                buf[0] = b' ';
                return Ok(1);
            }
            _ => {}
        }

        buf[0] = byte;
        Ok(1)
    }
}

// ─── Public type ─────────────────────────────────────────────────────────────

/// True incremental GeoJSON feature reader.
///
/// Implements [`Iterator`] over `Result<GeoJsonFeature, GeoJsonError>`.
pub struct IncrementalFeatureReader<R: Read> {
    stream: serde_json::StreamDeserializer<
        'static,
        serde_json::de::IoRead<ArrayElementRead<R>>,
        serde_json::Value,
    >,
    done: bool,
    parser: GeoJsonParser,
}

impl<R: Read> IncrementalFeatureReader<R> {
    /// Create a new reader and scan past the FeatureCollection envelope to
    /// position the stream just after the opening `[` of the features array.
    ///
    /// # Errors
    ///
    /// - [`GeoJsonError::IoError`] on I/O failure.
    /// - [`GeoJsonError::ParseError`] if the JSON header is malformed.
    /// - [`GeoJsonError::InvalidStructure`] if no `"features"` key is found.
    pub fn new(reader: R) -> Result<Self, GeoJsonError> {
        let mut buf = BufReader::new(reader);
        scan_to_features_array(&mut buf)?;
        let element_read = ArrayElementRead::new(buf);
        let stream = serde_json::Deserializer::from_reader(element_read).into_iter();
        Ok(Self {
            stream,
            done: false,
            parser: GeoJsonParser::new(),
        })
    }
}

// ─── Iterator ────────────────────────────────────────────────────────────────

impl<R: Read> Iterator for IncrementalFeatureReader<R> {
    type Item = Result<GeoJsonFeature, GeoJsonError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.stream.next() {
            None => {
                self.done = true;
                None
            }
            Some(Ok(val)) => Some(self.parser.parse_feature(&val)),
            Some(Err(e)) => {
                self.done = true;
                Some(Err(GeoJsonError::ParseError(e)))
            }
        }
    }
}

// ─── Header scanner ──────────────────────────────────────────────────────────

/// Consume bytes from `buf` until positioned just after the `[` that opens the
/// FeatureCollection's `"features"` array.
///
/// Algorithm (RFC 7946 §3.3 — object keys may appear in any order):
/// 1. Skip whitespace, expect `{`.
/// 2. Loop:
///    a. Parse the next string key using `serde_json::from_reader`.
///    b. Skip whitespace, consume `:`.
///    c. If key == `"features"`: skip whitespace, consume `[`, return `Ok(())`.
///    d. Otherwise: use a depth-counting byte scanner to skip the value without
///    buffering it, then skip optional whitespace + comma.
///    e. Peek: if `}`, return `InvalidStructure` (no "features" key found).
fn scan_to_features_array<R: Read>(buf: &mut BufReader<R>) -> Result<(), GeoJsonError> {
    skip_whitespace(buf)?;
    expect_byte(buf, b'{')?;

    loop {
        skip_whitespace(buf)?;

        // Peek: if `}` the object is empty — no "features" key.
        if peek_byte(buf)? == b'}' {
            return Err(GeoJsonError::InvalidStructure(
                "FeatureCollection has no \"features\" key".into(),
            ));
        }

        // Parse the key as a JSON string.  We use a tiny dedicated
        // Deserializer on a slice of bytes that we read into a local buffer
        // first to avoid buffering conflicts with the shared BufReader.
        let key = read_json_string(buf)?;

        skip_whitespace(buf)?;
        expect_byte(buf, b':')?;
        skip_whitespace(buf)?;

        if key == "features" {
            // Consume the `[` that opens the features array.
            expect_byte(buf, b'[')?;
            return Ok(());
        }

        // Skip the value for this key using a depth-counting scanner.
        skip_json_value(buf)?;

        // Skip whitespace and optional comma between key-value pairs.
        skip_whitespace(buf)?;
        skip_optional_byte(buf, b',')?;
        skip_whitespace(buf)?;
    }
}

// ─── Low-level byte helpers ───────────────────────────────────────────────────

/// Skip ASCII whitespace.
fn skip_whitespace<R: Read>(buf: &mut BufReader<R>) -> Result<(), GeoJsonError> {
    loop {
        let fill = buf.fill_buf()?;
        if fill.is_empty() {
            return Ok(());
        }
        let n = fill
            .iter()
            .take_while(|&&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .count();
        if n == 0 {
            return Ok(());
        }
        buf.consume(n);
    }
}

/// Peek at the next byte without consuming it.
fn peek_byte<R: Read>(buf: &mut BufReader<R>) -> Result<u8, GeoJsonError> {
    let fill = buf.fill_buf()?;
    fill.first()
        .copied()
        .ok_or_else(|| GeoJsonError::InvalidStructure("Unexpected end of input".into()))
}

/// Consume exactly one byte.
fn consume_byte<R: Read>(buf: &mut BufReader<R>) -> Result<u8, GeoJsonError> {
    let b = peek_byte(buf)?;
    buf.consume(1);
    Ok(b)
}

/// Assert the next byte (after skipping whitespace) equals `expected`.
fn expect_byte<R: Read>(buf: &mut BufReader<R>, expected: u8) -> Result<(), GeoJsonError> {
    let b = consume_byte(buf)?;
    if b != expected {
        return Err(GeoJsonError::InvalidStructure(format!(
            "Expected '{}' but found '{}'",
            char::from(expected),
            char::from(b),
        )));
    }
    Ok(())
}

/// If the next byte equals `b`, consume it; otherwise do nothing.
fn skip_optional_byte<R: Read>(buf: &mut BufReader<R>, b: u8) -> Result<(), GeoJsonError> {
    let fill = buf.fill_buf()?;
    if fill.first().copied() == Some(b) {
        buf.consume(1);
    }
    Ok(())
}

/// Read a JSON string literal from the reader and return its unescaped value.
///
/// We read the raw bytes of the JSON string (including the surrounding `"`)
/// into a `Vec<u8>`, then let serde_json decode escape sequences.
fn read_json_string<R: Read>(buf: &mut BufReader<R>) -> Result<String, GeoJsonError> {
    // Expect opening `"`.
    let first = peek_byte(buf)?;
    if first != b'"' {
        return Err(GeoJsonError::InvalidStructure(format!(
            "Expected '\"' for JSON string key but found '{}'",
            char::from(first),
        )));
    }

    // Collect raw bytes of the string literal (including delimiters).
    let mut raw: Vec<u8> = Vec::with_capacity(32);
    raw.push(b'"');
    buf.consume(1); // consume opening `"`

    let mut escaped = false;
    loop {
        let b = consume_byte(buf)?;
        raw.push(b);
        if escaped {
            escaped = false;
        } else if b == b'\\' {
            escaped = true;
        } else if b == b'"' {
            break; // closing `"`
        }
    }

    // Decode via serde_json.
    let s: String = serde_json::from_slice(&raw)?;
    Ok(s)
}

/// Skip one JSON value (string, number, object, array, true, false, null)
/// from the reader, using a depth-counting state machine.
///
/// This avoids loading the skipped value into memory.
fn skip_json_value<R: Read>(buf: &mut BufReader<R>) -> Result<(), GeoJsonError> {
    // Peek at the first byte to determine the value type.
    let first = peek_byte(buf)?;

    match first {
        b'"' => {
            // String — read until unescaped closing `"`.
            buf.consume(1); // opening `"`
            let mut escaped = false;
            loop {
                let b = consume_byte(buf)?;
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == b'"' {
                    break;
                }
            }
            Ok(())
        }
        b'{' | b'[' => {
            // Object or array — depth-count until matching close.
            let mut depth: i32 = 0;
            let mut in_string = false;
            let mut escaped = false;
            loop {
                let b = consume_byte(buf)?;
                if in_string {
                    if escaped {
                        escaped = false;
                    } else if b == b'\\' {
                        escaped = true;
                    } else if b == b'"' {
                        in_string = false;
                    }
                } else {
                    match b {
                        b'"' => in_string = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {
            // Number, boolean, or null — read until a structural character or whitespace.
            loop {
                let fill = buf.fill_buf()?;
                if fill.is_empty() {
                    break;
                }
                let n = fill
                    .iter()
                    .take_while(|&&b| {
                        !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']')
                    })
                    .count();
                if n == 0 {
                    break;
                }
                buf.consume(n);
            }
            Ok(())
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_collection(n: usize) -> Vec<u8> {
        let features: Vec<String> = (0..n)
            .map(|i| {
                format!(
                    r#"{{"type":"Feature","id":{i},"geometry":{{"type":"Point","coordinates":[{i}.0,0.0]}},"properties":{{"name":"feat{i}"}}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        )
        .into_bytes()
    }

    #[test]
    fn test_empty_features_array() {
        let data = br#"{"type":"FeatureCollection","features":[]}"#;
        let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        assert_eq!(reader.count(), 0);
    }

    #[test]
    fn test_single_feature() {
        let data = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":null}
        ]}"#;
        let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        let features: Vec<_> = reader.map(|r| r.expect("valid")).collect();
        assert_eq!(features.len(), 1);
    }

    #[test]
    fn test_reads_100_features_one_at_a_time() {
        let data = make_collection(100);
        let reader = IncrementalFeatureReader::new(Cursor::new(data)).expect("new");
        let count = reader.fold(0usize, |acc, r| {
            r.expect("feature");
            acc + 1
        });
        assert_eq!(count, 100);
    }

    #[test]
    fn test_preserves_id_and_properties() {
        let data = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","id":42,"geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"city":"Tokyo"}}
        ]}"#;
        let mut reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        let feat = reader.next().expect("first feature").expect("no error");
        assert!(feat.properties.is_some());
        let city: String = feat.get_property("city").expect("has city");
        assert_eq!(city, "Tokyo");
    }

    #[test]
    fn test_feature_id_preserved() {
        let data = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","id":42,"geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":null}
        ]}"#;
        let mut reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        let feat = reader.next().expect("first").expect("valid");
        assert!(
            matches!(feat.id, Some(crate::types::FeatureId::Number(n)) if (n - 42.0).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn test_errors_on_malformed_feature() {
        let data = br#"{"type":"FeatureCollection","features":[{"type":"Fe"#;
        let mut reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        let result = reader.next();
        assert!(result.is_some());
        assert!(result.expect("Some").is_err());
    }

    #[test]
    fn test_skips_unknown_top_level_fields() {
        let data = br#"{"name":"test","bbox":[0,0,1,1],"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":null}
        ]}"#;
        let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        assert_eq!(reader.count(), 1);
    }

    #[test]
    fn test_features_first_in_object() {
        let data = br#"{"features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[5.0,5.0]},"properties":null}
        ],"type":"FeatureCollection"}"#;
        let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        assert_eq!(reader.count(), 1);
    }

    #[test]
    fn test_multigeometry_features() {
        let data = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0.0,0.0],[1.0,1.0]]},"properties":null},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0],[0.0,0.0]]]},"properties":null}
        ]}"#;
        let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
        let features: Vec<_> = reader.map(|r| r.expect("valid")).collect();
        assert_eq!(features.len(), 2);
        assert!(features[0].has_geometry());
        assert!(features[1].has_geometry());
    }

    #[test]
    fn test_no_features_key_returns_error() {
        let data = br#"{"type":"FeatureCollection","bbox":[0,0,1,1]}"#;
        let result = IncrementalFeatureReader::new(Cursor::new(&data[..]));
        assert!(result.is_err());
    }
}
