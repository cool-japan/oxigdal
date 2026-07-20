//! HTTP transport for cloud object-store operations.
//!
//! This module provides a real HTTP client layer that uses the parsed
//! [`ObjectUrl`] and [`CloudCredentials`] types to perform HTTP range GETs,
//! full-object fetches, and HEAD requests against cloud storage endpoints.
//!
//! Enabled only when the `cloud-http` feature is active.

use bytes::Bytes;

use super::object_store::{CloudCredentials, CloudError, ObjectMetadata, ObjectUrl};
use super::retry::{RetryPolicy, RetryState};

// ─────────────────────────────────────────────────────────────────────────────
// HttpObjectStore
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP transport for cloud object-store operations.
///
/// Wraps a [`reqwest::Client`] with a [`RetryPolicy`] that retries on
/// transient server-side errors (429 Too Many Requests, 503 Service
/// Unavailable).
pub struct HttpObjectStore {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl Default for HttpObjectStore {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpObjectStore {
    /// Creates a new store with the default retry policy:
    /// 3 attempts, 100 ms initial delay, ×2 exponential back-off, 10 % jitter.
    pub fn new() -> Self {
        HttpObjectStore {
            client: reqwest::Client::new(),
            retry: RetryPolicy::new(),
        }
    }

    /// Creates a store with a custom retry policy.
    pub fn with_retry(retry: RetryPolicy) -> Self {
        HttpObjectStore {
            client: reqwest::Client::new(),
            retry,
        }
    }

    /// Apply authentication credentials to a request builder.
    ///
    /// - [`CloudCredentials::Anonymous`] — no auth header added.
    /// - [`CloudCredentials::Bearer`] — `Authorization: Bearer <token>`.
    /// - [`CloudCredentials::AccessKey`] — adds `X-Access-Key: <key_id>`.
    ///   Full SigV4 signing is available through [`PresignedUrlGenerator`]; this
    ///   path is intentionally lightweight and meant for environments that
    ///   already have pre-authenticated URLs or rely on a signing proxy.
    /// - [`CloudCredentials::SasToken`] — appends the SAS token as a query
    ///   parameter `?{token}` via the `Authorization` header bypass path.
    ///   Actual SAS attachment is done at the URL level; here we skip it.
    /// - Other variants — return [`CloudError::UnsupportedCredentials`].
    fn apply_credentials(
        builder: reqwest::RequestBuilder,
        credentials: &CloudCredentials,
        url_str: &str,
    ) -> Result<reqwest::RequestBuilder, CloudError> {
        match credentials {
            CloudCredentials::Anonymous => Ok(builder),

            CloudCredentials::Bearer { token } => {
                Ok(builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}")))
            }

            CloudCredentials::AccessKey { access_key_id, .. } => {
                // Full SigV4/GCS v4 signing is available via PresignedUrlGenerator.
                // For direct HTTP transport, we pass the key ID as an identification
                // header. Signing is a follow-up integration task.
                Ok(builder.header("X-Access-Key", access_key_id.as_str()))
            }

            CloudCredentials::SasToken { .. } => {
                // Azure SAS tokens are typically embedded in the URL; the URL
                // returned by to_https_url() already encodes the SAS for az://.
                // No additional auth header is needed here.
                Ok(builder)
            }

            CloudCredentials::AzureSharedKey { account_name, .. } => {
                // Full HMAC-SHA256 Azure Shared Key signing is a follow-up.
                // Return an unsupported error so callers know explicit action is
                // required rather than silently sending unsigned requests.
                Err(CloudError::UnsupportedCredentials(format!(
                    "Azure Shared Key signing is not yet implemented for account '{account_name}'. \
                     Use SasToken, Bearer, or Anonymous credentials, or generate a presigned URL."
                )))
            }

            CloudCredentials::ServiceAccountFile { path } => {
                Err(CloudError::UnsupportedCredentials(format!(
                    "Service account JSON file credentials ('{path}') are not yet supported for \
                     direct HTTP transport. Use Bearer token credentials obtained from the GCS \
                     OAuth2 token endpoint, or generate a presigned URL via PresignedUrlGenerator."
                )))
            }

            #[allow(unreachable_patterns)]
            _ => Err(CloudError::UnsupportedCredentials(format!(
                "unsupported credential type for URL: {url_str}"
            ))),
        }
    }

    /// Determines whether the given HTTP status code is a transient error
    /// that is worth retrying.
    fn is_retryable_status(status: u16) -> bool {
        // 429: Too Many Requests — rate limit; back off and retry.
        // 503: Service Unavailable — transient infra issue.
        matches!(status, 429 | 503)
    }

    /// Build the request URL string from an [`ObjectUrl`].
    ///
    /// For `http://` and `https://` scheme URLs the original scheme is
    /// preserved so that plain-HTTP test servers (and HTTP-only endpoints)
    /// are not silently upgraded to TLS.  For cloud-native schemes (S3, GCS,
    /// Azure) the canonical HTTPS endpoint is always used.
    fn build_url(url: &ObjectUrl) -> String {
        use super::object_store::CloudScheme;
        match &url.scheme {
            CloudScheme::Http => {
                // Preserve plain HTTP — do not upgrade to HTTPS.
                if let Some(ep) = url.endpoint.as_deref() {
                    let base = ep.trim_end_matches('/');
                    format!("{base}/{}/{}", url.bucket, url.key)
                } else {
                    // bucket = host for Http/Https ObjectUrls
                    let key = url.key.trim_start_matches('/');
                    format!("http://{}/{}", url.bucket, key)
                }
            }
            _ => url.to_https_url(url.endpoint.as_deref()),
        }
    }

    /// Fetches a byte range `[start, end_inclusive]` from the given URL.
    ///
    /// Issues an HTTP GET with `Range: bytes=start-end_inclusive`.
    /// A successful response is expected to carry status 206 Partial Content
    /// (or 200 OK for servers that ignore Range headers); both are accepted.
    ///
    /// Retries on 429 and 503 up to `retry.max_attempts` times with
    /// exponential back-off.
    pub async fn get_range(
        &self,
        url: &ObjectUrl,
        start: u64,
        end_inclusive: u64,
        credentials: &CloudCredentials,
    ) -> Result<Bytes, CloudError> {
        let url_str = Self::build_url(url);
        let range_header = format!("bytes={start}-{end_inclusive}");

        let mut state = RetryState::new(self.retry.clone());

        loop {
            let builder = self
                .client
                .get(&url_str)
                .header(reqwest::header::RANGE, &range_header);

            let builder = Self::apply_credentials(builder, credentials, &url_str)?;

            let response = builder
                .send()
                .await
                .map_err(|e| CloudError::TransportError(e.to_string()))?;

            let status = response.status().as_u16();

            if response.status().is_success() {
                let body = response
                    .bytes()
                    .await
                    .map_err(|e| CloudError::TransportError(e.to_string()))?;
                return Ok(body);
            }

            if Self::is_retryable_status(status) {
                if let Some(delay) = state.next_delay() {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    continue;
                }
                return Err(CloudError::RetryExhausted {
                    attempts: self.retry.max_attempts,
                });
            }

            return Err(CloudError::HttpError {
                status,
                url: url_str,
            });
        }
    }

    /// Fetches the entire object at the given URL.
    ///
    /// Issues an HTTP GET without a Range header.
    /// Retries on 429 and 503.
    pub async fn get_object(
        &self,
        url: &ObjectUrl,
        credentials: &CloudCredentials,
    ) -> Result<Bytes, CloudError> {
        let url_str = Self::build_url(url);
        let mut state = RetryState::new(self.retry.clone());

        loop {
            let builder = self.client.get(&url_str);
            let builder = Self::apply_credentials(builder, credentials, &url_str)?;

            let response = builder
                .send()
                .await
                .map_err(|e| CloudError::TransportError(e.to_string()))?;

            let status = response.status().as_u16();

            if response.status().is_success() {
                let body = response
                    .bytes()
                    .await
                    .map_err(|e| CloudError::TransportError(e.to_string()))?;
                return Ok(body);
            }

            if Self::is_retryable_status(status) {
                if let Some(delay) = state.next_delay() {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    continue;
                }
                return Err(CloudError::RetryExhausted {
                    attempts: self.retry.max_attempts,
                });
            }

            return Err(CloudError::HttpError {
                status,
                url: url_str,
            });
        }
    }

    /// Retrieves object metadata via HTTP HEAD.
    ///
    /// Extracts:
    /// - `size` from `Content-Length`
    /// - `etag` from `ETag`
    /// - `content_type` from `Content-Type`
    /// - `last_modified` as a Unix timestamp parsed from `Last-Modified`
    ///
    /// Retries on 429 and 503.
    pub async fn head(
        &self,
        url: &ObjectUrl,
        credentials: &CloudCredentials,
    ) -> Result<ObjectMetadata, CloudError> {
        let url_str = Self::build_url(url);
        let mut state = RetryState::new(self.retry.clone());

        loop {
            let builder = self.client.head(&url_str);
            let builder = Self::apply_credentials(builder, credentials, &url_str)?;

            let response = builder
                .send()
                .await
                .map_err(|e| CloudError::TransportError(e.to_string()))?;

            let status = response.status().as_u16();

            if response.status().is_success() {
                let headers = response.headers();

                // Content-Length → size (required; default to 0 if absent)
                let size: u64 = headers
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);

                // ETag — optional
                let etag: Option<String> = headers
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_owned());

                // Content-Type — optional
                let content_type: Option<String> = headers
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_owned());

                // Last-Modified — parse HTTP date to Unix timestamp.
                // The header follows RFC 7231, e.g. "Mon, 12 Jan 2026 00:00:00 GMT".
                // We attempt a best-effort parse; failures are silently ignored.
                let last_modified: Option<u64> =
                    parse_http_date_to_unix(headers.get(reqwest::header::LAST_MODIFIED));

                return Ok(ObjectMetadata {
                    url: url.clone(),
                    size,
                    content_type,
                    etag,
                    last_modified,
                    user_metadata: std::collections::HashMap::new(),
                });
            }

            if Self::is_retryable_status(status) {
                if let Some(delay) = state.next_delay() {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    continue;
                }
                return Err(CloudError::RetryExhausted {
                    attempts: self.retry.max_attempts,
                });
            }

            return Err(CloudError::HttpError {
                status,
                url: url_str,
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP date parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an HTTP `Last-Modified` header value into a Unix timestamp.
///
/// Accepts RFC 7231 / RFC 1123 format:
/// `"Mon, 12 Jan 2026 00:00:00 GMT"`
///
/// Returns `None` on any parse failure to remain robust against
/// non-standard server responses.
fn parse_http_date_to_unix(header: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let raw = header?.to_str().ok()?;
    parse_rfc1123_date(raw)
}

/// Parse an RFC 1123 HTTP date string into a Unix timestamp (seconds since
/// 1970-01-01 00:00:00 UTC).
///
/// Format: `"wday, DD Mon YYYY HH:MM:SS GMT"`
///
/// Pure-Rust implementation with no external date/time crate dependency.
fn parse_rfc1123_date(s: &str) -> Option<u64> {
    // Strip optional leading weekday+comma: "Mon, 12 Jan 2026 00:00:00 GMT"
    let s = if let Some(idx) = s.find(',') {
        s[idx + 1..].trim_start()
    } else {
        s.trim_start()
    };

    // Expected remainder: "12 Jan 2026 00:00:00 GMT"
    let parts: Vec<&str> = s.splitn(5, ' ').collect();
    if parts.len() < 4 {
        return None;
    }

    let day: u64 = parts[0].parse().ok()?;
    let month: u64 = month_name_to_number(parts[1])?;
    let year: u64 = parts[2].parse().ok()?;

    // Time part: "HH:MM:SS"
    let time_parts: Vec<&str> = parts[3].splitn(3, ':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let hour: u64 = time_parts[0].parse().ok()?;
    let minute: u64 = time_parts[1].parse().ok()?;
    let second: u64 = time_parts[2].parse().ok()?;

    // Compute days since epoch using the Gregorian calendar proleptic formula.
    // We use Julian Day Number arithmetic to convert calendar date → Unix epoch.
    if year < 1970 {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    let unix_secs = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;

    Some(unix_secs)
}

/// Convert month abbreviation to 1-based month number.
fn month_name_to_number(name: &str) -> Option<u64> {
    match name {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

/// Compute the number of days from 1970-01-01 (Unix epoch) to the given
/// Gregorian calendar date.
///
/// Uses the algorithm from Howard Hinnant's `date.h` (public domain):
/// <https://howardhinnant.github.io/date_algorithms.html#days_from_civil>
fn days_from_civil(y: u64, m: u64, d: u64) -> Option<u64> {
    // Shift year so March is month 1 (avoids leap-year boundary on Feb/Mar)
    let (y_adj, m_adj) = if m <= 2 {
        (y.checked_sub(1)?, m + 9)
    } else {
        (y, m - 3)
    };

    let era = y_adj / 400;
    let yoe = y_adj % 400; // year of era [0, 399]
    let doy = (153 * m_adj + 2) / 5 + d - 1; // day of year [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // day of era [0, 146096]
    let jdn = era * 146_097 + doe; // Julian Day from epoch 0000-03-01

    // Adjust to Unix epoch (1970-01-01 = JDN offset 719468 from 0000-03-01)
    const UNIX_EPOCH_JDN: u64 = 719_468;
    jdn.checked_sub(UNIX_EPOCH_JDN)
}
