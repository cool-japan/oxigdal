//! Core data structures for the OGC Features API.

use serde::{Deserialize, Serialize};

use super::error::FeaturesError;

/// OGC link object used throughout the API responses
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Link {
    /// Target URI
    pub href: String,

    /// Link relation type (e.g. `self`, `next`, `alternate`)
    pub rel: String,

    /// MIME type of the target resource
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    /// Human-readable label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Language tag of the target resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hreflang: Option<String>,
}

impl Link {
    /// Construct a minimal link with href and rel.
    pub fn new(href: impl Into<String>, rel: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            rel: rel.into(),
            type_: None,
            title: None,
            hreflang: None,
        }
    }

    /// Set the MIME type.
    pub fn with_type(mut self, t: impl Into<String>) -> Self {
        self.type_ = Some(t.into());
        self
    }

    /// Set the human-readable title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Root landing page for the OGC Features API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingPage {
    /// Service title
    pub title: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Links to the API resources
    pub links: Vec<Link>,
}

/// OGC conformance declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceClasses {
    /// List of conformance class URIs
    pub conforms_to: Vec<String>,
}

impl ConformanceClasses {
    /// Return conformance classes for OGC API - Features Part 1 (Core).
    pub fn ogc_features_core() -> Self {
        Self {
            conforms_to: vec![
                "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core".to_string(),
                "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/oas30".to_string(),
                "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/html".to_string(),
                "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/geojson".to_string(),
            ],
        }
    }

    /// Return conformance classes for Part 1 + Part 2 (CRS).
    pub fn with_crs() -> Self {
        let mut base = Self::ogc_features_core();
        base.conforms_to
            .push("http://www.opengis.net/spec/ogcapi-features-2/1.0/conf/crs".to_string());
        base
    }
}

/// Spatial extent of a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialExtent {
    /// One or more bounding boxes `[xmin, ymin, xmax, ymax]`
    pub bbox: Vec<[f64; 4]>,

    /// CRS URI for the bbox coordinates (defaults to CRS84)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
}

/// Temporal extent of a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalExtent {
    /// One or more intervals `[start, end]`; `null` means open-ended
    pub interval: Vec<[Option<String>; 2]>,

    /// Temporal reference system URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trs: Option<String>,
}

/// Combined spatial and temporal extent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extent {
    /// Spatial component
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialExtent>,

    /// Temporal component
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal: Option<TemporalExtent>,
}

/// Metadata about a single feature collection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    /// Unique identifier for the collection
    pub id: String,

    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Links to related resources
    pub links: Vec<Link>,

    /// Spatial/temporal extent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<Extent>,

    /// Type of items in the collection (usually `"feature"`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,

    /// List of supported CRS URIs (Part 2)
    #[serde(default)]
    pub crs: Vec<String>,

    /// Native/storage CRS URI (Part 2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_crs: Option<String>,
}

impl Collection {
    /// Create a minimal collection with an id.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: None,
            description: None,
            links: vec![],
            extent: None,
            item_type: Some("feature".to_string()),
            crs: vec![],
            storage_crs: None,
        }
    }
}

/// List of collections with navigational links
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collections {
    /// Top-level links (e.g. self)
    pub links: Vec<Link>,

    /// The actual collection metadata records
    pub collections: Vec<Collection>,
}

/// Feature identifier — may be a string or an integer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum FeatureId {
    /// String identifier
    String(String),
    /// Integer identifier
    Integer(i64),
}

/// A GeoJSON Feature with optional OGC links
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    /// Always `"Feature"`
    #[serde(rename = "type")]
    pub type_: String,

    /// Optional feature identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<FeatureId>,

    /// Geometry object (null if geometry-less)
    pub geometry: Option<serde_json::Value>,

    /// Properties object
    pub properties: Option<serde_json::Value>,

    /// OGC addition: links for this feature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Link>>,
}

impl Feature {
    /// Create a feature with default `type_` = `"Feature"`.
    pub fn new() -> Self {
        Self {
            type_: "Feature".to_string(),
            id: None,
            geometry: None,
            properties: None,
            links: None,
        }
    }
}

impl Default for Feature {
    fn default() -> Self {
        Self::new()
    }
}

/// A GeoJSON FeatureCollection with OGC pagination metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureCollection {
    /// Always `"FeatureCollection"`
    #[serde(rename = "type")]
    pub type_: String,

    /// Features in this page
    pub features: Vec<Feature>,

    /// OGC navigation links (next, prev, self, …)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Link>>,

    /// ISO 8601 timestamp when the response was generated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_stamp: Option<String>,

    /// Total number of features matching the query (before pagination)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_matched: Option<u64>,

    /// Number of features in this response page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_returned: Option<u64>,
}

impl FeatureCollection {
    /// Create an empty FeatureCollection.
    pub fn new() -> Self {
        Self {
            type_: "FeatureCollection".to_string(),
            features: vec![],
            links: None,
            time_stamp: None,
            number_matched: None,
            number_returned: None,
        }
    }
}

impl Default for FeatureCollection {
    fn default() -> Self {
        Self::new()
    }
}

/// Parsed datetime filter from the `datetime` query parameter
#[derive(Debug, Clone, PartialEq)]
pub enum DateTimeFilter {
    /// A single point in time: `"2021-04-22T00:00:00Z"`
    Instant(String),
    /// A time interval: `[start, end]` where `None` means open-ended
    Interval(Option<String>, Option<String>),
}

impl DateTimeFilter {
    /// Parse a datetime query parameter value into a `DateTimeFilter`.
    ///
    /// Accepted formats:
    /// - `"2021-04-22T00:00:00Z"` → `Instant`
    /// - `"../2021-01-01T00:00:00Z"` → `Interval(None, Some(...))`
    /// - `"2021-01-01T00:00:00Z/.."` → `Interval(Some(...), None)`
    /// - `"2021-01-01T00:00:00Z/2021-12-31T23:59:59Z"` → `Interval(Some, Some)`
    pub fn parse(s: &str) -> Result<Self, FeaturesError> {
        if s.is_empty() {
            return Err(FeaturesError::InvalidDatetime(
                "datetime value is empty".to_string(),
            ));
        }

        if let Some(slash_pos) = s.find('/') {
            let start_str = &s[..slash_pos];
            let end_str = &s[slash_pos + 1..];

            let start = if start_str == ".." || start_str.is_empty() {
                None
            } else {
                Some(start_str.to_string())
            };
            let end = if end_str == ".." || end_str.is_empty() {
                None
            } else {
                Some(end_str.to_string())
            };

            Ok(DateTimeFilter::Interval(start, end))
        } else {
            Ok(DateTimeFilter::Instant(s.to_string()))
        }
    }
}
