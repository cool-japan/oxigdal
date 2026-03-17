//! STAC API response types.
//!
//! Provides strongly-typed structs for every significant STAC API response
//! payload: landing page, collection, item, and item collection.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// StacLink
// ---------------------------------------------------------------------------

/// A STAC / OGC API hypermedia link.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StacLink {
    /// Target URL.
    pub href: String,

    /// Relation type (`"self"`, `"root"`, `"next"`, `"prev"`, …).
    pub rel: String,

    /// Media type of the linked resource.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    /// Human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// HTTP method hint (used in next/prev link objects).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// Body to send when following this link (POST pagination).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,

    /// Whether the body should be merged with the current request body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge: Option<bool>,
}

impl StacLink {
    /// Construct a minimal link with only `rel` and `href`.
    pub fn new(rel: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            rel: rel.into(),
            type_: None,
            title: None,
            method: None,
            body: None,
            merge: None,
        }
    }

    /// Add a media type.
    pub fn with_type(mut self, media_type: impl Into<String>) -> Self {
        self.type_ = Some(media_type.into());
        self
    }

    /// Add a human-readable title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Mark this link as using a specific HTTP method.
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }
}

// ---------------------------------------------------------------------------
// StacLandingPage
// ---------------------------------------------------------------------------

/// STAC API landing page (`GET /`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StacLandingPage {
    /// STAC specification version implemented by this endpoint.
    pub stac_version: String,

    /// URIs of STAC extensions used by this endpoint.
    #[serde(default)]
    pub stac_extensions: Vec<String>,

    /// Unique identifier for this API endpoint.
    pub id: String,

    /// Short human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description of this endpoint.
    pub description: String,

    /// Hypermedia links.
    #[serde(default)]
    pub links: Vec<StacLink>,

    /// OGC API conformance classes declared by this endpoint.
    #[serde(rename = "conformsTo", skip_serializing_if = "Option::is_none")]
    pub conformance_classes: Option<Vec<String>>,
}

impl StacLandingPage {
    /// Return the link with `rel = "conformance"`, if present.
    pub fn conformance_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "conformance")
    }

    /// Return the link with `rel = "data"` (collections list), if present.
    pub fn data_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "data")
    }
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// A data provider associated with a STAC collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    /// Provider name.
    pub name: String,

    /// Optional description of the provider's role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Provider roles (`"producer"`, `"licensor"`, `"processor"`, `"host"`).
    #[serde(default)]
    pub roles: Vec<String>,

    /// URL to the provider's website.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// ---------------------------------------------------------------------------
// CollectionExtent / SpatialExtent / TemporalExtent
// ---------------------------------------------------------------------------

/// Spatial extent of a STAC collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpatialExtent {
    /// One or more bounding boxes `[west, south, east, north]`.
    pub bbox: Vec<[f64; 4]>,
}

/// Temporal extent of a STAC collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemporalExtent {
    /// One or more `[start, end]` intervals (RFC 3339 strings or `null`).
    pub interval: Vec<[Option<String>; 2]>,
}

/// Spatio-temporal extent of a STAC collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionExtent {
    /// Spatial extent.
    pub spatial: SpatialExtent,

    /// Temporal extent.
    pub temporal: TemporalExtent,
}

// ---------------------------------------------------------------------------
// StacAsset
// ---------------------------------------------------------------------------

/// A STAC asset (file or resource) associated with an item or collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StacAsset {
    /// Direct URL to the asset.
    pub href: String,

    /// Human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Longer description of the asset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Media type of the asset.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    /// Semantic roles of this asset (`"data"`, `"thumbnail"`, `"overview"`, …).
    #[serde(default)]
    pub roles: Vec<String>,
}

impl StacAsset {
    /// Construct a minimal asset with only an `href`.
    pub fn new(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            title: None,
            description: None,
            type_: None,
            roles: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// StacCollection
// ---------------------------------------------------------------------------

/// A STAC Collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StacCollection {
    /// STAC version.
    pub stac_version: String,

    /// STAC extension URIs used by this collection.
    #[serde(default)]
    pub stac_extensions: Vec<String>,

    /// Unique collection identifier.
    pub id: String,

    /// Short human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Full description of the collection.
    pub description: String,

    /// Descriptive keywords.
    #[serde(default)]
    pub keywords: Vec<String>,

    /// SPDX license identifier or `"proprietary"` / `"various"`.
    pub license: String,

    /// Data providers.
    #[serde(default)]
    pub providers: Vec<Provider>,

    /// Spatio-temporal extent.
    pub extent: CollectionExtent,

    /// Additional summary properties (free-form).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summaries: Option<serde_json::Value>,

    /// Hypermedia links.
    #[serde(default)]
    pub links: Vec<StacLink>,

    /// Collection-level assets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<HashMap<String, StacAsset>>,
}

impl StacCollection {
    /// Return the link with `rel = "items"` (the items endpoint), if present.
    pub fn items_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "items")
    }

    /// Return the link with `rel = "self"`, if present.
    pub fn self_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "self")
    }

    /// Return the overall spatial bounding box (first element of `spatial.bbox`).
    pub fn bbox(&self) -> Option<[f64; 4]> {
        self.extent.spatial.bbox.first().copied()
    }
}

// ---------------------------------------------------------------------------
// StacItem
// ---------------------------------------------------------------------------

/// A STAC Item (GeoJSON Feature with STAC metadata).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StacItem {
    /// STAC version.
    pub stac_version: String,

    /// STAC extension URIs used by this item.
    #[serde(default)]
    pub stac_extensions: Vec<String>,

    /// Unique item identifier.
    pub id: String,

    /// GeoJSON type — always `"Feature"`.
    #[serde(rename = "type")]
    pub type_: String,

    /// Item geometry (GeoJSON) or `null`.
    pub geometry: Option<serde_json::Value>,

    /// Minimum bounding rectangle `[west, south, east, north]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[f64; 4]>,

    /// Item properties object (must contain at least `datetime`).
    pub properties: serde_json::Value,

    /// Hypermedia links.
    #[serde(default)]
    pub links: Vec<StacLink>,

    /// Item assets keyed by role/identifier.
    #[serde(default)]
    pub assets: HashMap<String, StacAsset>,

    /// Identifier of the parent collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
}

impl StacItem {
    /// Extract the `datetime` property value (RFC 3339 string).
    pub fn datetime(&self) -> Option<&str> {
        self.properties.get("datetime").and_then(|v| v.as_str())
    }

    /// Extract the `eo:cloud_cover` property.
    pub fn cloud_cover(&self) -> Option<f64> {
        self.properties
            .get("eo:cloud_cover")
            .and_then(|v| v.as_f64())
    }

    /// Extract the `platform` property.
    pub fn platform(&self) -> Option<&str> {
        self.properties.get("platform").and_then(|v| v.as_str())
    }

    /// Extract the `constellation` property.
    pub fn constellation(&self) -> Option<&str> {
        self.properties
            .get("constellation")
            .and_then(|v| v.as_str())
    }

    /// Extract and deserialize an arbitrary property value.
    pub fn get_property<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.properties
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Return the link with `rel = "self"`, if present.
    pub fn self_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "self")
    }

    /// Return the link with `rel = "collection"`, if present.
    pub fn collection_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "collection")
    }
}

// ---------------------------------------------------------------------------
// SearchContext
// ---------------------------------------------------------------------------

/// Pagination context metadata embedded in an [`ItemCollection`] response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchContext {
    /// Current page number (1-indexed), if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,

    /// Page-size limit that was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Total items matching the query (if the server computed it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<u64>,

    /// Items returned in this page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned: Option<u64>,
}

// ---------------------------------------------------------------------------
// ItemCollection
// ---------------------------------------------------------------------------

/// A GeoJSON `FeatureCollection` of STAC items, as returned by `/search` and
/// `/collections/{id}/items`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemCollection {
    /// Always `"FeatureCollection"`.
    #[serde(rename = "type")]
    pub type_: String,

    /// The STAC items in this page.
    pub features: Vec<StacItem>,

    /// Hypermedia links (navigation).
    #[serde(default)]
    pub links: Vec<StacLink>,

    /// Optional pagination context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<SearchContext>,

    /// Total items matched (OGC API – Features field).
    #[serde(rename = "numberMatched", skip_serializing_if = "Option::is_none")]
    pub number_matched: Option<u64>,

    /// Items returned in this response (OGC API – Features field).
    #[serde(rename = "numberReturned", skip_serializing_if = "Option::is_none")]
    pub number_returned: Option<u64>,
}

impl ItemCollection {
    /// Construct an empty feature collection.
    pub fn empty() -> Self {
        Self {
            type_: "FeatureCollection".to_string(),
            features: Vec::new(),
            links: Vec::new(),
            context: None,
            number_matched: None,
            number_returned: Some(0),
        }
    }

    /// `true` if there are no features in this page.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Number of features in this page.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Return the opaque pagination token from the `"next"` link, if any.
    ///
    /// Looks for a `"next"` link that carries either a `token` query-parameter
    /// in its `href` or a `token` key inside its `body`.
    pub fn next_page_token(&self) -> Option<String> {
        let next_link = self.links.iter().find(|l| l.rel == "next")?;

        // Check body first (POST-based pagination).
        if let Some(body) = &next_link.body {
            if let Some(token) = body.get("token").and_then(|v| v.as_str()) {
                return Some(token.to_string());
            }
        }

        // Fall back to extracting `token=` from the href query string.
        extract_query_param(&next_link.href, "token")
    }

    /// Return the opaque pagination token from the `"prev"` link, if any.
    pub fn prev_page_token(&self) -> Option<String> {
        let prev_link = self.links.iter().find(|l| l.rel == "prev")?;

        if let Some(body) = &prev_link.body {
            if let Some(token) = body.get("token").and_then(|v| v.as_str()) {
                return Some(token.to_string());
            }
        }

        extract_query_param(&prev_link.href, "token")
    }

    /// Return the raw `"next"` link, if present.
    pub fn next_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "next")
    }

    /// Return the raw `"prev"` link, if present.
    pub fn prev_link(&self) -> Option<&StacLink> {
        self.links.iter().find(|l| l.rel == "prev")
    }
}

/// Extract the value of a named query parameter from a URL-like string.
fn extract_query_param(url: &str, key: &str) -> Option<String> {
    let qs = url.find('?').map(|i| &url[i + 1..])?;
    for part in qs.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_item() -> StacItem {
        StacItem {
            stac_version: "1.0.0".to_string(),
            stac_extensions: Vec::new(),
            id: "item-1".to_string(),
            type_: "Feature".to_string(),
            geometry: None,
            bbox: None,
            properties: serde_json::json!({
                "datetime": "2023-06-01T00:00:00Z",
                "platform": "sentinel-2a",
                "eo:cloud_cover": 12.5,
            }),
            links: Vec::new(),
            assets: HashMap::new(),
            collection: Some("sentinel-2-l2a".to_string()),
        }
    }

    #[test]
    fn test_stac_item_datetime() {
        let item = minimal_item();
        assert_eq!(item.datetime(), Some("2023-06-01T00:00:00Z"));
    }

    #[test]
    fn test_stac_item_cloud_cover() {
        let item = minimal_item();
        assert!((item.cloud_cover().expect("cloud cover") - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stac_item_platform() {
        let item = minimal_item();
        assert_eq!(item.platform(), Some("sentinel-2a"));
    }

    #[test]
    fn test_stac_item_get_property_f64() {
        let item = minimal_item();
        let cc: Option<f64> = item.get_property("eo:cloud_cover");
        assert!(cc.is_some());
        assert!((cc.expect("cloud cover property") - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_item_collection_next_token_from_href() {
        let mut ic = ItemCollection::empty();
        ic.links.push(StacLink::new(
            "next",
            "https://api.example.com/search?token=abc123",
        ));
        assert_eq!(ic.next_page_token(), Some("abc123".to_string()));
    }

    #[test]
    fn test_item_collection_next_token_from_body() {
        let mut ic = ItemCollection::empty();
        let mut link = StacLink::new("next", "https://api.example.com/search");
        link.body = Some(serde_json::json!({ "token": "cursor-xyz" }));
        ic.links.push(link);
        assert_eq!(ic.next_page_token(), Some("cursor-xyz".to_string()));
    }

    #[test]
    fn test_item_collection_len_and_empty() {
        let ic = ItemCollection::empty();
        assert!(ic.is_empty());
        assert_eq!(ic.len(), 0);
    }

    #[test]
    fn test_stac_link_builder() {
        let link = StacLink::new("self", "https://example.com")
            .with_type("application/json")
            .with_title("Self")
            .with_method("GET");
        assert_eq!(link.type_, Some("application/json".to_string()));
        assert_eq!(link.method, Some("GET".to_string()));
    }

    #[test]
    fn test_extract_query_param() {
        let url = "https://example.com/search?limit=10&token=abc&page=2";
        assert_eq!(extract_query_param(url, "token"), Some("abc".to_string()));
        assert_eq!(extract_query_param(url, "page"), Some("2".to_string()));
        assert_eq!(extract_query_param(url, "missing"), None);
    }
}
