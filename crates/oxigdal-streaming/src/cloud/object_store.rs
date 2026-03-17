//! Cloud object storage abstractions: URL parsing, byte-range requests,
//! credentials, presigned URLs, multipart upload, and range coalescing.

use std::collections::HashMap;
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// CloudError
// ─────────────────────────────────────────────────────────────────────────────

/// Errors produced by the cloud I/O layer.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CloudError {
    /// The URL string could not be parsed.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// The URL scheme is not supported.
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),

    /// Credentials are required but were not provided.
    #[error("missing credentials")]
    MissingCredentials,

    /// The supplied credentials are malformed or expired.
    #[error("invalid credentials: {0}")]
    InvalidCredentials(String),

    /// An error occurred while generating a presigned URL.
    #[error("presign error: {0}")]
    PresignError(String),

    /// The requested byte range exceeds the object size.
    #[error("range out of bounds: [{start}, {end}) vs size {size}")]
    RangeOutOfBounds {
        /// Start offset of the requested range.
        start: u64,
        /// End offset (exclusive) of the requested range.
        end: u64,
        /// Actual size of the object.
        size: u64,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudScheme / ObjectUrl
// ─────────────────────────────────────────────────────────────────────────────

/// Supported cloud URL schemes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudScheme {
    /// Amazon S3 (`s3://`)
    S3,
    /// Google Cloud Storage (`gs://`)
    Gs,
    /// Azure Blob Storage (`az://` or `abfs://`)
    Az,
    /// Plain HTTP (`http://`)
    Http,
    /// HTTPS (`https://`)
    Https,
}

/// A parsed cloud object URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectUrl {
    /// Scheme of the URL.
    pub scheme: CloudScheme,
    /// Bucket or container name.
    pub bucket: String,
    /// Object key / path within the bucket.
    pub key: String,
    /// AWS / GCS region (if present in the URL).
    pub region: Option<String>,
    /// Custom endpoint override.
    pub endpoint: Option<String>,
}

impl ObjectUrl {
    /// Parse a cloud URL string into an [`ObjectUrl`].
    ///
    /// Supported forms:
    /// - `s3://bucket/key`
    /// - `gs://bucket/key`
    /// - `az://container/blob`
    /// - `abfs://container@account.dfs.core.windows.net/path`
    /// - `http://host/path`
    /// - `https://host/path`
    pub fn parse(url: &str) -> Result<Self, CloudError> {
        let (scheme_str, rest) = url
            .split_once("://")
            .ok_or_else(|| CloudError::InvalidUrl(format!("no scheme separator in '{url}'")))?;

        let scheme = match scheme_str.to_ascii_lowercase().as_str() {
            "s3" => CloudScheme::S3,
            "gs" => CloudScheme::Gs,
            "az" | "abfs" => CloudScheme::Az,
            "http" => CloudScheme::Http,
            "https" => CloudScheme::Https,
            other => return Err(CloudError::UnsupportedScheme(other.to_owned())),
        };

        match &scheme {
            CloudScheme::Http | CloudScheme::Https => {
                // For http(s) the "bucket" is the host and the "key" is the path.
                let (host, path) = if let Some(idx) = rest.find('/') {
                    (&rest[..idx], &rest[idx + 1..])
                } else {
                    (rest, "")
                };
                if host.is_empty() {
                    return Err(CloudError::InvalidUrl(format!("no host in '{url}'")));
                }
                Ok(ObjectUrl {
                    scheme,
                    bucket: host.to_owned(),
                    key: path.to_owned(),
                    region: None,
                    endpoint: None,
                })
            }
            _ => {
                // s3://, gs://, az://  →  bucket/key
                let (bucket, key) = if let Some(idx) = rest.find('/') {
                    (&rest[..idx], &rest[idx + 1..])
                } else {
                    (rest, "")
                };
                if bucket.is_empty() {
                    return Err(CloudError::InvalidUrl(format!("no bucket in '{url}'")));
                }
                Ok(ObjectUrl {
                    scheme,
                    bucket: bucket.to_owned(),
                    key: key.to_owned(),
                    region: None,
                    endpoint: None,
                })
            }
        }
    }

    /// Convert the cloud URL to an HTTPS URL.
    ///
    /// - S3: `https://{bucket}.s3.{region}.amazonaws.com/{key}`
    /// - GCS: `https://storage.googleapis.com/{bucket}/{key}`
    /// - Azure: `https://{account}.blob.core.windows.net/{container}/{key}`
    /// - HTTP/HTTPS: return as-is (upgraded to https for http)
    pub fn to_https_url(&self, endpoint_override: Option<&str>) -> String {
        if let Some(ep) = endpoint_override {
            let base = ep.trim_end_matches('/');
            return format!("{base}/{}/{}", self.bucket, self.key);
        }
        match &self.scheme {
            CloudScheme::S3 => {
                let region = self.region.as_deref().unwrap_or("us-east-1");
                format!(
                    "https://{}.s3.{}.amazonaws.com/{}",
                    self.bucket, region, self.key
                )
            }
            CloudScheme::Gs => {
                format!(
                    "https://storage.googleapis.com/{}/{}",
                    self.bucket, self.key
                )
            }
            CloudScheme::Az => {
                // bucket is treated as "account/container"
                // We store just "container" in bucket; account may be in endpoint.
                format!("https://{}.blob.core.windows.net/{}", self.bucket, self.key)
            }
            CloudScheme::Http => {
                format!("https://{}/{}", self.bucket, self.key)
            }
            CloudScheme::Https => {
                format!("https://{}/{}", self.bucket, self.key)
            }
        }
    }

    /// Build the canonical host string used when signing requests.
    pub fn signing_host(&self) -> String {
        match &self.scheme {
            CloudScheme::S3 => {
                let region = self.region.as_deref().unwrap_or("us-east-1");
                format!("{}.s3.{}.amazonaws.com", self.bucket, region)
            }
            CloudScheme::Gs => "storage.googleapis.com".to_owned(),
            CloudScheme::Az => {
                format!("{}.blob.core.windows.net", self.bucket)
            }
            CloudScheme::Http | CloudScheme::Https => self.bucket.clone(),
        }
    }

    /// Return the URL path component used for signing.
    pub fn signing_path(&self) -> String {
        let key = if self.key.starts_with('/') {
            self.key.clone()
        } else {
            format!("/{}", self.key)
        };
        match &self.scheme {
            CloudScheme::Gs | CloudScheme::Az => {
                format!("/{}{}", self.bucket, key)
            }
            _ => key,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ByteRangeRequest
// ─────────────────────────────────────────────────────────────────────────────

/// A request for a specific byte range of a cloud object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteRangeRequest {
    /// The object URL to read from.
    pub url: ObjectUrl,
    /// Byte range (exclusive end).
    pub range: std::ops::Range<u64>,
}

impl ByteRangeRequest {
    /// Create a new byte-range request.
    pub fn new(url: ObjectUrl, start: u64, end: u64) -> Self {
        ByteRangeRequest {
            url,
            range: start..end,
        }
    }

    /// Return the HTTP `Range` header value: `bytes=start-end_inclusive`.
    pub fn to_http_range_header(&self) -> String {
        let end_inclusive = self.range.end.saturating_sub(1);
        format!("bytes={}-{}", self.range.start, end_inclusive)
    }

    /// Number of bytes in this range.
    pub fn length(&self) -> u64 {
        self.range.end.saturating_sub(self.range.start)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ObjectMetadata
// ─────────────────────────────────────────────────────────────────────────────

/// Metadata for a cloud object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMetadata {
    /// The object URL.
    pub url: ObjectUrl,
    /// Total size in bytes.
    pub size: u64,
    /// MIME content type, if available.
    pub content_type: Option<String>,
    /// ETag string, if available.
    pub etag: Option<String>,
    /// Last-modified Unix timestamp, if available.
    pub last_modified: Option<u64>,
    /// User-defined metadata key/value pairs.
    pub user_metadata: HashMap<String, String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudCredentials
// ─────────────────────────────────────────────────────────────────────────────

/// Authentication credentials for cloud object storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudCredentials {
    /// No authentication (public buckets).
    Anonymous,
    /// AWS / GCS access-key credentials.
    AccessKey {
        /// Access key ID.
        access_key_id: String,
        /// Secret access key.
        secret_access_key: String,
        /// Optional STS session token.
        session_token: Option<String>,
    },
    /// GCS service-account JSON file path.
    ServiceAccountFile {
        /// Path to the service account JSON file.
        path: String,
    },
    /// Azure Shared Key authentication.
    AzureSharedKey {
        /// Azure storage account name.
        account_name: String,
        /// Base64-encoded account key.
        account_key: String,
    },
    /// Azure SAS token.
    SasToken {
        /// The SAS token string.
        token: String,
    },
    /// Generic OAuth2 bearer token.
    Bearer {
        /// The bearer token string.
        token: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// PresignedUrlConfig / HttpMethod
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP verb used in a presigned URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    /// GET request.
    Get,
    /// PUT request.
    Put,
    /// DELETE request.
    Delete,
    /// HEAD request.
    Head,
}

impl HttpMethod {
    fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
        }
    }
}

/// Configuration for presigned URL generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresignedUrlConfig {
    /// How many seconds the URL should remain valid.
    pub expires_in_secs: u64,
    /// HTTP method the presigned URL will allow.
    pub method: HttpMethod,
    /// Optional content-type constraint.
    pub content_type: Option<String>,
}

impl PresignedUrlConfig {
    /// Create a GET presigned URL configuration.
    pub fn get(expires_in_secs: u64) -> Self {
        PresignedUrlConfig {
            expires_in_secs,
            method: HttpMethod::Get,
            content_type: None,
        }
    }

    /// Create a PUT presigned URL configuration with a content type.
    pub fn put(expires_in_secs: u64, content_type: impl Into<String>) -> Self {
        PresignedUrlConfig {
            expires_in_secs,
            method: HttpMethod::Put,
            content_type: Some(content_type.into()),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust SHA-256 and HMAC-SHA256
// ─────────────────────────────────────────────────────────────────────────────

/// SHA-256 round constants.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial hash values (first 32 bits of the fractional parts of the square roots of the first 8 primes).
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Compute SHA-256 of `data`.  Pure-Rust implementation — no external crates.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = H0;

    // Pre-processing: add padding
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg: Vec<u8> = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate().take(16) {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Compute HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;

    // Derive the actual HMAC key (hash if longer than block size)
    let mut k = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = sha256(key);
        k[..32].copy_from_slice(&hashed);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0u8; BLOCK_SIZE];
    let mut opad = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] = k[i] ^ 0x36;
        opad[i] = k[i] ^ 0x5c;
    }

    let mut inner = ipad.to_vec();
    inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);

    let mut outer = opad.to_vec();
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// Compute HMAC-SHA256 and return the result as a lowercase hex string.
pub fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex_encode(&hmac_sha256(key, data))
}

/// Encode a byte slice as a lowercase hexadecimal string.
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}

// ─────────────────────────────────────────────────────────────────────────────
// PresignedUrlGenerator
// ─────────────────────────────────────────────────────────────────────────────

/// Generates presigned URLs using AWS SigV4 / GCS v4 signing.
///
/// The implementation is entirely pure Rust — no external cryptographic crates.
pub struct PresignedUrlGenerator {
    /// The credentials used for signing.
    pub credentials: CloudCredentials,
    /// AWS / GCS region.
    pub region: String,
}

impl PresignedUrlGenerator {
    /// Create a new generator.
    pub fn new(credentials: CloudCredentials, region: impl Into<String>) -> Self {
        PresignedUrlGenerator {
            credentials,
            region: region.into(),
        }
    }

    /// Format a Unix timestamp as an AWS date string (`YYYYMMDD`).
    fn format_date(ts: u64) -> String {
        // Days since Unix epoch
        let days = ts / 86_400;
        let (year, month, day) = days_to_ymd(days);
        format!("{year:04}{month:02}{day:02}")
    }

    /// Format a Unix timestamp as an AWS datetime string (`YYYYMMDDTHHmmSSZ`).
    fn format_datetime(ts: u64) -> String {
        let date = Self::format_date(ts);
        let rem = ts % 86_400;
        let h = rem / 3600;
        let m = (rem % 3600) / 60;
        let s = rem % 60;
        format!("{date}T{h:02}{m:02}{s:02}Z")
    }

    /// Derive the AWS SigV4 signing key.
    fn derive_signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
        let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, service.as_bytes());
        hmac_sha256(&k_service, b"aws4_request")
    }

    /// Percent-encode a string using AWS URI encoding rules.
    fn uri_encode(s: &str, encode_slash: bool) -> String {
        let mut out = String::with_capacity(s.len());
        for byte in s.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                b'/' if !encode_slash => out.push('/'),
                other => {
                    use std::fmt::Write;
                    let _ = write!(out, "%{other:02X}");
                }
            }
        }
        out
    }

    /// Build a sorted canonical query string from key-value pairs.
    pub fn canonical_query_string(&self, params: &[(String, String)]) -> String {
        let mut sorted: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (Self::uri_encode(k, true), Self::uri_encode(v, true)))
            .collect();
        sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
        sorted
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Generate an AWS SigV4 presigned URL.
    pub fn generate_s3(
        &self,
        url: &ObjectUrl,
        config: &PresignedUrlConfig,
        timestamp_unix: u64,
    ) -> Result<String, CloudError> {
        let (access_key_id, secret_access_key) = match &self.credentials {
            CloudCredentials::AccessKey {
                access_key_id,
                secret_access_key,
                ..
            } => (access_key_id.as_str(), secret_access_key.as_str()),
            _ => return Err(CloudError::MissingCredentials),
        };

        let service = "s3";
        let date = Self::format_date(timestamp_unix);
        let datetime = Self::format_datetime(timestamp_unix);

        let credential = format!(
            "{access_key_id}/{date}/{}/{service}/aws4_request",
            self.region
        );

        let host = url.signing_host();
        let path = Self::uri_encode(&url.key, false);
        let canonical_path = format!("/{path}");

        let mut query_params: Vec<(String, String)> = vec![
            ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
            ("X-Amz-Credential".to_owned(), credential.clone()),
            ("X-Amz-Date".to_owned(), datetime.clone()),
            (
                "X-Amz-Expires".to_owned(),
                config.expires_in_secs.to_string(),
            ),
            ("X-Amz-SignedHeaders".to_owned(), "host".to_owned()),
        ];

        if let CloudCredentials::AccessKey {
            session_token: Some(tok),
            ..
        } = &self.credentials
        {
            query_params.push(("X-Amz-Security-Token".to_owned(), tok.clone()));
        }

        let canonical_query = self.canonical_query_string(&query_params);

        let canonical_headers = format!("host:{host}\n");
        let signed_headers = "host";

        // For presigned URLs, the payload hash is "UNSIGNED-PAYLOAD"
        let payload_hash = "UNSIGNED-PAYLOAD";

        let canonical_request = format!(
            "{method}\n{path}\n{query}\n{headers}\n{signed}\n{payload}",
            method = config.method.as_str(),
            path = canonical_path,
            query = canonical_query,
            headers = canonical_headers,
            signed = signed_headers,
            payload = payload_hash,
        );

        let scope = format!("{date}/{}/{service}/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{datetime}\n{scope}\n{hash}",
            hash = hex_encode(&sha256(canonical_request.as_bytes())),
        );

        let signing_key = Self::derive_signing_key(secret_access_key, &date, &self.region, service);
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

        let mut final_params = query_params;
        final_params.push(("X-Amz-Signature".to_owned(), signature));
        let final_query = self.canonical_query_string(&final_params);

        let base_url = format!("https://{host}{canonical_path}");
        Ok(format!("{base_url}?{final_query}"))
    }

    /// Generate a GCS v4 presigned URL (same signing algorithm as S3 SigV4).
    pub fn generate_gcs(
        &self,
        url: &ObjectUrl,
        config: &PresignedUrlConfig,
        timestamp_unix: u64,
    ) -> Result<String, CloudError> {
        let (access_key_id, secret_access_key) = match &self.credentials {
            CloudCredentials::AccessKey {
                access_key_id,
                secret_access_key,
                ..
            } => (access_key_id.as_str(), secret_access_key.as_str()),
            CloudCredentials::ServiceAccountFile { path } => {
                // In a real implementation we'd parse the JSON; here we use the path
                // as a stand-in identifier so the signature is deterministic in tests.
                return Err(CloudError::PresignError(format!(
                    "service account file signing requires JSON parsing (path: {path})"
                )));
            }
            _ => return Err(CloudError::MissingCredentials),
        };

        let service = "storage";
        let date = Self::format_date(timestamp_unix);
        let datetime = Self::format_datetime(timestamp_unix);
        let host = "storage.googleapis.com";
        let canonical_path = format!("/{}/{}", url.bucket, Self::uri_encode(&url.key, false));

        let credential = format!(
            "{access_key_id}/{date}/{}/{service}/goog4_request",
            self.region
        );

        let query_params: Vec<(String, String)> = vec![
            (
                "X-Goog-Algorithm".to_owned(),
                "GOOG4-HMAC-SHA256".to_owned(),
            ),
            ("X-Goog-Credential".to_owned(), credential.clone()),
            ("X-Goog-Date".to_owned(), datetime.clone()),
            (
                "X-Goog-Expires".to_owned(),
                config.expires_in_secs.to_string(),
            ),
            ("X-Goog-SignedHeaders".to_owned(), "host".to_owned()),
        ];

        let canonical_query = self.canonical_query_string(&query_params);
        let canonical_headers = format!("host:{host}\n");
        let signed_headers = "host";
        let payload_hash = "UNSIGNED-PAYLOAD";

        let canonical_request = format!(
            "{method}\n{path}\n{query}\n{headers}\n{signed}\n{payload}",
            method = config.method.as_str(),
            path = canonical_path,
            query = canonical_query,
            headers = canonical_headers,
            signed = signed_headers,
            payload = payload_hash,
        );

        let scope = format!("{date}/{}/{service}/goog4_request", self.region);
        let string_to_sign = format!(
            "GOOG4-HMAC-SHA256\n{datetime}\n{scope}\n{hash}",
            hash = hex_encode(&sha256(canonical_request.as_bytes())),
        );

        let signing_key = Self::derive_signing_key(secret_access_key, &date, &self.region, service);
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

        let mut final_params = query_params;
        final_params.push(("X-Goog-Signature".to_owned(), signature));
        let final_query = self.canonical_query_string(&final_params);

        Ok(format!("https://{host}{canonical_path}?{final_query}"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Date arithmetic helper
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a count of days since the Unix epoch (1970-01-01) to (year, month, day).
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Use the civil calendar algorithm (proleptic Gregorian).
    // Reference: http://howardhinnant.github.io/date_algorithms.html
    let z = days as i64 + 719_468;
    let era: i64 = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_adj = if m <= 2 { y + 1 } else { y };
    (y_adj as u32, m as u32, d as u32)
}

// ─────────────────────────────────────────────────────────────────────────────
// MultipartUploadState
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks the state of an S3 multipart upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedPart {
    /// 1-based part number.
    pub part_number: u16,
    /// ETag returned by the server for this part.
    pub etag: String,
    /// Size of this part in bytes.
    pub size: u64,
}

/// State machine for an in-progress multipart upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartUploadState {
    /// The upload ID assigned by the cloud provider.
    pub upload_id: String,
    /// The target object URL.
    pub url: ObjectUrl,
    /// Parts that have been successfully uploaded.
    pub parts: Vec<CompletedPart>,
    /// Nominal part size in bytes.
    pub part_size: u64,
}

impl MultipartUploadState {
    /// Create a new multipart upload tracker.
    pub fn new(upload_id: impl Into<String>, url: ObjectUrl, part_size: u64) -> Self {
        MultipartUploadState {
            upload_id: upload_id.into(),
            url,
            parts: Vec::new(),
            part_size,
        }
    }

    /// Record a completed part.
    pub fn add_part(&mut self, part_number: u16, etag: impl Into<String>, size: u64) {
        self.parts.push(CompletedPart {
            part_number,
            etag: etag.into(),
            size,
        });
    }

    /// Total uploaded bytes across all parts.
    pub fn total_size(&self) -> u64 {
        self.parts.iter().map(|p| p.size).sum()
    }

    /// Number of parts recorded.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    /// Build the S3 `CompleteMultipartUpload` XML body.
    ///
    /// Parts are emitted in ascending part-number order.
    pub fn to_xml(&self) -> String {
        let mut sorted = self.parts.clone();
        sorted.sort_by_key(|p| p.part_number);

        let mut xml = String::from("<CompleteMultipartUpload>\n");
        for part in &sorted {
            xml.push_str("  <Part>\n");
            xml.push_str(&format!(
                "    <PartNumber>{}</PartNumber>\n",
                part.part_number
            ));
            xml.push_str(&format!("    <ETag>{}</ETag>\n", part.etag));
            xml.push_str("  </Part>\n");
        }
        xml.push_str("</CompleteMultipartUpload>");
        xml
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudRangeCoalescer
// ─────────────────────────────────────────────────────────────────────────────

/// Merges nearby byte-range requests into larger ones to reduce round-trip
/// overhead when reading from cloud object storage.
pub struct CloudRangeCoalescer {
    /// Maximum gap between two ranges that should still be merged.
    pub max_gap_bytes: u64,
    /// Maximum total size of a coalesced request.
    pub max_request_size: u64,
    /// Minimum size of a request (avoid sending tiny reads).
    pub min_request_size: u64,
}

impl Default for CloudRangeCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudRangeCoalescer {
    /// Create a coalescer with sensible defaults for cloud storage:
    /// - `max_gap_bytes` = 512 KiB
    /// - `max_request_size` = 8 MiB
    /// - `min_request_size` = 64 KiB
    pub fn new() -> Self {
        CloudRangeCoalescer {
            max_gap_bytes: 512 * 1024,
            max_request_size: 8 * 1024 * 1024,
            min_request_size: 64 * 1024,
        }
    }

    /// Coalesce a list of byte-range requests.
    ///
    /// All input requests must target the **same** URL.  Requests are sorted by
    /// start offset and then merged when:
    /// - the gap between consecutive ranges is ≤ `max_gap_bytes`, **and**
    /// - the resulting coalesced range would not exceed `max_request_size`.
    pub fn coalesce(&self, mut ranges: Vec<ByteRangeRequest>) -> Vec<ByteRangeRequest> {
        if ranges.is_empty() {
            return ranges;
        }

        // Sort by start offset
        ranges.sort_by_key(|r| r.range.start);

        let url = ranges[0].url.clone();
        let mut coalesced: Vec<ByteRangeRequest> = Vec::new();
        let mut current_start = ranges[0].range.start;
        let mut current_end = ranges[0].range.end;

        for req in ranges.into_iter().skip(1) {
            let gap = req.range.start.saturating_sub(current_end);
            let new_end = req.range.end.max(current_end);
            let new_size = new_end - current_start;

            if gap <= self.max_gap_bytes && new_size <= self.max_request_size {
                // Merge
                current_end = new_end;
            } else {
                coalesced.push(ByteRangeRequest::new(
                    url.clone(),
                    current_start,
                    current_end,
                ));
                current_start = req.range.start;
                current_end = req.range.end;
            }
        }
        coalesced.push(ByteRangeRequest::new(url, current_start, current_end));
        coalesced
    }

    /// Extract a sub-range from a coalesced response buffer.
    ///
    /// `coalesced_start` is the byte offset at which `coalesced_data` begins.
    /// `sub_range` is the desired slice within the full object.
    ///
    /// # Panics
    ///
    /// Panics if `sub_range` does not fall within the coalesced data window.
    pub fn slice_response<'a>(
        coalesced_data: &'a [u8],
        coalesced_start: u64,
        sub_range: &std::ops::Range<u64>,
    ) -> &'a [u8] {
        let offset = (sub_range.start - coalesced_start) as usize;
        let len = (sub_range.end - sub_range.start) as usize;
        &coalesced_data[offset..offset + len]
    }
}
