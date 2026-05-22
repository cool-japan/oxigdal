//! Integration tests for the `cloud-storage` feature of `oxigdal-pmtiles`.
//!
//! Tests 1–7 are pure URI parsing and URL construction — they require no
//! network access and run in every CI environment.
//!
//! Tests 8–12 are marked `#[ignore]` because they would require a real
//! (or mocked) cloud-storage server; they can be enabled ad-hoc with
//! `cargo test -- --ignored`.

#![cfg(feature = "cloud-storage")]

use oxigdal_pmtiles::{CloudCredentials, CloudObjectUri, CloudProvider};

// ── Test 1 ────────────────────────────────────────────────────────────────────

/// Parse an S3 URI into bucket, key, and provider variant.
#[test]
fn test_cloud_object_uri_parse_s3_form() {
    let uri = CloudObjectUri::parse("s3://my-bucket/path/to/tiles.pmtiles").expect("parse ok");
    assert_eq!(uri.bucket, "my-bucket");
    assert_eq!(uri.key, "path/to/tiles.pmtiles");
    assert!(
        matches!(&uri.provider, CloudProvider::S3 { region } if !region.is_empty()),
        "Provider should be S3 with a non-empty default region"
    );
}

// ── Test 2 ────────────────────────────────────────────────────────────────────

/// Parse a GCS URI and verify the provider is `Gcs`.
#[test]
fn test_cloud_object_uri_parse_gcs_form() {
    let uri = CloudObjectUri::parse("gs://my-bucket/tiles.pmtiles").expect("parse ok");
    assert_eq!(uri.bucket, "my-bucket");
    assert_eq!(uri.key, "tiles.pmtiles");
    assert!(
        matches!(uri.provider, CloudProvider::Gcs),
        "Provider should be Gcs"
    );
}

// ── Test 3 ────────────────────────────────────────────────────────────────────

/// Parse an Azure Blob URI and verify account, container, and key.
#[test]
fn test_cloud_object_uri_parse_azure_form() {
    let uri = CloudObjectUri::parse("az://myaccount/mycontainer/tiles.pmtiles").expect("parse ok");
    assert_eq!(uri.bucket, "mycontainer", "bucket should be the container");
    assert_eq!(uri.key, "tiles.pmtiles");
    assert!(
        matches!(&uri.provider, CloudProvider::AzureBlob { account } if account == "myaccount"),
        "Provider should be AzureBlob with account=myaccount"
    );
}

// ── Test 4 ────────────────────────────────────────────────────────────────────

/// An `http://` URI is not a supported cloud scheme and must return `Err`.
#[test]
fn test_cloud_object_uri_parse_invalid_scheme_errors() {
    let result = CloudObjectUri::parse("http://example.com/tiles.pmtiles");
    assert!(
        result.is_err(),
        "http:// scheme should be rejected; got Ok(_)"
    );

    // Also verify https:// is rejected (cloud-native schemes only).
    let result2 = CloudObjectUri::parse("https://example.com/tiles.pmtiles");
    assert!(
        result2.is_err(),
        "https:// scheme should be rejected; got Ok(_)"
    );

    // Completely unknown scheme.
    let result3 = CloudObjectUri::parse("ftp://bucket/key");
    assert!(
        result3.is_err(),
        "ftp:// scheme should be rejected; got Ok(_)"
    );
}

// ── Test 5 ────────────────────────────────────────────────────────────────────

/// `to_https_url` for S3 must include `amazonaws.com` and the bucket name.
#[test]
fn test_cloud_object_uri_to_https_s3_virtual_host() {
    let uri = CloudObjectUri::parse("s3://my-bucket/path/tiles.pmtiles").expect("parse ok");
    let url = uri.to_https_url().expect("url ok");
    let s = url.as_str();

    assert!(
        s.contains("amazonaws.com"),
        "S3 URL should contain 'amazonaws.com'; got '{s}'"
    );
    assert!(
        s.contains("my-bucket"),
        "S3 URL should contain the bucket name; got '{s}'"
    );
    assert!(
        s.starts_with("https://"),
        "S3 URL should use HTTPS; got '{s}'"
    );
}

// ── Test 6 ────────────────────────────────────────────────────────────────────

/// `to_https_url` for GCS must include `storage.googleapis.com` and the bucket.
#[test]
fn test_cloud_object_uri_to_https_gcs() {
    let uri = CloudObjectUri::parse("gs://my-bucket/tiles.pmtiles").expect("parse ok");
    let url = uri.to_https_url().expect("url ok");
    let s = url.as_str();

    assert!(
        s.contains("storage.googleapis.com"),
        "GCS URL should contain 'storage.googleapis.com'; got '{s}'"
    );
    assert!(
        s.contains("my-bucket"),
        "GCS URL should contain the bucket; got '{s}'"
    );
    assert!(
        s.starts_with("https://"),
        "GCS URL should use HTTPS; got '{s}'"
    );
}

// ── Test 7 ────────────────────────────────────────────────────────────────────

/// `to_https_url` for Azure must include `blob.core.windows.net`, the account,
/// and the container.
#[test]
fn test_cloud_object_uri_to_https_azure_blob() {
    let uri = CloudObjectUri::parse("az://myaccount/mycontainer/tiles.pmtiles").expect("parse ok");
    let url = uri.to_https_url().expect("url ok");
    let s = url.as_str();

    assert!(
        s.contains("blob.core.windows.net"),
        "Azure URL should contain 'blob.core.windows.net'; got '{s}'"
    );
    assert!(
        s.contains("myaccount"),
        "Azure URL should contain the account; got '{s}'"
    );
    assert!(
        s.contains("mycontainer"),
        "Azure URL should contain the container; got '{s}'"
    );
    assert!(
        s.starts_with("https://"),
        "Azure URL should use HTTPS; got '{s}'"
    );
}

// ── Test 8 ────────────────────────────────────────────────────────────────────

/// Anonymous range request returning raw bytes.
///
/// Integration test requiring network access to a real PMTiles archive.
/// Run with: `cargo test -- --ignored test_cloud_reader_anonymous_read_range_returns_bytes`
#[test]
#[ignore = "integration test requiring network access to a real PMTiles archive"]
fn test_cloud_reader_anonymous_read_range_returns_bytes() {
    // Requires a publicly accessible PMTiles archive, e.g. on a local MinIO
    // instance or a public S3 bucket.  Set PMTILES_TEST_URI in the environment.
    todo!("set PMTILES_TEST_URI and call CloudPmTilesReader::from_s3_uri / from_gcs_uri");
}

// ── Test 9 ────────────────────────────────────────────────────────────────────

/// Bearer token credential attaches `Authorization: Bearer` header.
///
/// Integration test requiring a mock HTTP server that validates the header.
#[test]
#[ignore = "integration test requiring a mock HTTP server that validates Authorization headers"]
fn test_cloud_reader_bearer_token_sends_authorization_header() {
    let _creds = CloudCredentials::bearer("test-token-abc");
    todo!("spin up mock server, assert Authorization: Bearer test-token-abc is present");
}

// ── Test 10 ───────────────────────────────────────────────────────────────────

/// `read_header` is idempotent — calling it twice returns the same cached data
/// without extra network round-trips.
///
/// Integration test requiring network access.
#[test]
#[ignore = "integration test requiring network access to a real PMTiles archive"]
fn test_cloud_reader_read_header_caches_after_first_call() {
    todo!("call read_header twice, assert second call returns same result without extra fetches");
}

// ── Test 11 ───────────────────────────────────────────────────────────────────

/// Round-trip: build an in-memory PMTiles archive, serve it via a local HTTP
/// server, open it with `CloudPmTilesReader`, and read back a known tile.
///
/// Integration test requiring a live local HTTP server.
#[test]
#[ignore = "integration test requiring a local HTTP server serving a fake PMTiles archive"]
fn test_cloud_reader_read_tile_round_trip_via_fake_archive() {
    todo!(
        "build archive with PmTilesBuilder, serve via axum/hyper, read tile via CloudPmTilesReader"
    );
}

// ── Test 12 ───────────────────────────────────────────────────────────────────

/// A tile absent from the archive returns `Ok(None)` rather than an error.
///
/// Integration test requiring network access.
#[test]
#[ignore = "integration test requiring network access to verify missing-tile handling"]
fn test_cloud_reader_404_returns_none_for_missing_tile() {
    todo!("request a tile outside the archive's zoom/extent and assert Ok(None) is returned");
}
