//! Integration tests for cloud storage backends
#![allow(clippy::panic)]

use oxigdal_cloud::*;

#[cfg(feature = "s3")]
mod s3_tests {
    use super::*;
    use backends::S3Backend;

    #[test]
    fn test_s3_backend_creation() {
        let backend = S3Backend::new("test-bucket", "data");
        assert_eq!(backend.bucket, "test-bucket");
        assert_eq!(backend.prefix, "data");
    }

    #[test]
    fn test_s3_backend_configuration() {
        let backend = S3Backend::new("bucket", "prefix")
            .with_region("us-east-1")
            .with_endpoint("http://localhost:9000");

        assert_eq!(backend.region, Some("us-east-1".to_string()));
        assert_eq!(backend.endpoint, Some("http://localhost:9000".to_string()));
    }
}

#[cfg(feature = "azure-blob")]
mod azure_tests {
    use super::*;
    use backends::AzureBlobBackend;

    #[test]
    fn test_azure_backend_creation() {
        let backend = AzureBlobBackend::new("testaccount", "testcontainer");
        assert_eq!(backend.account_name, "testaccount");
        assert_eq!(backend.container, "testcontainer");
    }

    #[test]
    fn test_azure_backend_configuration() {
        let backend = AzureBlobBackend::new("account", "container")
            .with_prefix("data/blobs")
            .with_sas_token("?sv=2020-08-04&ss=b");

        assert_eq!(backend.prefix, "data/blobs");
        assert!(backend.sas_token.is_some());
    }
}

#[cfg(feature = "gcs")]
mod gcs_tests {
    use super::*;
    use backends::GcsBackend;

    #[test]
    fn test_gcs_backend_creation() {
        let backend = GcsBackend::new("test-bucket");
        assert_eq!(backend.bucket, "test-bucket");
    }

    #[test]
    fn test_gcs_backend_configuration() {
        let backend = GcsBackend::new("bucket")
            .with_prefix("data/objects")
            .with_project_id("my-project");

        assert_eq!(backend.prefix, "data/objects");
        assert_eq!(backend.project_id, Some("my-project".to_string()));
    }
}

#[cfg(feature = "http")]
mod http_tests {
    use super::*;
    use backends::http::{HttpAuth, HttpBackend};

    #[test]
    fn test_http_backend_creation() {
        let backend = HttpBackend::new("https://example.com/data");
        assert_eq!(backend.base_url, "https://example.com/data");
    }

    #[test]
    fn test_http_backend_authentication() {
        let backend = HttpBackend::new("https://example.com").with_auth(HttpAuth::Bearer {
            token: "test-token".to_string(),
        });

        assert!(matches!(backend.auth, HttpAuth::Bearer { .. }));
    }

    #[test]
    fn test_http_backend_headers() {
        let backend =
            HttpBackend::new("https://example.com").with_header("X-Custom-Header", "value");

        assert_eq!(backend.headers.len(), 1);
    }
}

#[cfg(feature = "cache")]
mod cache_tests {
    use super::*;
    use cache::{CacheConfig, EvictionStrategy};

    #[test]
    fn test_cache_config() {
        let config = CacheConfig::new()
            .with_max_memory_size(50 * 1024 * 1024)
            .with_eviction_strategy(EvictionStrategy::Lru);

        assert_eq!(config.max_memory_size, 50 * 1024 * 1024);
        assert_eq!(config.eviction_strategy, EvictionStrategy::Lru);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_memory_cache() {
        use bytes::Bytes;
        use cache::MemoryCache;

        let config = CacheConfig::new();
        let cache = MemoryCache::new(config).expect("Failed to create cache");

        let key = "test-key".to_string();
        let data = Bytes::from("test data");

        cache
            .put(key.clone(), data.clone(), None)
            .await
            .expect("Put failed");

        let retrieved = cache.get(&key).await.expect("Get failed");
        assert_eq!(retrieved, data);
    }
}

#[cfg(feature = "prefetch")]
mod prefetch_tests {
    use super::*;
    use prefetch::{AccessPattern, AccessRecord, PatternAnalyzer};

    #[test]
    fn test_pattern_analyzer() {
        let mut analyzer = PatternAnalyzer::new(10);

        analyzer.record_access(AccessRecord::new("file_0".to_string()));
        analyzer.record_access(AccessRecord::new("file_1".to_string()));
        analyzer.record_access(AccessRecord::new("file_2".to_string()));
        analyzer.record_access(AccessRecord::new("file_3".to_string()));

        // Should detect sequential pattern (forward or backward)
        assert!(matches!(
            analyzer.current_pattern(),
            AccessPattern::SequentialForward | AccessPattern::SequentialBackward
        ));
    }

    #[test]
    fn test_spatial_pattern() {
        let mut analyzer = PatternAnalyzer::new(10);

        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_0_0_0".to_string(),
            0,
            0,
            0,
        ));
        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_1_0_0".to_string(),
            1,
            0,
            0,
        ));
        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_1_1_0".to_string(),
            1,
            1,
            0,
        ));
        analyzer.record_access(AccessRecord::with_coordinates(
            "tile_2_1_0".to_string(),
            2,
            1,
            0,
        ));

        assert_eq!(analyzer.current_pattern(), AccessPattern::Spatial);
    }
}

#[cfg(feature = "retry")]
mod retry_tests {
    use super::*;
    use retry::{CircuitBreaker, CircuitState, RetryBudget, RetryConfig};
    use std::time::Duration;

    #[test]
    fn test_retry_config() {
        let config = RetryConfig::new()
            .with_max_retries(5)
            .with_backoff_multiplier(3.0);

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.backoff_multiplier, 3.0);
    }

    #[test]
    fn test_circuit_breaker() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(60));

        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request().is_ok());

        // Open circuit with failures
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();

        assert_eq!(cb.state(), CircuitState::Open);
        assert!(cb.allow_request().is_err());
    }

    #[test]
    fn test_retry_budget() {
        let mut budget = RetryBudget::new(10, 1.0);

        for _ in 0..10 {
            assert!(budget.try_consume().is_ok());
        }

        // Should fail when budget exhausted
        assert!(budget.try_consume().is_err());
    }
}

mod auth_tests {
    use super::*;
    use auth::Credentials;

    #[test]
    fn test_credentials_api_key() {
        let creds = Credentials::api_key("test-key");
        match creds {
            Credentials::ApiKey { key } => assert_eq!(key, "test-key"),
            _ => panic!("Expected ApiKey credentials"),
        }
    }

    #[test]
    fn test_credentials_access_key() {
        let creds = Credentials::access_key("access", "secret");
        match creds {
            Credentials::AccessKey {
                access_key,
                secret_key,
                ..
            } => {
                assert_eq!(access_key, "access");
                assert_eq!(secret_key, "secret");
            }
            _ => panic!("Expected AccessKey credentials"),
        }
    }

    #[test]
    fn test_credentials_expiry() {
        let now = chrono::Utc::now();
        let past = now - chrono::Duration::hours(1);

        let expired = Credentials::OAuth2 {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: Some(past),
        };

        assert!(expired.is_expired());
    }
}

mod url_tests {
    use super::*;

    #[test]
    #[cfg(feature = "s3")]
    fn test_parse_s3_url() {
        let result = CloudBackend::from_url("s3://my-bucket/path/to/file.tif");
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "gcs")]
    fn test_parse_gcs_url() {
        let result = CloudBackend::from_url("gs://my-bucket/path/to/file.tif");
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "http")]
    fn test_parse_http_url() {
        let result = CloudBackend::from_url("https://example.com/path/to/file.tif");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_invalid_url() {
        let result = CloudBackend::from_url("invalid-url");
        assert!(result.is_err());
    }
}

#[cfg(all(feature = "async", feature = "retry"))]
mod integration_tests {
    use super::*;
    use retry::RetryExecutor;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_retry_executor() {
        let config = retry::RetryConfig::new().with_max_retries(3);
        let mut executor = RetryExecutor::new(config);

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();
        let result = executor
            .execute(|| {
                let attempts = attempts_clone.clone();
                async move {
                    let current = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    if current < 2 {
                        Err(CloudError::Timeout {
                            message: "timeout".to_string(),
                        })
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
