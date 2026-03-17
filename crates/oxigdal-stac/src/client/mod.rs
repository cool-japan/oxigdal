//! Transport-agnostic STAC API client module.
//!
//! This module provides pure-Rust request/response types for the STAC API
//! specification without any HTTP library dependencies. Users wire their own
//! transport (reqwest, ureq, hyper, etc.) and pass raw JSON strings into the
//! parsing helpers.
//!
//! # Architecture
//!
//! - [`request`] — `StacApiRequest` enum + `SearchRequest` builder
//! - [`response`] — `StacLandingPage`, `StacCollection`, `StacItem`, `ItemCollection`
//! - [`pagination`] — `PaginationStrategy`, `PageIterator`, `StacApiClient`
//!
//! # Quick Start
//!
//! ```rust
//! use oxigdal_stac::client::{StacApiClient, SearchRequest, PaginationStrategy};
//!
//! let client = StacApiClient::new("https://earth-search.aws.element84.com/v1");
//!
//! // Build a search request
//! let req = SearchRequest::new()
//!     .with_bbox([-122.5, 37.5, -122.0, 38.0])
//!     .with_collections(vec!["sentinel-2-l2a".to_string()])
//!     .with_limit(10);
//!
//! // Get the URL users must fetch themselves
//! let url = client.search_url(&req);
//! println!("Fetch: POST {}", url);
//! ```

pub mod pagination;
pub mod request;
pub mod response;

pub use pagination::{PageIterator, PaginationStrategy, StacApiClient};
pub use request::{FieldsSpec, SearchRequest, SortDirection, SortField, StacApiRequest};
pub use response::{
    CollectionExtent, ItemCollection, Provider, SearchContext, SpatialExtent, StacAsset,
    StacCollection, StacItem, StacLandingPage, StacLink, TemporalExtent,
};

/// Error types for the STAC API client.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// JSON serialization / deserialization error.
    #[error("JSON error: {0}")]
    SerdeError(#[from] serde_json::Error),

    /// Malformed URL.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Invalid bounding box.
    #[error("Invalid bbox: {0}")]
    InvalidBbox(String),

    /// Invalid datetime string.
    #[error("Invalid datetime: {0}")]
    InvalidDatetime(String),

    /// Pagination logic error.
    #[error("Pagination error: {0}")]
    PaginationError(String),
}
