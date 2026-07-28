//! Real HTTP(S) health-check probes.
//!
//! These probes perform genuine network requests: a raw HTTP/1.1 request over a TCP socket
//! for `http://` URLs, and the same request over a Pure-Rust TLS session (rustls driven by
//! the OxiTLS RustCrypto provider — no `ring`/C/ASM) for `https://` URLs. The status line,
//! headers and (optionally) body of the real response are parsed and returned so the caller
//! can decide health from actual server behaviour instead of a hardcoded success.

use crate::error::{GatewayError, Result};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Maximum number of response bytes read for a probe (headers + sampled body).
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Parsed pieces of a probe target URL.
struct Target {
    tls: bool,
    host: String,
    port: u16,
    /// Path (already combined by the caller); defaults to `/`.
    path: String,
}

impl Target {
    fn parse(base_url: &str, path: &str) -> Result<Self> {
        let parsed = url::Url::parse(base_url)
            .map_err(|e| GatewayError::HttpError(format!("invalid probe URL '{base_url}': {e}")))?;

        let tls = match parsed.scheme() {
            "http" => false,
            "https" => true,
            other => {
                return Err(GatewayError::HttpError(format!(
                    "unsupported scheme '{other}' for HTTP probe"
                )));
            }
        };

        let host = parsed
            .host_str()
            .ok_or_else(|| GatewayError::HttpError(format!("probe URL '{base_url}' has no host")))?
            .to_string();

        let port = parsed.port().unwrap_or(if tls { 443 } else { 80 });

        // The request-target path. Prefer the explicit health path; fall back to the URL's
        // own path when the caller passes an empty string.
        let request_path = if path.is_empty() {
            let p = parsed.path();
            if p.is_empty() {
                "/".to_string()
            } else {
                p.to_string()
            }
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        Ok(Self {
            tls,
            host,
            port,
            path: request_path,
        })
    }
}

/// Outcome of a successful (network-level) HTTP probe.
pub struct ProbeResponse {
    /// HTTP status code from the response status line.
    pub status: u16,
    /// Response body (possibly truncated to `MAX_RESPONSE_BYTES`).
    pub body: String,
}

/// Performs a real HTTP/1.1 `GET` against `base_url` + `path`, honoring `timeout`, custom
/// request `headers`, and (optionally) a single redirect hop when `follow_redirects` is set.
///
/// Returns [`ProbeResponse`] on any complete HTTP response (including 4xx/5xx — the caller
/// decides what counts as healthy), or an error on connect/TLS/IO/timeout failures.
pub async fn http_probe(
    base_url: &str,
    path: &str,
    timeout: Duration,
    headers: &[(String, String)],
    follow_redirects: bool,
) -> Result<ProbeResponse> {
    // At most one redirect hop to keep health checks bounded and avoid loops.
    let mut current_url = base_url.to_string();
    let mut current_path = path.to_string();
    let mut redirects_left = if follow_redirects { 1u8 } else { 0u8 };

    loop {
        let target = Target::parse(&current_url, &current_path)?;

        let response = tokio::time::timeout(timeout, probe_once(&target, headers))
            .await
            .map_err(|_| GatewayError::Timeout("health check timed out".to_string()))??;

        let (status, location, body) = response;

        if redirects_left > 0
            && matches!(status, 301 | 302 | 303 | 307 | 308)
            && let Some(location) = location
        {
            redirects_left -= 1;
            // Absolute redirect target replaces the URL; relative target keeps host/scheme.
            if location.starts_with("http://") || location.starts_with("https://") {
                current_url = location;
                current_path = String::new();
            } else {
                current_path = location;
            }
            continue;
        }

        return Ok(ProbeResponse { status, body });
    }
}

/// A single request/response round-trip (no redirect handling). Returns
/// `(status, location_header, body)`.
async fn probe_once(
    target: &Target,
    headers: &[(String, String)],
) -> Result<(u16, Option<String>, String)> {
    let request = build_request(target, headers);
    let addr = format!("{}:{}", target.host, target.port);

    let tcp = TcpStream::connect(&addr)
        .await
        .map_err(|e| GatewayError::BackendUnavailable(format!("connect to {addr} failed: {e}")))?;

    let raw = if target.tls {
        let connector = tls_connector()?;
        let server_name = rustls::pki_types::ServerName::try_from(target.host.clone())
            .map_err(|e| GatewayError::HttpError(format!("invalid TLS server name: {e}")))?;
        let mut stream = connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| GatewayError::HttpError(format!("TLS handshake failed: {e}")))?;
        exchange(&mut stream, &request).await?
    } else {
        let mut stream = tcp;
        exchange(&mut stream, &request).await?
    };

    parse_response(&raw)
}

/// Writes the request and reads the response bytes (bounded) until EOF or cap.
async fn exchange<S>(stream: &mut S, request: &[u8]) -> Result<Vec<u8>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    stream
        .write_all(request)
        .await
        .map_err(|e| GatewayError::HttpError(format!("failed to send request: {e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| GatewayError::HttpError(format!("failed to flush request: {e}")))?;

    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream
            .read(&mut chunk)
            .await
            .map_err(|e| GatewayError::HttpError(format!("failed to read response: {e}")))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() >= MAX_RESPONSE_BYTES {
            buf.truncate(MAX_RESPONSE_BYTES);
            break;
        }
    }

    Ok(buf)
}

fn build_request(target: &Target, headers: &[(String, String)]) -> Vec<u8> {
    let host_header = if (target.tls && target.port == 443) || (!target.tls && target.port == 80) {
        target.host.clone()
    } else {
        format!("{}:{}", target.host, target.port)
    };

    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: oxigeo-gateway-healthcheck\r\nAccept: */*\r\nConnection: close\r\n",
        target.path, host_header
    );

    for (name, value) in headers {
        // Skip caller attempts to override framing-critical headers.
        let lower = name.to_ascii_lowercase();
        if lower == "host" || lower == "connection" || lower == "content-length" {
            continue;
        }
        request.push_str(&format!("{name}: {value}\r\n"));
    }

    request.push_str("\r\n");
    request.into_bytes()
}

fn parse_response(raw: &[u8]) -> Result<(u16, Option<String>, String)> {
    // Split headers from body on the first CRLFCRLF.
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| (pos, pos + 4))
        .unwrap_or((raw.len(), raw.len()));

    let header_bytes = &raw[..split.0];
    let body_bytes = raw.get(split.1..).unwrap_or(&[]);

    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.split("\r\n");

    let status_line = lines
        .next()
        .ok_or_else(|| GatewayError::HttpError("empty HTTP response".to_string()))?;

    // Status line: "HTTP/1.1 200 OK"
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            GatewayError::HttpError(format!("malformed HTTP status line: '{status_line}'"))
        })?;

    let mut location = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("location")
        {
            location = Some(value.trim().to_string());
            break;
        }
    }

    Ok((
        status,
        location,
        String::from_utf8_lossy(body_bytes).to_string(),
    ))
}

/// Lazily-built, shared TLS connector using the Pure-Rust OxiTLS RustCrypto provider and the
/// Mozilla webpki root bundle. Built once and reused across probes.
///
/// Exposed as `pub(crate)` so the reverse proxy (`server::proxy`) can reuse the exact same
/// Pure-Rust TLS client configuration when forwarding to `https://` upstreams.
pub(crate) fn tls_connector() -> Result<tokio_rustls::TlsConnector> {
    static CONNECTOR: OnceLock<std::result::Result<tokio_rustls::TlsConnector, String>> =
        OnceLock::new();

    let result = CONNECTOR.get_or_init(|| {
        use oxitls_adapter_rustls_rustcrypto::RustcryptoClientConfigBuilder;
        use oxitls_webpki_roots::webpki_root_certs;

        let root_store = webpki_root_certs();
        let config = RustcryptoClientConfigBuilder::new()
            .with_roots(root_store)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
    });

    match result {
        Ok(connector) => Ok(connector.clone()),
        Err(e) => Err(GatewayError::HttpError(format!(
            "failed to build TLS client config: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    /// Spawns a one-shot TCP server that returns a fixed raw HTTP response for each
    /// connection, looping until aborted. Returns its address.
    async fn spawn_http_server(
        response: &'static str,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn test_probe_parses_real_200() {
        let (addr, handle) = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await;
        let url = format!("http://{addr}");

        let resp = http_probe(&url, "/health", Duration::from_secs(2), &[], false)
            .await
            .expect("probe should succeed");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "OK");

        handle.abort();
    }

    #[tokio::test]
    async fn test_probe_reports_503() {
        let (addr, handle) = spawn_http_server(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
        let url = format!("http://{addr}");

        let resp = http_probe(&url, "/health", Duration::from_secs(2), &[], false)
            .await
            .expect("probe should still parse an error response");
        assert_eq!(resp.status, 503);

        handle.abort();
    }

    #[tokio::test]
    async fn test_probe_connection_refused_errors() {
        // Nothing is listening on this port; the probe must surface a real error, never a
        // fabricated healthy response.
        let result = http_probe(
            "http://127.0.0.1:1",
            "/health",
            Duration::from_secs(2),
            &[],
            false,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_probe_follows_redirect() {
        let (addr, handle) = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone",
        )
        .await;
        // Point the redirect at the same server so the second hop resolves to 200.
        let url = format!("http://{addr}");
        let resp = http_probe(&url, "/health", Duration::from_secs(2), &[], true)
            .await
            .expect("probe should succeed");
        assert_eq!(resp.status, 200);
        handle.abort();
    }
}
