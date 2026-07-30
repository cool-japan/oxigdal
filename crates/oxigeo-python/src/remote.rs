//! Remote / cloud raster source support.
//!
//! [`Dataset::open`](crate::dataset::Dataset::open) needs to work on more
//! than local filesystem paths: the crate's own docstrings advertise
//! `oxigeo.open_raster("s3://bucket/file.tif", ...)`. This module provides:
//!
//! - [`classify_remote_url`]: detects whether a path is a local filesystem
//!   path or a remote URL (and which scheme), without doing any I/O.
//! - [`fetch_remote_bytes`]: fetches the full object body for a remote URL,
//!   honoring a subset of the `options` dict as cloud-auth configuration.
//!   This is a **real** implementation (via `oxigeo-cloud`'s S3/HTTP/GCS/Azure
//!   backends) when the `cloud` feature is enabled, and an **honest typed
//!   error** (never a silent no-op or fake data) when it is not.
//! - [`MemoryDataSource`] / [`AnySource`]: an in-memory [`DataSource`]
//!   implementation so `GeoTiffReader` can read the fetched bytes directly,
//!   without ever touching the filesystem for remote objects.

use std::collections::HashMap;

use oxigeo_core::error::{IoError, OxiGeoError, Result as CoreResult};
use oxigeo_core::io::{ByteRange, DataSource, FileDataSource};
use pyo3::PyErr;
use pyo3::exceptions::{PyNotImplementedError, PyValueError};

/// The remote schemes this module knows how to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteScheme {
    /// AWS S3 (`s3://bucket/key`)
    S3,
    /// Google Cloud Storage (`gs://` or `gcs://`)
    Gcs,
    /// Azure Blob Storage (`az://` or `azure://`)
    Azure,
    /// Plain HTTP/HTTPS
    Http,
}

/// Classifies `path` as a local filesystem path (`None`) or a remote URL
/// (`Some(scheme)`). Pure syntax check -- no I/O, no network.
#[must_use]
pub fn classify_remote_url(path: &str) -> Option<RemoteScheme> {
    if path.starts_with("s3://") {
        Some(RemoteScheme::S3)
    } else if path.starts_with("gs://") || path.starts_with("gcs://") {
        Some(RemoteScheme::Gcs)
    } else if path.starts_with("az://") || path.starts_with("azure://") {
        Some(RemoteScheme::Azure)
    } else if path.starts_with("http://") || path.starts_with("https://") {
        Some(RemoteScheme::Http)
    } else {
        None
    }
}

/// An in-memory [`DataSource`], used to feed a fully-fetched remote object's
/// bytes into `GeoTiffReader` without ever writing them to disk.
pub struct MemoryDataSource {
    data: Vec<u8>,
}

impl MemoryDataSource {
    /// Wraps an already-fetched byte buffer as a data source.
    #[must_use]
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Borrows `range` out of the fetched buffer, or reports the same
    /// out-of-range error [`DataSource::read_range`] reports for it.
    fn slice_for(&self, range: ByteRange) -> CoreResult<&[u8]> {
        let out_of_range = || {
            OxiGeoError::Io(IoError::Read {
                message: format!(
                    "out-of-range read {}..{} for a {}-byte in-memory buffer",
                    range.start,
                    range.end,
                    self.data.len()
                ),
            })
        };
        let start = usize::try_from(range.start).map_err(|_| out_of_range())?;
        let end = usize::try_from(range.end).map_err(|_| out_of_range())?;
        // `get` rejects both an inverted range (`start > end`) and one running
        // past the buffer, exactly like the explicit checks it replaces.
        self.data.get(start..end).ok_or_else(out_of_range)
    }
}

/// Builds the error a `read_range_into` implementation returns when the
/// caller's destination buffer cannot hold the whole range.
///
/// Mirrors the message `oxigeo_core::io`'s built-in sources produce (their
/// helper is crate-private) so the diagnostic is identical whichever source a
/// caller is holding.
fn dst_too_small(needed: usize, available: usize) -> OxiGeoError {
    OxiGeoError::invalid_parameter(
        "dst",
        format!(
            "destination buffer is {available} bytes but the requested range needs {needed}; \
             size it with ByteRange::len()"
        ),
    )
}

/// Computes the destination length `range` requires, or `None` when the range
/// is itself malformed (inverted, or wider than `usize`).
///
/// A `None` result means "let the source's own range check report it", which
/// keeps `read_range_into` erroring exactly like `read_range` instead of
/// underflowing on `ByteRange::len`.
fn needed_len(range: ByteRange) -> Option<usize> {
    usize::try_from(range.end.checked_sub(range.start)?).ok()
}

impl DataSource for MemoryDataSource {
    fn size(&self) -> CoreResult<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> CoreResult<Vec<u8>> {
        Ok(self.slice_for(range)?.to_vec())
    }

    /// Copies straight out of the fetched buffer, skipping the intermediate
    /// `Vec` the trait's default implementation would allocate per block
    /// (cool-japan/oxigeo#14).
    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> CoreResult<usize> {
        if let Some(needed) = needed_len(range)
            && dst.len() < needed
        {
            return Err(dst_too_small(needed, dst.len()));
        }
        let src = self.slice_for(range)?;
        let available = dst.len();
        let out = dst
            .get_mut(..src.len())
            .ok_or_else(|| dst_too_small(src.len(), available))?;
        out.copy_from_slice(src);
        Ok(src.len())
    }

    /// Lends the requested bytes straight out of the resident buffer: reading a
    /// block of a remote raster costs neither an allocation nor a copy once the
    /// object has been fetched.
    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        self.slice_for(range).ok()
    }
}

/// Either a local file or an in-memory (already fully fetched, e.g. remote)
/// byte source. Both implement [`DataSource`], so `GeoTiffReader<AnySource>`
/// works uniformly over local and remote datasets.
pub enum AnySource {
    /// A locally opened file.
    File(FileDataSource),
    /// A fully-buffered in-memory object (fetched from a remote URL).
    Memory(MemoryDataSource),
}

impl DataSource for AnySource {
    fn size(&self) -> CoreResult<u64> {
        match self {
            Self::File(f) => f.size(),
            Self::Memory(m) => m.size(),
        }
    }

    fn read_range(&self, range: ByteRange) -> CoreResult<Vec<u8>> {
        match self {
            Self::File(f) => f.read_range(range),
            Self::Memory(m) => m.read_range(range),
        }
    }

    /// Forwards to the inner source rather than inheriting the trait default
    /// (cool-japan/oxigeo#14): the default would allocate a `Vec` per block and
    /// copy it into `dst`, throwing away `FileDataSource`'s single positional
    /// `pread` and `MemoryDataSource`'s direct copy.
    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> CoreResult<usize> {
        match self {
            Self::File(f) => f.read_range_into(range, dst),
            Self::Memory(m) => m.read_range_into(range, dst),
        }
    }

    /// Forwards to the inner source so a fully-buffered remote object can still
    /// be read without copying. `FileDataSource` cannot lend and returns `None`,
    /// which is the correct answer for the local-file arm.
    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        match self {
            Self::File(f) => f.range_slice(range),
            Self::Memory(m) => m.range_slice(range),
        }
    }
}

/// Returns `true` if the given options map has a truthy value (`YES`, `TRUE`,
/// `1`, case-insensitively) for `key`.
///
/// Only consumed by `cloud_fetch` (behind the `cloud` feature); allowed dead
/// otherwise so a non-cloud build stays warning-free.
#[cfg_attr(not(feature = "cloud"), allow(dead_code))]
fn option_is_truthy(options: &HashMap<String, String>, key: &str) -> bool {
    options
        .get(key)
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("yes") || v.eq_ignore_ascii_case("true") || v == "1"
        })
        .unwrap_or(false)
}

#[cfg(feature = "cloud")]
mod cloud_fetch {
    use super::{HashMap, RemoteScheme, option_is_truthy};
    use oxigeo_cloud::CloudBackend;
    use oxigeo_cloud::auth::Credentials;
    use pyo3::PyErr;
    use pyo3::exceptions::{PyIOError, PyNotImplementedError};

    fn cloud_err_to_py(err: oxigeo_cloud::CloudError) -> PyErr {
        PyIOError::new_err(err.to_string())
    }

    /// Applies the subset of GDAL/AWS-style `options` we can actually honor
    /// via `oxigeo-cloud`'s `S3Backend` to a freshly URL-derived backend.
    ///
    /// Recognized keys: `AWS_REGION`/`AWS_DEFAULT_REGION`,
    /// `AWS_ENDPOINT_URL`/`AWS_S3_ENDPOINT` (S3-compatible endpoints such as
    /// MinIO), `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`/`AWS_SESSION_TOKEN`.
    ///
    /// `AWS_NO_SIGN_REQUEST` (truly unsigned/anonymous requests) is
    /// deliberately **not** silently ignored: `oxigeo-cloud`'s `S3Backend`
    /// has no anonymous-credentials mode (its `Credentials::None` falls back
    /// to the ambient AWS credential chain, not to unsigned requests), so
    /// honoring it correctly is out of scope here. Rather than silently
    /// falling back to the (different) ambient chain, this returns a clear
    /// typed error when the caller explicitly asked for unsigned access and
    /// gave no explicit credentials.
    fn configure_s3(
        backend: oxigeo_cloud::backends::s3::S3Backend,
        options: &HashMap<String, String>,
    ) -> Result<oxigeo_cloud::backends::s3::S3Backend, PyErr> {
        let mut backend = backend;

        if let Some(region) = options
            .get("AWS_REGION")
            .or_else(|| options.get("AWS_DEFAULT_REGION"))
        {
            backend = backend.with_region(region.clone());
        }

        if let Some(endpoint) = options
            .get("AWS_ENDPOINT_URL")
            .or_else(|| options.get("AWS_S3_ENDPOINT"))
        {
            backend = backend.with_endpoint(endpoint.clone());
        }

        let explicit_keys = (
            options.get("AWS_ACCESS_KEY_ID"),
            options.get("AWS_SECRET_ACCESS_KEY"),
        );

        match explicit_keys {
            (Some(access_key), Some(secret_key)) => {
                backend = backend.with_credentials(Credentials::AccessKey {
                    access_key: access_key.clone(),
                    secret_key: secret_key.clone(),
                    session_token: options.get("AWS_SESSION_TOKEN").cloned(),
                });
            }
            _ => {
                if option_is_truthy(options, "AWS_NO_SIGN_REQUEST") {
                    return Err(PyNotImplementedError::new_err(
                        "AWS_NO_SIGN_REQUEST=YES (truly unsigned/anonymous S3 access) is not \
                         yet supported: oxigeo-cloud's S3Backend has no anonymous-credentials \
                         mode (it falls back to the ambient AWS credential chain, which is not \
                         the same thing as unsigned requests). Provide explicit \
                         AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY options, set up ambient AWS \
                         credentials, or fetch the object out-of-band and open it as a local \
                         file.",
                    ));
                }
            }
        }

        Ok(backend)
    }

    /// Applies the subset of `options` we can honor for HTTP(S): bearer
    /// token, basic auth, and arbitrary `HEADER_<Name>` -> custom header.
    fn configure_http(
        backend: oxigeo_cloud::backends::http::HttpBackend,
        options: &HashMap<String, String>,
    ) -> oxigeo_cloud::backends::http::HttpBackend {
        use oxigeo_cloud::backends::http::HttpAuth;

        let mut backend = backend;

        if let Some(token) = options.get("HTTP_BEARER_TOKEN") {
            backend = backend.with_auth(HttpAuth::Bearer {
                token: token.clone(),
            });
        } else if let (Some(user), Some(pass)) = (
            options.get("HTTP_BASIC_USER"),
            options.get("HTTP_BASIC_PASSWORD"),
        ) {
            backend = backend.with_auth(HttpAuth::Basic {
                username: user.clone(),
                password: pass.clone(),
            });
        }

        for (key, value) in options {
            if let Some(header_name) = key.strip_prefix("HEADER_") {
                backend = backend.with_header(header_name.to_string(), value.clone());
            }
        }

        backend
    }

    /// Builds a `oxigeo-cloud` backend for `url`, applying whatever of
    /// `options` this backend kind supports, then synchronously fetches the
    /// full object body. This does blocking network I/O -- callers must run
    /// it with the GIL released.
    pub fn fetch(
        url: &str,
        _scheme: RemoteScheme,
        options: &HashMap<String, String>,
    ) -> Result<Vec<u8>, PyErr> {
        let backend = CloudBackend::from_url(url).map_err(cloud_err_to_py)?;

        let backend = match backend {
            CloudBackend::S3 { backend, key } => CloudBackend::S3 {
                backend: configure_s3(backend, options)?,
                key,
            },
            CloudBackend::Http { backend, path } => CloudBackend::Http {
                backend: configure_http(backend, options),
                path,
            },
            other => other,
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "Failed to start async runtime for remote fetch: {e}"
                ))
            })?;

        let bytes = runtime.block_on(backend.get()).map_err(cloud_err_to_py)?;

        Ok(bytes.to_vec())
    }
}

/// Fetches the full object body for a remote URL, honoring the subset of
/// `options` documented on [`fetch_remote_bytes`]'s cloud-enabled
/// implementation. Does blocking network I/O -- run with the GIL released.
///
/// # Errors
/// Returns a typed `PyErr`:
/// - when the `cloud` feature is disabled: a clear error explaining that
///   remote/cloud paths require rebuilding with `--features cloud` (this
///   crate is not silently pretending to support what it cannot);
/// - when the backend rejects the request (network error, 404, auth
///   failure, unsupported combination of options): the underlying
///   `oxigeo-cloud` error, converted to the matching Python exception type.
#[cfg(feature = "cloud")]
pub fn fetch_remote_bytes(
    url: &str,
    scheme: RemoteScheme,
    options: &HashMap<String, String>,
) -> Result<Vec<u8>, PyErr> {
    cloud_fetch::fetch(url, scheme, options)
}

/// Honest typed error for remote URLs when the `cloud` feature is not
/// compiled in -- never a silent no-op / fake read.
#[cfg(not(feature = "cloud"))]
pub fn fetch_remote_bytes(
    url: &str,
    scheme: RemoteScheme,
    _options: &HashMap<String, String>,
) -> Result<Vec<u8>, PyErr> {
    let scheme_name = match scheme {
        RemoteScheme::S3 => "s3://",
        RemoteScheme::Gcs => "gs:// / gcs://",
        RemoteScheme::Azure => "az:// / azure://",
        RemoteScheme::Http => "http:// / https://",
    };
    Err(PyNotImplementedError::new_err(format!(
        "Cannot open '{url}': remote/cloud paths ({scheme_name}) require oxigeo-python built \
         with the 'cloud' feature (cargo build --features cloud, or install the oxigeo Python \
         wheel variant that bundles it). This build only supports local filesystem paths."
    )))
}

/// Rejects a request for an unimplemented driver with a clear, honest
/// error rather than silently ignoring the requested driver (previously,
/// `driver` was parsed and validated but never actually consulted).
pub(crate) fn unsupported_driver_error(driver: &str) -> PyErr {
    match driver {
        "VRT" => PyNotImplementedError::new_err(
            "The 'VRT' driver is not yet wired into oxigeo-python's Dataset reader: \
             oxigeo-drivers/vrt implements VRT parsing at the Rust level, but Dataset::open() \
             only supports reading GeoTIFF data today. Read the underlying source raster(s) \
             directly, or use oxigeo's Rust VRT APIs (oxigeo_vrt) from a native extension.",
        ),
        other => PyValueError::new_err(format!("Unsupported driver '{other}'")),
    }
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
                "oxigeo_python_remote_{}_{seq}_{name}",
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
    fn test_classify_remote_url() {
        assert_eq!(
            classify_remote_url("s3://bucket/key.tif"),
            Some(RemoteScheme::S3)
        );
        assert_eq!(
            classify_remote_url("gs://bucket/object.tif"),
            Some(RemoteScheme::Gcs)
        );
        assert_eq!(
            classify_remote_url("gcs://bucket/object.tif"),
            Some(RemoteScheme::Gcs)
        );
        assert_eq!(
            classify_remote_url("az://container/blob.tif"),
            Some(RemoteScheme::Azure)
        );
        assert_eq!(
            classify_remote_url("https://example.com/file.tif"),
            Some(RemoteScheme::Http)
        );
        assert_eq!(
            classify_remote_url("http://example.com/file.tif"),
            Some(RemoteScheme::Http)
        );
        assert_eq!(classify_remote_url("/local/path/file.tif"), None);
        assert_eq!(classify_remote_url("relative/file.tif"), None);
        assert_eq!(classify_remote_url("C:\\windows\\file.tif"), None);
    }

    #[test]
    fn test_option_is_truthy() {
        let mut options = HashMap::new();
        options.insert("AWS_NO_SIGN_REQUEST".to_string(), "YES".to_string());
        assert!(option_is_truthy(&options, "AWS_NO_SIGN_REQUEST"));

        options.insert("AWS_NO_SIGN_REQUEST".to_string(), "no".to_string());
        assert!(!option_is_truthy(&options, "AWS_NO_SIGN_REQUEST"));

        assert!(!option_is_truthy(&options, "MISSING_KEY"));
    }

    #[test]
    fn test_memory_data_source_read_range() {
        let source = MemoryDataSource::new(vec![1, 2, 3, 4, 5]);
        assert_eq!(source.size().expect("size"), 5);
        let bytes = source.read_range(ByteRange::new(1, 4)).expect("read_range");
        assert_eq!(bytes, vec![2, 3, 4]);
    }

    #[test]
    fn test_memory_data_source_out_of_range() {
        let source = MemoryDataSource::new(vec![1, 2, 3]);
        assert!(source.read_range(ByteRange::new(1, 10)).is_err());
    }

    /// cool-japan/oxigeo#14: the zero-copy entry points must agree with
    /// `read_range` byte for byte, and error for error.
    #[test]
    fn test_issue_14_memory_read_range_into_matches_read_range() {
        let source = MemoryDataSource::new((0u8..32).collect());
        for range in [
            ByteRange::new(0, 32),  // whole buffer
            ByteRange::new(8, 20),  // interior
            ByteRange::new(0, 1),   // leading boundary
            ByteRange::new(31, 32), // trailing boundary
            ByteRange::new(5, 5),   // empty
            ByteRange::new(32, 32), // empty at EOF
        ] {
            let expected = source.read_range(range).expect("read_range");
            let mut dst = vec![0xAAu8; expected.len()];
            let written = source.read_range_into(range, &mut dst).expect("read_into");
            assert_eq!(written, expected.len(), "count mismatch for {range:?}");
            assert_eq!(dst, expected, "bytes mismatch for {range:?}");
        }

        // Past EOF / inverted: both paths must fail, and `read_range_into` must
        // not panic on the underflowing length.
        for range in [
            ByteRange::new(28, 40),
            ByteRange::new(32, 33),
            ByteRange::new(20, 8),
        ] {
            assert!(source.read_range(range).is_err(), "read_range {range:?}");
            let mut dst = vec![0u8; 64];
            let err = source
                .read_range_into(range, &mut dst)
                .expect_err("read_range_into should reject");
            assert!(
                matches!(err, OxiGeoError::Io(IoError::Read { .. })),
                "expected an out-of-range read error for {range:?}, got {err}"
            );
        }
    }

    #[test]
    fn test_issue_14_memory_read_range_into_buffer_sizing() {
        let source = MemoryDataSource::new((0u8..16).collect());
        let range = ByteRange::new(4, 12);

        // Too long: only the first 8 bytes are written, the tail is preserved.
        let mut dst = vec![0xEEu8; 12];
        assert_eq!(
            source.read_range_into(range, &mut dst).expect("read_into"),
            8
        );
        assert_eq!(&dst[..8], &(4u8..12).collect::<Vec<u8>>()[..]);
        assert_eq!(&dst[8..], &[0xEE; 4], "tail must be left alone");

        // Too short: rejected before anything is written.
        let mut dst = vec![0xEEu8; 7];
        let err = source
            .read_range_into(range, &mut dst)
            .expect_err("short dst must be rejected");
        assert!(
            matches!(err, OxiGeoError::InvalidParameter { parameter, .. } if parameter == "dst"),
            "expected an InvalidParameter(dst) error, got {err}"
        );
        assert_eq!(dst, vec![0xEE; 7], "dst must be untouched");

        // An empty range writes nothing, even into an empty destination.
        assert_eq!(
            source
                .read_range_into(ByteRange::new(3, 3), &mut [])
                .expect("empty range"),
            0
        );
    }

    #[test]
    fn test_issue_14_memory_range_slice_borrows_backing_buffer() {
        let source = MemoryDataSource::new((0u8..64).collect());
        let borrowed = source.range_slice(ByteRange::new(16, 48)).expect("borrow");
        assert_eq!(borrowed, &source.data[16..48]);
        assert!(
            std::ptr::eq(borrowed.as_ptr(), source.data[16..48].as_ptr()),
            "range_slice must borrow the backing buffer, not copy it"
        );
        assert!(
            source
                .range_slice(ByteRange::new(9, 9))
                .expect("empty")
                .is_empty()
        );
        assert!(
            source.range_slice(ByteRange::new(60, 65)).is_none(),
            "past EOF"
        );
        assert!(
            source.range_slice(ByteRange::new(40, 8)).is_none(),
            "inverted"
        );
        assert!(
            source
                .range_slice(ByteRange::new(u64::MAX - 1, u64::MAX))
                .is_none(),
            "unrepresentable offset"
        );
    }

    /// `AnySource` must forward both zero-copy entry points to whichever source
    /// it wraps -- inheriting the trait defaults would silently drop
    /// `FileDataSource`'s single `pread` and `MemoryDataSource`'s borrow.
    #[test]
    fn test_issue_14_any_source_forwards_to_inner() {
        use std::io::Write;

        let payload: Vec<u8> = (0u8..64).collect();

        let memory = AnySource::Memory(MemoryDataSource::new(payload.clone()));
        let range = ByteRange::new(16, 48);
        let borrowed = memory
            .range_slice(range)
            .expect("the memory arm must lend its buffer");
        assert_eq!(borrowed, &payload[16..48]);
        let mut dst = vec![0u8; 32];
        assert_eq!(memory.read_range_into(range, &mut dst).expect("into"), 32);
        assert_eq!(dst, payload[16..48]);

        let path = TempPath::new("issue_14_any_source.bin");
        {
            let mut file = std::fs::File::create(&path).expect("create temp file");
            file.write_all(&payload).expect("write temp file");
            file.flush().expect("flush temp file");
        }
        let file = AnySource::File(FileDataSource::open(&path).expect("open temp file"));
        // A file cannot lend, so the caller must fall back to a copy -- but the
        // copy still goes straight into `dst`.
        assert!(file.range_slice(range).is_none());
        let mut dst = vec![0xEEu8; 40];
        assert_eq!(file.read_range_into(range, &mut dst).expect("into"), 32);
        assert_eq!(&dst[..32], &payload[16..48]);
        assert_eq!(&dst[32..], &[0xEE; 8], "tail must be left alone");
        assert!(
            file.read_range_into(range, &mut [0u8; 8]).is_err(),
            "a short dst must be rejected through the forwarding arm too"
        );
    }

    #[cfg(not(feature = "cloud"))]
    #[test]
    fn test_fetch_remote_bytes_without_cloud_feature_is_honest_error() {
        pyo3::Python::initialize();
        let err = fetch_remote_bytes("s3://bucket/key.tif", RemoteScheme::S3, &HashMap::new())
            .expect_err("should error without the cloud feature");
        pyo3::Python::attach(|py| {
            assert!(
                err.to_string().contains("cloud") || err.value(py).to_string().contains("cloud")
            );
        });
    }

    /// With the `cloud` feature, this is a *real* network fetch attempt (via
    /// `oxigeo-cloud`'s `HttpBackend`), not a fake/stubbed path: pointed at
    /// an address nothing listens on, it must fail with a real connection
    /// error rather than panicking or fabricating a successful empty read.
    #[cfg(feature = "cloud")]
    #[test]
    fn test_fetch_remote_bytes_http_connection_failure_is_real_error() {
        pyo3::Python::initialize();
        let err = fetch_remote_bytes(
            "http://127.0.0.1:1/unreachable.tif",
            RemoteScheme::Http,
            &HashMap::new(),
        )
        .expect_err("connecting to a port nothing listens on must fail");
        pyo3::Python::attach(|_py| {
            let message = err.to_string();
            assert!(
                !message.is_empty(),
                "expected a real, descriptive connection error"
            );
        });
    }

    /// `AWS_NO_SIGN_REQUEST=YES` without explicit credentials must raise a
    /// clear, honest error under the `cloud` feature too: oxigeo-cloud's
    /// `S3Backend` has no genuine anonymous/unsigned-request mode, so
    /// silently falling back to the (different) ambient credential chain
    /// would be misleading.
    #[cfg(feature = "cloud")]
    #[test]
    fn test_fetch_remote_bytes_s3_unsigned_without_credentials_is_honest_error() {
        pyo3::Python::initialize();
        let mut options = HashMap::new();
        options.insert("AWS_NO_SIGN_REQUEST".to_string(), "YES".to_string());
        let err = fetch_remote_bytes(
            "s3://some-public-bucket/key.tif",
            RemoteScheme::S3,
            &options,
        )
        .expect_err("unsigned S3 access without oxigeo-cloud support must error");
        pyo3::Python::attach(|_py| {
            let message = err.to_string();
            assert!(
                message.contains("AWS_NO_SIGN_REQUEST")
                    || message.to_lowercase().contains("unsigned"),
                "expected a message naming the unsupported unsigned-request case, got: {message}"
            );
        });
    }
}
