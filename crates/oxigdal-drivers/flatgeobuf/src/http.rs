//! HTTP range request support for cloud-native `FlatGeobuf` access
//!
//! Enables efficient reading of `FlatGeobuf` files from HTTP sources using
//! range requests to fetch only needed portions of the file.

use crate::MAGIC_BYTES;
use crate::error::{FlatGeobufError, Result};
use crate::feature_codec;
use crate::geometry::GeometryCodec;
use crate::header::Header;
use crate::index::{BoundingBox, PackedRTree};
use byteorder::{LittleEndian, ReadBytesExt};
use oxigdal_core::vector::Feature;
use std::io::{Cursor, Read};

/// HTTP reader for `FlatGeobuf` files
#[cfg(feature = "http")]
pub struct HttpReader {
    url: String,
    client: reqwest::blocking::Client,
    header: Header,
    geometry_codec: GeometryCodec,
    index: Option<PackedRTree>,
    features_offset: u64,
    file_size: Option<u64>,
}

#[cfg(feature = "http")]
impl HttpReader {
    /// Creates a new HTTP reader for the given URL
    pub fn new(url: String) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| FlatGeobufError::Http(format!("Failed to create HTTP client: {e}")))?;

        // Read header and index using range requests
        let mut reader = Self {
            url: url.clone(),
            client: client.clone(),
            header: Header::default(),
            geometry_codec: GeometryCodec::new(false, false),
            index: None,
            features_offset: 0,
            file_size: None,
        };

        reader.initialize()?;

        Ok(reader)
    }

    /// Initializes by reading header and index
    fn initialize(&mut self) -> Result<()> {
        // Get file size using HEAD request
        let head_response = self
            .client
            .head(&self.url)
            .send()
            .map_err(|e| FlatGeobufError::Http(format!("HEAD request failed: {e}")))?;

        self.file_size = head_response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
            .and_then(|s: &str| s.parse::<u64>().ok());

        // Read first chunk to get header (magic + header size + header + potential index)
        // Request first 1MB which should be enough for most headers and indices
        let initial_chunk = self.read_range(0, 1024 * 1024)?;
        let mut cursor = Cursor::new(&initial_chunk);

        // Verify magic bytes
        let mut magic = [0u8; 8];
        cursor.read_exact(&mut magic)?;

        if &magic != MAGIC_BYTES {
            return Err(FlatGeobufError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic.to_vec(),
            });
        }

        // Read the size-prefixed header FlatBuffer.
        let header_size = cursor.read_u32::<LittleEndian>()? as usize;
        let mut header_bytes = vec![0u8; header_size];
        cursor.read_exact(&mut header_bytes)?;
        self.header = Header::from_bytes(&header_bytes)?;

        // Update geometry codec
        self.geometry_codec = GeometryCodec::new(self.header.has_z, self.header.has_m);

        // Read spatial index if present
        if self.header.has_index {
            let feature_count = self.header.features_count.ok_or_else(|| {
                FlatGeobufError::InvalidHeader(
                    "Feature count required when index is present".to_string(),
                )
            })?;

            self.index = Some(PackedRTree::read(&mut cursor, feature_count)?);
        }

        // Record features offset
        self.features_offset = cursor.position();

        Ok(())
    }

    /// Reads a byte range from the URL
    fn read_range(&self, start: u64, length: u64) -> Result<Vec<u8>> {
        let end = start + length - 1;
        let range_header = format!("bytes={start}-{end}");

        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, range_header)
            .send()
            .map_err(|e| FlatGeobufError::Http(format!("Range request failed: {e}")))?;

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(FlatGeobufError::Http(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .map_err(|e| FlatGeobufError::Http(format!("Failed to read response: {e}")))?;

        Ok(bytes.to_vec())
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Returns the spatial index if present
    #[must_use]
    pub const fn index(&self) -> Option<&PackedRTree> {
        self.index.as_ref()
    }

    /// Reads a feature by index
    pub fn read_feature_by_index(&self, index: u64) -> Result<Feature> {
        // If we have a spatial index, use it to find the feature offset
        if let Some(ref spatial_index) = self.index {
            if index >= spatial_index.nodes.len() as u64 {
                return Err(FlatGeobufError::FeatureNotFound(index));
            }

            let node = &spatial_index.nodes[index as usize];
            let offset = self.features_offset + node.offset;

            // Read feature size first (4 bytes)
            let size_bytes = self.read_range(offset, 4)?;
            let mut cursor = Cursor::new(&size_bytes);
            let feature_size = cursor.read_u32::<LittleEndian>()?;

            // Read feature data
            let feature_bytes = self.read_range(offset + 4, u64::from(feature_size))?;
            let feature = self.parse_feature(&feature_bytes)?;

            Ok(feature)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Reading by index requires spatial index".to_string(),
            ))
        }
    }

    /// Queries features in a bounding box
    pub fn query_bbox(&self, bbox: &BoundingBox) -> Result<Vec<Feature>> {
        if let Some(ref index) = self.index {
            let offsets = index.search(bbox);
            let mut features = Vec::with_capacity(offsets.len());

            for offset in offsets {
                match self.read_feature_by_index(offset) {
                    Ok(feature) => features.push(feature),
                    Err(e) => {
                        // Log error but continue with other features
                        eprintln!("Warning: Failed to read feature {offset}: {e}");
                    }
                }
            }

            Ok(features)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Spatial queries require spatial index".to_string(),
            ))
        }
    }

    /// Parses a feature from its `FlatBuffers` `Feature` message bytes.
    fn parse_feature(&self, data: &[u8]) -> Result<Feature> {
        feature_codec::decode_feature(&self.header, &self.geometry_codec, data)
    }
}

/// Async HTTP reader for `FlatGeobuf` files
#[cfg(all(feature = "http", feature = "async"))]
pub struct AsyncHttpReader {
    url: String,
    client: reqwest::Client,
    header: Header,
    geometry_codec: GeometryCodec,
    index: Option<PackedRTree>,
    features_offset: u64,
}

#[cfg(all(feature = "http", feature = "async"))]
impl AsyncHttpReader {
    /// Creates a new async HTTP reader
    pub async fn new(url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| FlatGeobufError::Http(format!("Failed to create HTTP client: {e}")))?;

        let mut reader = Self {
            url: url.clone(),
            client: client.clone(),
            header: Header::default(),
            geometry_codec: GeometryCodec::new(false, false),
            index: None,
            features_offset: 0,
        };

        reader.initialize().await?;

        Ok(reader)
    }

    /// Initializes by reading header and index
    async fn initialize(&mut self) -> Result<()> {
        // Read initial chunk
        let initial_chunk = self.read_range(0, 1024 * 1024).await?;
        let mut cursor = Cursor::new(&initial_chunk);

        // Verify magic bytes
        let mut magic = [0u8; 8];
        cursor.read_exact(&mut magic)?;

        if &magic != MAGIC_BYTES {
            return Err(FlatGeobufError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic.to_vec(),
            });
        }

        // Read the size-prefixed header FlatBuffer.
        let header_size = cursor.read_u32::<LittleEndian>()? as usize;
        let mut header_bytes = vec![0u8; header_size];
        cursor.read_exact(&mut header_bytes)?;
        self.header = Header::from_bytes(&header_bytes)?;
        self.geometry_codec = GeometryCodec::new(self.header.has_z, self.header.has_m);

        // Read index if present
        if self.header.has_index {
            let feature_count = self.header.features_count.ok_or_else(|| {
                FlatGeobufError::InvalidHeader(
                    "Feature count required when index is present".to_string(),
                )
            })?;

            self.index = Some(PackedRTree::read(&mut cursor, feature_count)?);
        }

        self.features_offset = cursor.position();

        Ok(())
    }

    /// Reads a byte range
    async fn read_range(&self, start: u64, length: u64) -> Result<Vec<u8>> {
        let end = start + length - 1;
        let range_header = format!("bytes={start}-{end}");

        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, range_header)
            .send()
            .await
            .map_err(|e| FlatGeobufError::Http(format!("Range request failed: {e}")))?;

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(FlatGeobufError::Http(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| FlatGeobufError::Http(format!("Failed to read response: {e}")))?;

        Ok(bytes.to_vec())
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Queries features in a bounding box
    pub async fn query_bbox(&self, bbox: &BoundingBox) -> Result<Vec<Feature>> {
        if let Some(ref index) = self.index {
            let offsets = index.search(bbox);
            let mut features = Vec::with_capacity(offsets.len());

            for offset in offsets {
                match self.read_feature_by_index(offset).await {
                    Ok(feature) => features.push(feature),
                    Err(e) => {
                        eprintln!("Warning: Failed to read feature {offset}: {e}");
                    }
                }
            }

            Ok(features)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Spatial queries require spatial index".to_string(),
            ))
        }
    }

    /// Reads a feature by index
    async fn read_feature_by_index(&self, index: u64) -> Result<Feature> {
        if let Some(ref spatial_index) = self.index {
            if index >= spatial_index.nodes.len() as u64 {
                return Err(FlatGeobufError::FeatureNotFound(index));
            }

            let node = &spatial_index.nodes[index as usize];
            let offset = self.features_offset + node.offset;

            // Read feature size
            let size_bytes = self.read_range(offset, 4).await?;
            let mut cursor = Cursor::new(&size_bytes);
            let feature_size = cursor.read_u32::<LittleEndian>()?;

            // Read feature data
            let feature_bytes = self.read_range(offset + 4, u64::from(feature_size)).await?;

            // Parse the size-prefixed feature FlatBuffer (same as sync version).
            feature_codec::decode_feature(&self.header, &self.geometry_codec, &feature_bytes)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Reading by index requires spatial index".to_string(),
            ))
        }
    }
}

#[cfg(all(test, feature = "http"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::header::{GeometryType, Header};
    use crate::writer::FlatGeobufWriter;
    use oxigdal_core::vector::{Feature, Geometry, Point};
    use std::io::Write as _;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    /// Builds an in-memory FlatGeobuf file with `n` point features and a
    /// spatial index. `n > 16` yields a multi-level packed R-tree.
    fn build_sample_fgb(n: u32, with_index: bool) -> Vec<u8> {
        let mut header = Header::new(GeometryType::Point);
        if with_index {
            header = header.with_index(true);
        }
        let cursor = Cursor::new(Vec::new());
        let mut writer = FlatGeobufWriter::new(cursor, header).expect("create writer");
        for i in 0..n {
            let cx = f64::from(i) * 4.0 - 40.0;
            let cy = f64::from(i) * 2.0 - 20.0;
            writer
                .add_feature(&Feature::new(Geometry::Point(Point::new(cx, cy))))
                .expect("add feature");
        }
        writer.finish().expect("finish writer").into_inner()
    }

    /// Spawns a minimal HTTP/1.1 server that serves `data` with HEAD support and
    /// single-range GET (`bytes=start-end`) support. Returns the request URL.
    /// The server thread runs until the process exits.
    fn serve(data: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local server");
        let addr = listener.local_addr().expect("local addr");
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = handle_conn(&mut stream, &data);
            }
        });
        format!("http://{addr}/data.fgb")
    }

    fn parse_range(req: &str) -> Option<(usize, usize)> {
        for line in req.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("range:")
                && let Some(spec) = rest.trim().strip_prefix("bytes=")
            {
                let mut it = spec.split('-');
                let start = it.next()?.trim().parse::<usize>().ok()?;
                let end = it.next()?.trim().parse::<usize>().ok()?;
                return Some((start, end));
            }
        }
        None
    }

    fn handle_conn(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
        let mut buf = [0u8; 8192];
        let nread = stream.read(&mut buf)?;
        let req = String::from_utf8_lossy(&buf[..nread]);
        let method = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or("")
            .to_ascii_uppercase();
        let total = data.len();

        if method == "HEAD" {
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(resp.as_bytes())?;
            return Ok(());
        }

        if let Some((start, end_incl)) = parse_range(&req) {
            let start = start.min(total);
            let end = (end_incl + 1).min(total); // exclusive
            let slice = &data[start..end];
            let resp = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                start,
                end.saturating_sub(1),
                total,
                slice.len()
            );
            stream.write_all(resp.as_bytes())?;
            stream.write_all(slice)?;
        } else {
            let resp =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n");
            stream.write_all(resp.as_bytes())?;
            stream.write_all(data)?;
        }
        Ok(())
    }

    #[test]
    fn test_http_reader_initialize_and_query() {
        let n = 20u32;
        let data = build_sample_fgb(n, true);
        let url = serve(data);

        let reader = HttpReader::new(url).expect("open http reader");
        assert!(matches!(reader.header().geometry_type, GeometryType::Point));
        assert!(reader.index().is_some(), "spatial index must be loaded");

        // Direct leaf access by ordinal.
        let f0 = reader.read_feature_by_index(0).expect("read feature 0");
        assert!(f0.geometry.is_some());

        // Full-extent spatial query must return every feature.
        let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0);
        let feats = reader.query_bbox(&bbox).expect("query bbox");
        assert_eq!(
            feats.len(),
            n as usize,
            "full-extent query must return all features via the packed R-tree"
        );
    }

    #[test]
    fn test_http_reader_bad_magic() {
        let data = vec![0u8; 512];
        let url = serve(data);
        assert!(matches!(
            HttpReader::new(url),
            Err(FlatGeobufError::InvalidMagic { .. })
        ));
    }

    #[test]
    fn test_http_reader_truncated_header() {
        let mut data = MAGIC_BYTES.to_vec();
        data.extend_from_slice(&100u32.to_le_bytes()); // header size
        data.extend_from_slice(&[1u8, 2u8]); // truncated header body
        let url = serve(data);
        let res = HttpReader::new(url);
        assert!(res.is_err(), "truncated header must error gracefully");
    }

    #[test]
    fn test_http_reader_head_failure() {
        // Reserve then release a port so nothing is listening -> refused.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let url = format!("http://{addr}/data.fgb");
        assert!(matches!(
            HttpReader::new(url),
            Err(FlatGeobufError::Http(_))
        ));
    }

    #[test]
    fn test_http_reader_query_requires_index() {
        let data = build_sample_fgb(3, false); // no spatial index
        let url = serve(data);
        let reader = HttpReader::new(url).expect("open reader");
        assert!(reader.index().is_none());

        let bbox = BoundingBox::new(-100.0, -100.0, 100.0, 100.0);
        assert!(matches!(
            reader.query_bbox(&bbox),
            Err(FlatGeobufError::NotSupported(_))
        ));
        assert!(matches!(
            reader.read_feature_by_index(0),
            Err(FlatGeobufError::NotSupported(_))
        ));
    }
}
