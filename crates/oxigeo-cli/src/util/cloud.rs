//! Cloud URI dispatch for reading from S3, GCS, Azure Blob, and local file paths.

use anyhow::{Result, anyhow};
use oxigeo_core::io::{DataSource, FileDataSource};
use std::sync::OnceLock;

static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Lazily initialises the shared Tokio runtime for cloud I/O.
///
/// Uses `OnceLock` so the runtime is created at most once across all calls.
fn get_runtime() -> Result<&'static tokio::runtime::Runtime> {
    if let Some(rt) = TOKIO_RUNTIME.get() {
        return Ok(rt);
    }
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| anyhow!("failed to create tokio runtime for cloud I/O: {}", e))?;
    // `set` returns Err(rt) if another thread races and wins; in that case we
    // discard our freshly-built runtime and use theirs.
    let _ = TOKIO_RUNTIME.set(rt);
    TOKIO_RUNTIME
        .get()
        .ok_or_else(|| anyhow!("tokio runtime unavailable after init"))
}

/// Returns true if the string looks like a cloud URI (s3://, gs://, az://).
pub fn is_cloud_uri(uri: &str) -> bool {
    uri.starts_with("s3://") || uri.starts_with("gs://") || uri.starts_with("az://")
}

/// Returns an informative error when the user tries to write to a cloud URI.
pub fn error_for_cloud_write(uri: &str) -> anyhow::Error {
    anyhow!("cloud write not yet supported: {uri}; please write locally then upload")
}

/// Opens a data source for the given URI or file path.
///
/// Supports:
/// - Bare file paths: `/path/to/file.tif`
/// - `file:///path/to/file.tif`
/// - `s3://bucket/key`
/// - `gs://bucket/object`
/// - `az://container/blob`
pub fn open_datasource(uri: &str) -> Result<Box<dyn DataSource>> {
    if let Some(path) = uri.strip_prefix("file://") {
        return Ok(Box::new(
            FileDataSource::open(path).map_err(|e| anyhow!("{}", e))?,
        ));
    }

    if is_cloud_uri(uri) {
        let rt = get_runtime()?;
        let ds = rt.block_on(open_cloud_datasource(uri))?;
        return Ok(ds);
    }

    // Bare path
    Ok(Box::new(
        FileDataSource::open(uri).map_err(|e| anyhow!("{}", e))?,
    ))
}

async fn open_cloud_datasource(uri: &str) -> Result<Box<dyn DataSource>> {
    let (backend, bucket, key) = oxigeo_rs3gw::parse_url(uri).map_err(|e| anyhow!("{}", e))?;
    let storage = backend
        .create_storage()
        .await
        .map_err(|e| anyhow!("{}", e))?;
    let ds = oxigeo_rs3gw::Rs3gwDataSource::new(storage, bucket, key)
        .await
        .map_err(|e| anyhow!("{}", e))?;
    Ok(Box::new(ds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_cli_cloud_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_is_cloud_uri() {
        assert!(is_cloud_uri("s3://bucket/key"));
        assert!(is_cloud_uri("gs://bucket/obj"));
        assert!(is_cloud_uri("az://container/blob"));
        assert!(!is_cloud_uri("/local/path.tif"));
        assert!(!is_cloud_uri("file:///local.tif"));
        assert!(!is_cloud_uri("relative/path.tif"));
    }

    #[test]
    fn test_error_for_cloud_write() {
        let err = error_for_cloud_write("s3://my-bucket/output.tif");
        let msg = err.to_string();
        assert!(msg.contains("s3://my-bucket/output.tif"));
        assert!(msg.contains("not yet supported"));
    }

    #[test]
    fn test_open_datasource_file_path() {
        let path = TempPath::new("direct.bin");
        std::fs::write(&path, b"test data").expect("write temp file");
        let result = open_datasource(path.to_str().expect("valid path"));
        assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn test_open_datasource_file_uri() {
        let path = TempPath::new("uri.bin");
        std::fs::write(&path, b"test data").expect("write temp file");
        let uri = format!("file://{}", path.display());
        let result = open_datasource(&uri);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn test_cloud_uri_classification_comprehensive() {
        // All recognised cloud schemes
        assert!(is_cloud_uri("s3://bucket/path/to/file.tif"));
        assert!(is_cloud_uri("gs://my-gcs-bucket/dir/file.tif"));
        assert!(is_cloud_uri("az://mycontainer/blob/path.tif"));

        // Non-cloud URIs that must NOT be treated as cloud
        assert!(!is_cloud_uri("file:///data/local.tif"));
        assert!(!is_cloud_uri("/absolute/path.tif"));
        assert!(!is_cloud_uri("relative/path.tif"));
        assert!(!is_cloud_uri("http://example.com/file.tif"));
        assert!(!is_cloud_uri("https://example.com/file.tif"));
        assert!(!is_cloud_uri(""));
    }
}
