//! Integration tests for the cloud-native I/O module.
//! Covers ObjectUrl parsing, ByteRangeRequest, CloudRangeCoalescer,
//! PresignedUrlGenerator (SigV4), SHA-256/HMAC-SHA256 vectors,
//! MultipartUploadState, and RetryPolicy/RetryState.

use oxigdal_streaming::cloud::{
    ByteRangeRequest, CloudCredentials, CloudError, CloudRangeCoalescer, CloudScheme,
    CompletedPart, HttpMethod, MultipartUploadState, ObjectUrl, PresignedUrlConfig,
    PresignedUrlGenerator, RetryPolicy, RetryState, hex_encode, hmac_sha256, sha256,
};

// ─────────────────────────────────────────────────────────────────────────────
// ObjectUrl parsing — valid
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn parse_s3_url_basic() {
    let url = ObjectUrl::parse("s3://my-bucket/path/to/object.tiff").unwrap();
    assert_eq!(url.scheme, CloudScheme::S3);
    assert_eq!(url.bucket, "my-bucket");
    assert_eq!(url.key, "path/to/object.tiff");
}

#[test]
fn parse_s3_url_root_key() {
    let url = ObjectUrl::parse("s3://bucket/key").unwrap();
    assert_eq!(url.scheme, CloudScheme::S3);
    assert_eq!(url.bucket, "bucket");
    assert_eq!(url.key, "key");
}

#[test]
fn parse_s3_url_no_key() {
    let url = ObjectUrl::parse("s3://bucket/").unwrap();
    assert_eq!(url.bucket, "bucket");
    assert_eq!(url.key, "");
}

#[test]
fn parse_gs_url() {
    let url = ObjectUrl::parse("gs://gcs-bucket/data/raster.zarr").unwrap();
    assert_eq!(url.scheme, CloudScheme::Gs);
    assert_eq!(url.bucket, "gcs-bucket");
    assert_eq!(url.key, "data/raster.zarr");
}

#[test]
fn parse_az_url() {
    let url = ObjectUrl::parse("az://mycontainer/blobs/file.nc").unwrap();
    assert_eq!(url.scheme, CloudScheme::Az);
    assert_eq!(url.bucket, "mycontainer");
    assert_eq!(url.key, "blobs/file.nc");
}

#[test]
fn parse_abfs_url() {
    let url = ObjectUrl::parse("abfs://container/path/file.tif").unwrap();
    assert_eq!(url.scheme, CloudScheme::Az);
    assert_eq!(url.bucket, "container");
}

#[test]
fn parse_https_url() {
    let url = ObjectUrl::parse("https://example.com/bucket/key.tiff").unwrap();
    assert_eq!(url.scheme, CloudScheme::Https);
    assert_eq!(url.bucket, "example.com");
    assert_eq!(url.key, "bucket/key.tiff");
}

#[test]
fn parse_http_url() {
    let url = ObjectUrl::parse("http://minio.local:9000/data/file.nc").unwrap();
    assert_eq!(url.scheme, CloudScheme::Http);
    assert_eq!(url.bucket, "minio.local:9000");
    assert_eq!(url.key, "data/file.nc");
}

#[test]
fn parse_url_uppercase_scheme_normalized() {
    // Scheme comparison is case-insensitive
    let url = ObjectUrl::parse("S3://bucket/key").unwrap();
    assert_eq!(url.scheme, CloudScheme::S3);
}

// ─────────────────────────────────────────────────────────────────────────────
// ObjectUrl parsing — invalid
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn parse_invalid_no_scheme() {
    let err = ObjectUrl::parse("bucket/key").unwrap_err();
    assert!(matches!(err, CloudError::InvalidUrl(_)));
}

#[test]
fn parse_unsupported_scheme() {
    let err = ObjectUrl::parse("ftp://bucket/key").unwrap_err();
    assert!(matches!(err, CloudError::UnsupportedScheme(_)));
}

#[test]
fn parse_empty_bucket_s3() {
    let err = ObjectUrl::parse("s3:///key").unwrap_err();
    assert!(matches!(err, CloudError::InvalidUrl(_)));
}

#[test]
fn parse_empty_host_https() {
    let err = ObjectUrl::parse("https:///path").unwrap_err();
    assert!(matches!(err, CloudError::InvalidUrl(_)));
}

// ─────────────────────────────────────────────────────────────────────────────
// ObjectUrl::to_https_url
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn to_https_url_s3_default_region() {
    let mut url = ObjectUrl::parse("s3://my-bucket/data/file.tiff").unwrap();
    url.region = None;
    let https = url.to_https_url(None);
    assert!(https.starts_with("https://my-bucket.s3.us-east-1.amazonaws.com/"));
    assert!(https.ends_with("data/file.tiff"));
}

#[test]
fn to_https_url_s3_custom_region() {
    let mut url = ObjectUrl::parse("s3://my-bucket/data/file.tiff").unwrap();
    url.region = Some("eu-west-1".to_owned());
    let https = url.to_https_url(None);
    assert!(https.contains("eu-west-1.amazonaws.com"));
}

#[test]
fn to_https_url_gs() {
    let url = ObjectUrl::parse("gs://gcs-bucket/path/data.zarr").unwrap();
    let https = url.to_https_url(None);
    assert_eq!(
        https,
        "https://storage.googleapis.com/gcs-bucket/path/data.zarr"
    );
}

#[test]
fn to_https_url_az() {
    let url = ObjectUrl::parse("az://mycontainer/blob.nc").unwrap();
    let https = url.to_https_url(None);
    assert!(https.starts_with("https://mycontainer.blob.core.windows.net/"));
}

#[test]
fn to_https_url_with_endpoint_override() {
    let url = ObjectUrl::parse("s3://bucket/key.tiff").unwrap();
    let https = url.to_https_url(Some("http://minio.local:9000"));
    assert_eq!(https, "http://minio.local:9000/bucket/key.tiff");
}

#[test]
fn to_https_url_http_upgraded() {
    let url = ObjectUrl::parse("http://example.com/path/file.tif").unwrap();
    let https = url.to_https_url(None);
    assert!(https.starts_with("https://"));
}

// ─────────────────────────────────────────────────────────────────────────────
// ByteRangeRequest
// ─────────────────────────────────────────────────────────────────────────────

fn make_s3_url() -> ObjectUrl {
    ObjectUrl::parse("s3://test-bucket/object.tiff").unwrap()
}

#[test]
fn byte_range_http_header_basic() {
    let req = ByteRangeRequest::new(make_s3_url(), 0, 1024);
    assert_eq!(req.to_http_range_header(), "bytes=0-1023");
}

#[test]
fn byte_range_http_header_mid_object() {
    let req = ByteRangeRequest::new(make_s3_url(), 4096, 8192);
    assert_eq!(req.to_http_range_header(), "bytes=4096-8191");
}

#[test]
fn byte_range_length() {
    let req = ByteRangeRequest::new(make_s3_url(), 100, 200);
    assert_eq!(req.length(), 100);
}

#[test]
fn byte_range_length_zero() {
    let req = ByteRangeRequest::new(make_s3_url(), 50, 50);
    assert_eq!(req.length(), 0);
}

#[test]
fn byte_range_single_byte() {
    let req = ByteRangeRequest::new(make_s3_url(), 42, 43);
    assert_eq!(req.to_http_range_header(), "bytes=42-42");
    assert_eq!(req.length(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudRangeCoalescer
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn coalescer_empty_input() {
    let c = CloudRangeCoalescer::new();
    let result = c.coalesce(vec![]);
    assert!(result.is_empty());
}

#[test]
fn coalescer_single_range_unchanged() {
    let c = CloudRangeCoalescer::new();
    let ranges = vec![ByteRangeRequest::new(make_s3_url(), 0, 1024)];
    let result = c.coalesce(ranges);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].range, 0..1024);
}

#[test]
fn coalescer_adjacent_ranges_merged() {
    let c = CloudRangeCoalescer::new();
    let ranges = vec![
        ByteRangeRequest::new(make_s3_url(), 0, 1024),
        ByteRangeRequest::new(make_s3_url(), 1024, 2048),
    ];
    let result = c.coalesce(ranges);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].range, 0..2048);
}

#[test]
fn coalescer_small_gap_merged() {
    let c = CloudRangeCoalescer::new();
    // Gap of 100 bytes << 512 KiB threshold → should merge
    let ranges = vec![
        ByteRangeRequest::new(make_s3_url(), 0, 1000),
        ByteRangeRequest::new(make_s3_url(), 1100, 2000),
    ];
    let result = c.coalesce(ranges);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].range, 0..2000);
}

#[test]
fn coalescer_large_gap_not_merged() {
    let c = CloudRangeCoalescer::new();
    // Gap of 1 MiB > 512 KiB threshold → should NOT merge
    let gap = 1024 * 1024;
    let ranges = vec![
        ByteRangeRequest::new(make_s3_url(), 0, 1000),
        ByteRangeRequest::new(make_s3_url(), 1000 + gap, 2000 + gap),
    ];
    let result = c.coalesce(ranges);
    assert_eq!(result.len(), 2);
}

#[test]
fn coalescer_max_request_size_not_exceeded() {
    let c = CloudRangeCoalescer::new();
    // Two ranges whose merged size would exceed 8 MiB
    let eight_mb = 8 * 1024 * 1024;
    let ranges = vec![
        ByteRangeRequest::new(make_s3_url(), 0, eight_mb),
        ByteRangeRequest::new(make_s3_url(), eight_mb, eight_mb * 2),
    ];
    let result = c.coalesce(ranges);
    // They should NOT be merged because new_size = 16 MiB > 8 MiB
    assert_eq!(result.len(), 2);
}

#[test]
fn coalescer_sort_unsorted_input() {
    let c = CloudRangeCoalescer::new();
    // Input is in reverse order — should be sorted then merged
    let ranges = vec![
        ByteRangeRequest::new(make_s3_url(), 2000, 3000),
        ByteRangeRequest::new(make_s3_url(), 0, 1000),
        ByteRangeRequest::new(make_s3_url(), 1000, 2000),
    ];
    let result = c.coalesce(ranges);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].range, 0..3000);
}

#[test]
fn coalescer_overlapping_ranges_merged() {
    let c = CloudRangeCoalescer::new();
    let ranges = vec![
        ByteRangeRequest::new(make_s3_url(), 0, 500),
        ByteRangeRequest::new(make_s3_url(), 300, 800),
    ];
    let result = c.coalesce(ranges);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].range.start, 0);
    assert_eq!(result[0].range.end, 800);
}

#[test]
fn coalescer_slice_response_start() {
    let data = vec![0u8; 200];
    let slice = CloudRangeCoalescer::slice_response(&data, 100, &(100..150));
    assert_eq!(slice.len(), 50);
}

#[test]
fn coalescer_slice_response_middle() {
    let data: Vec<u8> = (0..100).collect();
    let slice = CloudRangeCoalescer::slice_response(&data, 1000, &(1010..1020));
    assert_eq!(slice.len(), 10);
    // bytes at offset 10..20 in data
    assert_eq!(slice, &data[10..20]);
}

#[test]
fn coalescer_slice_response_full_range() {
    let data = vec![42u8; 512];
    let slice = CloudRangeCoalescer::slice_response(&data, 0, &(0..512));
    assert_eq!(slice.len(), 512);
    assert!(slice.iter().all(|&b| b == 42));
}

// ─────────────────────────────────────────────────────────────────────────────
// SHA-256 NIST test vectors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn sha256_empty_string() {
    let hash = sha256(b"");
    assert_eq!(
        hex_encode(&hash),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_abc() {
    let hash = sha256(b"abc");
    assert_eq!(
        hex_encode(&hash),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_448_bit_message() {
    // NIST FIPS 180-4 example: "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
    let hash = sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
    assert_eq!(
        hex_encode(&hash),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn sha256_known_greeting() {
    // "The quick brown fox jumps over the lazy dog"
    let hash = sha256(b"The quick brown fox jumps over the lazy dog");
    assert_eq!(
        hex_encode(&hash),
        "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// HMAC-SHA256 test vectors (RFC 4231 / wikipedia)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn hmac_sha256_quick_brown_fox() {
    let mac = hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog");
    assert_eq!(
        hex_encode(&mac),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
    );
}

#[test]
fn hmac_sha256_empty_message() {
    // HMAC-SHA256("key", "") — known value
    let mac = hmac_sha256(b"key", b"");
    // Verify it is 32 bytes
    assert_eq!(mac.len(), 32);
    // Value should be non-zero
    assert!(mac.iter().any(|&b| b != 0));
}

#[test]
fn hmac_sha256_long_key_hashed() {
    // Keys longer than 64 bytes should be hashed first; verify the result is 32 bytes.
    let key = vec![0xaau8; 131];
    let mac = hmac_sha256(&key, b"Test With a key longer than the block size");
    assert_eq!(mac.len(), 32);
}

#[test]
fn hmac_sha256_rfc4231_vector1() {
    // RFC 4231 test case 1:
    // Key = 0x0b*20, Data = "Hi There"
    let key = vec![0x0bu8; 20];
    let mac = hmac_sha256(&key, b"Hi There");
    assert_eq!(
        hex_encode(&mac),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// PresignedUrlGenerator
// ─────────────────────────────────────────────────────────────────────────────

fn make_generator() -> PresignedUrlGenerator {
    PresignedUrlGenerator::new(
        CloudCredentials::AccessKey {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_owned(),
            session_token: None,
        },
        "us-east-1",
    )
}

#[test]
fn presign_s3_contains_algorithm() {
    let generator = make_generator();
    let url = ObjectUrl::parse("s3://examplebucket/test.txt").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let signed = generator.generate_s3(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"),
        "missing algorithm: {signed}"
    );
}

#[test]
fn presign_s3_contains_credential() {
    let generator = make_generator();
    let url = ObjectUrl::parse("s3://examplebucket/test.txt").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let signed = generator.generate_s3(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.contains("X-Amz-Credential="),
        "missing credential: {signed}"
    );
}

#[test]
fn presign_s3_contains_expires() {
    let generator = make_generator();
    let url = ObjectUrl::parse("s3://examplebucket/test.txt").unwrap();
    let cfg = PresignedUrlConfig::get(7200);
    let signed = generator.generate_s3(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.contains("X-Amz-Expires=7200"),
        "missing expires: {signed}"
    );
}

#[test]
fn presign_s3_contains_signature() {
    let generator = make_generator();
    let url = ObjectUrl::parse("s3://examplebucket/test.txt").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let signed = generator.generate_s3(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.contains("X-Amz-Signature="),
        "missing signature: {signed}"
    );
}

#[test]
fn presign_s3_contains_date() {
    let generator = make_generator();
    let url = ObjectUrl::parse("s3://examplebucket/test.txt").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    // Timestamp 1609459200 = 2021-01-01 00:00:00 UTC
    let signed = generator.generate_s3(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.contains("X-Amz-Date=20210101T000000Z"),
        "missing date: {signed}"
    );
}

#[test]
fn presign_s3_host_in_url() {
    let generator = make_generator();
    let url = ObjectUrl::parse("s3://examplebucket/test.txt").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let signed = generator.generate_s3(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.starts_with("https://examplebucket.s3.us-east-1.amazonaws.com/"),
        "wrong host: {signed}"
    );
}

#[test]
fn presign_s3_missing_credentials_error() {
    let generator = PresignedUrlGenerator::new(CloudCredentials::Anonymous, "us-east-1");
    let url = ObjectUrl::parse("s3://bucket/key").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let err = generator.generate_s3(&url, &cfg, 0).unwrap_err();
    assert!(matches!(err, CloudError::MissingCredentials));
}

#[test]
fn presign_gcs_contains_goog_algorithm() {
    let generator = make_generator();
    let url = ObjectUrl::parse("gs://my-bucket/data/raster.tiff").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let signed = generator.generate_gcs(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.contains("X-Goog-Algorithm=GOOG4-HMAC-SHA256"),
        "missing GCS algorithm: {signed}"
    );
}

#[test]
fn presign_gcs_starts_with_storage_googleapis() {
    let generator = make_generator();
    let url = ObjectUrl::parse("gs://my-bucket/data/raster.tiff").unwrap();
    let cfg = PresignedUrlConfig::get(3600);
    let signed = generator.generate_gcs(&url, &cfg, 1_609_459_200).unwrap();
    assert!(
        signed.starts_with("https://storage.googleapis.com/"),
        "wrong base URL: {signed}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// PresignedUrlConfig constructors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn presigned_url_config_get() {
    let cfg = PresignedUrlConfig::get(3600);
    assert_eq!(cfg.method, HttpMethod::Get);
    assert_eq!(cfg.expires_in_secs, 3600);
    assert!(cfg.content_type.is_none());
}

#[test]
fn presigned_url_config_put() {
    let cfg = PresignedUrlConfig::put(900, "application/octet-stream");
    assert_eq!(cfg.method, HttpMethod::Put);
    assert_eq!(cfg.expires_in_secs, 900);
    assert_eq!(
        cfg.content_type.as_deref(),
        Some("application/octet-stream")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// MultipartUploadState
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn multipart_upload_new() {
    let url = ObjectUrl::parse("s3://bucket/large.tiff").unwrap();
    let state = MultipartUploadState::new("upload-id-abc", url, 5 * 1024 * 1024);
    assert_eq!(state.upload_id, "upload-id-abc");
    assert_eq!(state.part_count(), 0);
    assert_eq!(state.total_size(), 0);
}

#[test]
fn multipart_upload_add_parts() {
    let url = ObjectUrl::parse("s3://bucket/large.tiff").unwrap();
    let mut state = MultipartUploadState::new("uid", url, 5 * 1024 * 1024);
    state.add_part(1, "\"etag-part1\"", 5 * 1024 * 1024);
    state.add_part(2, "\"etag-part2\"", 3 * 1024 * 1024);
    assert_eq!(state.part_count(), 2);
    assert_eq!(state.total_size(), 8 * 1024 * 1024);
}

#[test]
fn multipart_upload_to_xml_tags() {
    let url = ObjectUrl::parse("s3://bucket/large.tiff").unwrap();
    let mut state = MultipartUploadState::new("uid", url, 5 * 1024 * 1024);
    state.add_part(1, "\"etag1\"", 1024);
    state.add_part(2, "\"etag2\"", 2048);
    let xml = state.to_xml();
    assert!(
        xml.contains("<CompleteMultipartUpload>"),
        "missing root: {xml}"
    );
    assert!(xml.contains("<Part>"), "missing Part: {xml}");
    assert!(
        xml.contains("<PartNumber>1</PartNumber>"),
        "missing PartNumber 1: {xml}"
    );
    assert!(
        xml.contains("<PartNumber>2</PartNumber>"),
        "missing PartNumber 2: {xml}"
    );
    assert!(
        xml.contains("<ETag>\"etag1\"</ETag>"),
        "missing ETag 1: {xml}"
    );
    assert!(
        xml.contains("</CompleteMultipartUpload>"),
        "missing closing tag: {xml}"
    );
}

#[test]
fn multipart_upload_xml_sorted_by_part_number() {
    let url = ObjectUrl::parse("s3://bucket/large.tiff").unwrap();
    let mut state = MultipartUploadState::new("uid", url, 5 * 1024 * 1024);
    // Add parts out of order
    state.add_part(3, "\"etag3\"", 100);
    state.add_part(1, "\"etag1\"", 100);
    state.add_part(2, "\"etag2\"", 100);
    let xml = state.to_xml();
    let pos1 = xml.find("<PartNumber>1</PartNumber>").unwrap();
    let pos2 = xml.find("<PartNumber>2</PartNumber>").unwrap();
    let pos3 = xml.find("<PartNumber>3</PartNumber>").unwrap();
    assert!(pos1 < pos2);
    assert!(pos2 < pos3);
}

#[test]
fn completed_part_fields() {
    let part = CompletedPart {
        part_number: 5,
        etag: "\"abc123\"".to_owned(),
        size: 1024 * 1024,
    };
    assert_eq!(part.part_number, 5);
    assert_eq!(part.etag, "\"abc123\"");
    assert_eq!(part.size, 1024 * 1024);
}

// ─────────────────────────────────────────────────────────────────────────────
// RetryPolicy
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn retry_policy_default_values() {
    let policy = RetryPolicy::new();
    assert_eq!(policy.max_attempts, 3);
    assert_eq!(policy.initial_delay_ms, 100);
    assert_eq!(policy.max_delay_ms, 30_000);
    assert!((policy.backoff_multiplier - 2.0).abs() < f64::EPSILON);
}

#[test]
fn retry_policy_no_retry() {
    let policy = RetryPolicy::no_retry();
    assert_eq!(policy.max_attempts, 1);
    assert_eq!(policy.delay_for_attempt(0).as_millis(), 0);
}

#[test]
fn retry_policy_aggressive() {
    let policy = RetryPolicy::aggressive();
    assert_eq!(policy.max_attempts, 5);
    assert_eq!(policy.initial_delay_ms, 50);
    assert_eq!(policy.max_delay_ms, 60_000);
}

#[test]
fn retry_policy_delay_increases() {
    let policy = RetryPolicy::new();
    let d0 = policy.delay_for_attempt(0).as_millis();
    let d1 = policy.delay_for_attempt(1).as_millis();
    let d2 = policy.delay_for_attempt(2).as_millis();
    // Each delay should generally be larger due to exponential back-off.
    // Allow for jitter but expect a clear upward trend.
    assert!(d1 > d0, "d1={d1} should be > d0={d0}");
    assert!(d2 >= d1, "d2={d2} should be >= d1={d1}");
}

#[test]
fn retry_policy_delay_capped_at_max() {
    let policy = RetryPolicy::new();
    // After many attempts the delay should not exceed max_delay_ms by much.
    for attempt in 10..20 {
        let d = policy.delay_for_attempt(attempt).as_millis() as u64;
        // Allow up to 1.5× max_delay_ms due to jitter
        assert!(
            d <= policy.max_delay_ms + policy.max_delay_ms / 2,
            "delay {d} ms too large at attempt {attempt}"
        );
    }
}

#[test]
fn retry_policy_is_retryable() {
    let range_err = CloudError::RangeOutOfBounds {
        start: 0,
        end: 100,
        size: 50,
    };
    assert!(RetryPolicy::is_retryable(&range_err));
}

#[test]
fn retry_policy_invalid_url_not_retryable() {
    let err = CloudError::InvalidUrl("bad".to_owned());
    assert!(!RetryPolicy::is_retryable(&err));
}

// ─────────────────────────────────────────────────────────────────────────────
// RetryState
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn retry_state_initial_attempt_zero() {
    let state = RetryState::new(RetryPolicy::new());
    assert_eq!(state.attempt(), 0);
}

#[test]
fn retry_state_exhausts_after_max_attempts() {
    let policy = RetryPolicy::new(); // 3 attempts
    let mut state = RetryState::new(policy);
    // We should get Some for 3 calls, then None.
    assert!(state.next_delay().is_some());
    assert!(state.next_delay().is_some());
    assert!(state.next_delay().is_some());
    assert!(state.next_delay().is_none());
}

#[test]
fn retry_state_no_retry_exhausts_immediately() {
    let mut state = RetryState::new(RetryPolicy::no_retry());
    // max_attempts = 1 → first call exhausts
    assert!(state.next_delay().is_some());
    assert!(state.next_delay().is_none());
}

#[test]
fn retry_state_should_retry_true() {
    let policy = RetryPolicy {
        max_attempts: 5,
        ..RetryPolicy::new()
    };
    let state = RetryState::new(policy);
    let err = CloudError::RangeOutOfBounds {
        start: 0,
        end: 10,
        size: 5,
    };
    assert!(state.should_retry(&err));
}

#[test]
fn retry_state_should_retry_false_non_retryable() {
    let state = RetryState::new(RetryPolicy::new());
    let err = CloudError::MissingCredentials;
    assert!(!state.should_retry(&err));
}

#[test]
fn retry_state_attempt_counter_increments() {
    let mut state = RetryState::new(RetryPolicy::new());
    assert_eq!(state.attempt(), 0);
    state.next_delay();
    assert_eq!(state.attempt(), 1);
    state.next_delay();
    assert_eq!(state.attempt(), 2);
}
