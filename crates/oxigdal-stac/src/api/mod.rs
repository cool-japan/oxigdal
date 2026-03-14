//! STAC API resource types.
//!
//! This module groups the types that model STAC API HTTP request / response
//! payloads, conformance classes, and OGC API – Features resources.
//!
//! # Modules
//!
//! - [`conformance`] — Conformance declaration (`GET /conformance`)
//! - [`search`] — Item Search request / response types
//! - [`features`] — OGC API – Features landing page and collection list

pub mod conformance;
pub mod features;
pub mod search;

pub use conformance::ConformanceDeclaration;
pub use features::{CollectionSummary, CollectionsList, LandingPage};
pub use search::{
    FieldsSpec, ItemCollection, Link, SearchContext, SearchRequest, SortDirection, SortField,
};
