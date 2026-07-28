//! Health check implementation.

use super::probe;
use std::time::Duration;

/// Health checker for backend servers.
///
/// Issues a real HTTP(S) `GET` to the backend and treats any 2xx/3xx status as healthy.
/// A backend that is actually down (connection refused, timeout, or a 4xx/5xx response) is
/// reported unhealthy — the load balancer then stops routing to it.
pub struct HealthChecker {
    interval: Duration,
    timeout: Duration,
    /// Request path appended to the backend URL for the probe.
    path: String,
}

impl HealthChecker {
    /// Creates a new health checker probing the `/health` path.
    pub fn new(interval: Duration, timeout: Duration) -> Self {
        Self {
            interval,
            timeout,
            path: "/health".to_string(),
        }
    }

    /// Creates a new health checker probing a custom path.
    pub fn with_path(interval: Duration, timeout: Duration, path: impl Into<String>) -> Self {
        Self {
            interval,
            timeout,
            path: path.into(),
        }
    }

    /// Performs a health check on a backend URL.
    ///
    /// Returns `true` only when the backend answers with a 2xx or 3xx status within the
    /// configured timeout; connect failures, timeouts, and 4xx/5xx responses return `false`.
    pub async fn check(&self, url: &str) -> bool {
        match probe::http_probe(url, &self.path, self.timeout, &[], false).await {
            Ok(response) => {
                let healthy = (200..400).contains(&response.status);
                if !healthy {
                    tracing::debug!(
                        "health check for {} returned status {}",
                        url,
                        response.status
                    );
                }
                healthy
            }
            Err(e) => {
                tracing::debug!("health check for {} failed: {}", url, e);
                false
            }
        }
    }

    /// Gets the health check interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Gets the health check timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_health_checker_healthy_and_down() {
        let checker = HealthChecker::new(Duration::from_secs(30), Duration::from_secs(2));
        assert_eq!(checker.interval(), Duration::from_secs(30));
        assert_eq!(checker.timeout(), Duration::from_secs(2));

        // Real server returning 200 -> healthy.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
                let _ = stream.shutdown().await;
            }
        });
        let url = format!("http://{addr}");
        assert!(checker.check(&url).await, "200 backend must be healthy");
        handle.abort();

        // Nothing listening -> unhealthy (this is the whole point of the fix).
        assert!(
            !checker.check("http://127.0.0.1:1").await,
            "a down backend must be reported unhealthy"
        );
    }
}
