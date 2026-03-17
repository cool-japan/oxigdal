//! Cloud-native I/O abstractions for OxiGDAL streaming.
//!
//! This module provides pure-Rust abstractions for cloud object storage:
//! - Parsed cloud URLs (S3, GCS, Azure, HTTP/HTTPS)
//! - Byte-range request building and HTTP Range header generation
//! - Object metadata representation
//! - Cloud credentials (anonymous, access key, service account, Azure, bearer)
//! - Presigned URL generation (AWS SigV4 / GCS v4 — pure-Rust HMAC-SHA256)
//! - Multipart upload state tracking and XML generation
//! - Range coalescing for efficient cloud reads
//! - Retry policy with exponential back-off and jitter

pub mod object_store;
pub mod retry;

pub use object_store::{
    ByteRangeRequest, CloudCredentials, CloudError, CloudRangeCoalescer, CloudScheme,
    CompletedPart, HttpMethod, MultipartUploadState, ObjectMetadata, ObjectUrl, PresignedUrlConfig,
    PresignedUrlGenerator, hex_encode, hmac_sha256, hmac_sha256_hex, sha256,
};
pub use retry::{RetryPolicy, RetryState};
