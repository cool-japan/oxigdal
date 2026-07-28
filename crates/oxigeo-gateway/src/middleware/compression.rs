//! Response compression middleware.

use super::{Middleware, Request, Response};
use crate::error::Result;

/// Compression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// Gzip compression
    Gzip,
    /// Brotli compression
    Brotli,
    /// No compression
    None,
}

/// Compression configuration.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Minimum size to compress (bytes)
    pub min_size: usize,
    /// Preferred algorithm
    pub algorithm: CompressionAlgorithm,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            min_size: 1024, // 1KB
            algorithm: CompressionAlgorithm::Gzip,
        }
    }
}

/// Compression middleware.
pub struct CompressionMiddleware {
    config: CompressionConfig,
}

impl CompressionMiddleware {
    /// Creates a new compression middleware.
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Compresses data using gzip.
    fn compress_gzip(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_archive::gzip::compress(data, 6)
            .map_err(|e| crate::error::GatewayError::InternalError(e.to_string()))
    }

    /// Compresses data using brotli.
    fn compress_brotli(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_brotli::compress(data, 11)
            .map_err(|e| crate::error::GatewayError::InternalError(e.to_string()))
    }
}

/// Looks up a header value case-insensitively.
///
/// HTTP header names are case-insensitive, but the in-house [`Request`]/[`Response`]
/// representation stores them in an ordinary [`std::collections::HashMap`] keyed by
/// whatever casing the producer used. This scans for the first key that matches
/// `name` ignoring ASCII case.
fn header_value_ci<'a>(
    headers: &'a std::collections::HashMap<String, String>,
    name: &str,
) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Returns `true` when an `Accept-Encoding` field value advertises support for the
/// coding `token` (compared case-insensitively) or the `*` wildcard.
///
/// The value is split on commas into individual codings; any parameters (e.g. a
/// `;q=` weight) are stripped before the name comparison. A coding carrying an
/// explicit `q=0` weight is treated as unacceptable, per RFC 9110.
fn accept_encoding_allows(accept_encoding: &str, token: &str) -> bool {
    for coding in accept_encoding.split(',') {
        let mut segments = coding.split(';');
        let name = segments.next().unwrap_or("").trim();
        if !(name.eq_ignore_ascii_case(token) || name == "*") {
            continue;
        }

        // Honour an explicit `q=0` weight, which marks the coding as unacceptable.
        let rejected = segments.any(|segment| {
            let segment = segment.trim();
            segment
                .strip_prefix("q=")
                .or_else(|| segment.strip_prefix("Q="))
                .and_then(|weight| weight.trim().parse::<f32>().ok())
                .map(|weight| weight <= 0.0)
                .unwrap_or(false)
        });

        if !rejected {
            return true;
        }
    }

    false
}

#[async_trait::async_trait]
impl Middleware for CompressionMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        Ok(())
    }

    async fn after_response(&self, request: &Request, response: &mut Response) -> Result<()> {
        // Never double-encode: if an upstream handler or another middleware already set
        // a Content-Encoding (any casing), leave the body untouched.
        if header_value_ci(&response.headers, "content-encoding").is_some() {
            return Ok(());
        }

        if response.body.len() < self.config.min_size {
            return Ok(());
        }

        // The coding token the configured algorithm would advertise via Content-Encoding.
        let token = match self.config.algorithm {
            CompressionAlgorithm::Gzip => "gzip",
            CompressionAlgorithm::Brotli => "br",
            CompressionAlgorithm::None => return Ok(()),
        };

        // Only compress when the client explicitly advertised support for this coding
        // (or the `*` wildcard) in Accept-Encoding. Absent / mismatched header => skip.
        match header_value_ci(&request.headers, "accept-encoding") {
            Some(accept) if accept_encoding_allows(accept, token) => {}
            _ => return Ok(()),
        }

        let compressed = match self.config.algorithm {
            CompressionAlgorithm::Gzip => {
                response
                    .headers
                    .insert("Content-Encoding".to_string(), "gzip".to_string());
                self.compress_gzip(&response.body)?
            }
            CompressionAlgorithm::Brotli => {
                response
                    .headers
                    .insert("Content-Encoding".to_string(), "br".to_string());
                self.compress_brotli(&response.body)?
            }
            CompressionAlgorithm::None => return Ok(()),
        };

        response.body = compressed;
        response.headers.insert(
            "Content-Length".to_string(),
            response.body.len().to_string(),
        );

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn middleware(algorithm: CompressionAlgorithm) -> CompressionMiddleware {
        CompressionMiddleware::new(CompressionConfig {
            min_size: 4,
            algorithm,
        })
    }

    fn request_with(headers: &[(&str, &str)]) -> Request {
        let mut map = std::collections::HashMap::new();
        for (key, value) in headers {
            map.insert((*key).to_string(), (*value).to_string());
        }
        Request {
            method: "GET".to_string(),
            path: "/api/data".to_string(),
            headers: map,
            body: Vec::new(),
        }
    }

    fn response_with_body(len: usize) -> Response {
        Response {
            status: 200,
            headers: std::collections::HashMap::new(),
            body: vec![b'a'; len],
        }
    }

    // The stored Content-Encoding value looked up case-insensitively.
    fn content_encoding(response: &Response) -> Option<&str> {
        header_value_ci(&response.headers, "content-encoding")
    }

    #[tokio::test]
    async fn test_gzip_compresses_when_accept_encoding_present() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[("Accept-Encoding", "gzip, deflate")]);
        let mut response = response_with_body(64);
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert_eq!(content_encoding(&response), Some("gzip"));
        assert_ne!(response.body, original);
        assert_eq!(
            response.headers.get("Content-Length"),
            Some(&response.body.len().to_string())
        );
    }

    #[tokio::test]
    async fn test_brotli_compresses_when_br_token_present() {
        let mw = middleware(CompressionAlgorithm::Brotli);
        let request = request_with(&[("Accept-Encoding", "br")]);
        let mut response = response_with_body(64);
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert_eq!(content_encoding(&response), Some("br"));
        assert_ne!(response.body, original);
    }

    #[tokio::test]
    async fn test_no_compression_when_accept_encoding_absent() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[]);
        let mut response = response_with_body(64);
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert!(content_encoding(&response).is_none());
        assert_eq!(response.body, original);
    }

    #[tokio::test]
    async fn test_wildcard_accept_encoding_compresses() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[("Accept-Encoding", "*")]);
        let mut response = response_with_body(64);

        mw.after_response(&request, &mut response).await.unwrap();

        assert_eq!(content_encoding(&response), Some("gzip"));
    }

    #[tokio::test]
    async fn test_gzip_rejected_when_only_br_offered() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[("Accept-Encoding", "br")]);
        let mut response = response_with_body(64);
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert!(content_encoding(&response).is_none());
        assert_eq!(response.body, original);
    }

    #[tokio::test]
    async fn test_already_encoded_response_is_skipped() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[("Accept-Encoding", "gzip")]);
        let mut response = response_with_body(64);
        // Existing Content-Encoding with lowercase key must still be detected.
        response
            .headers
            .insert("content-encoding".to_string(), "identity".to_string());
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert_eq!(content_encoding(&response), Some("identity"));
        assert_eq!(response.body, original);
    }

    #[tokio::test]
    async fn test_case_insensitive_accept_encoding_header_key() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        // Header key supplied in a non-canonical casing.
        let request = request_with(&[("accept-encoding", "GZIP")]);
        let mut response = response_with_body(64);

        mw.after_response(&request, &mut response).await.unwrap();

        assert_eq!(content_encoding(&response), Some("gzip"));
    }

    #[tokio::test]
    async fn test_body_below_min_size_is_not_compressed() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[("Accept-Encoding", "gzip")]);
        let mut response = response_with_body(2); // below min_size == 4
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert!(content_encoding(&response).is_none());
        assert_eq!(response.body, original);
    }

    #[tokio::test]
    async fn test_q_zero_weight_rejects_coding() {
        let mw = middleware(CompressionAlgorithm::Gzip);
        let request = request_with(&[("Accept-Encoding", "gzip;q=0, identity")]);
        let mut response = response_with_body(64);
        let original = response.body.clone();

        mw.after_response(&request, &mut response).await.unwrap();

        assert!(content_encoding(&response).is_none());
        assert_eq!(response.body, original);
    }

    #[test]
    fn test_accept_encoding_allows_matrix() {
        assert!(accept_encoding_allows("gzip, br", "gzip"));
        assert!(accept_encoding_allows("gzip, br", "br"));
        assert!(accept_encoding_allows("*", "gzip"));
        assert!(accept_encoding_allows("GZIP", "gzip"));
        assert!(accept_encoding_allows("gzip;q=0.5", "gzip"));
        assert!(!accept_encoding_allows("br", "gzip"));
        assert!(!accept_encoding_allows("gzip;q=0", "gzip"));
        assert!(!accept_encoding_allows("", "gzip"));
        assert!(!accept_encoding_allows("identity", "gzip"));
    }
}
