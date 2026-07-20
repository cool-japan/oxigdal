//! STAC API Item Search request and response models.
//!
//! These types model the STAC API Item Search endpoint as defined at
//! <https://api.stacspec.org/v1.0.0/item-search>.

use serde::{Deserialize, Serialize};

/// A link with relation, href, optional media-type, and optional title.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Link {
    /// Relation type (e.g., `"self"`, `"next"`, `"root"`).
    pub rel: String,
    /// Target URL.
    pub href: String,
    /// Media type of the linked resource.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub link_type: Option<String>,
    /// Human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Link {
    /// Creates a link with only `rel` and `href`.
    pub fn new(rel: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            rel: rel.into(),
            href: href.into(),
            link_type: None,
            title: None,
        }
    }

    /// Adds a media type.
    pub fn with_type(mut self, media_type: impl Into<String>) -> Self {
        self.link_type = Some(media_type.into());
        self
    }

    /// Adds a human-readable title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Sort direction for search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending order (smallest first).
    Asc,
    /// Descending order (largest first).
    Desc,
}

/// A sort field specifying a property and direction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SortField {
    /// Name of the property to sort by.
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

impl SortField {
    /// Creates a sort field.
    pub fn new(field: impl Into<String>, direction: SortDirection) -> Self {
        Self {
            field: field.into(),
            direction,
        }
    }
}

/// Field inclusion / exclusion specification for the `fields` extension.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FieldsSpec {
    /// Properties to include in the response (dot-notation supported).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Properties to exclude from the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

impl FieldsSpec {
    /// Creates a new, empty [`FieldsSpec`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a property to include.
    pub fn include(mut self, field: impl Into<String>) -> Self {
        self.include.push(field.into());
        self
    }

    /// Adds a property to exclude.
    pub fn exclude(mut self, field: impl Into<String>) -> Self {
        self.exclude.push(field.into());
        self
    }
}

/// STAC API search request (POST body or GET query parameters).
///
/// All fields are optional; an empty request returns all items up to the
/// default page size.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SearchRequest {
    /// Bounding box filter `[west, south, east, north]` in WGS 84.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[f64; 4]>,

    /// RFC 3339 datetime or interval `"2020-01-01T00:00:00Z/2021-01-01T00:00:00Z"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime: Option<String>,

    /// CQL2-JSON or CQL2-Text filter expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,

    /// Filter language identifier (e.g., `"cql2-json"`).
    #[serde(rename = "filter-lang", skip_serializing_if = "Option::is_none")]
    pub filter_lang: Option<String>,

    /// Restrict results to items belonging to these collections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<String>,

    /// Restrict results to items with these IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,

    /// Maximum number of items to return (server may apply a lower cap).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Opaque pagination token returned by the previous page response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Sort specifications (evaluated in order).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sortby: Vec<SortField>,

    /// Field inclusion/exclusion specification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<FieldsSpec>,
}

impl SearchRequest {
    /// Creates a new, empty [`SearchRequest`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the bounding box filter.
    pub fn with_bbox(mut self, bbox: [f64; 4]) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Sets the datetime / interval filter.
    pub fn with_datetime(mut self, dt: impl Into<String>) -> Self {
        self.datetime = Some(dt.into());
        self
    }

    /// Restricts the search to the given collections.
    pub fn with_collections(mut self, cols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.collections = cols.into_iter().map(Into::into).collect();
        self
    }

    /// Restricts the search to the given item IDs.
    pub fn with_ids(mut self, ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.ids = ids.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the page size limit.
    pub fn with_limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }

    /// Appends a sort specification.
    pub fn with_sort(mut self, field: impl Into<String>, dir: SortDirection) -> Self {
        self.sortby.push(SortField::new(field, dir));
        self
    }

    /// Sets a CQL2-JSON filter.
    pub fn with_filter(mut self, filter: serde_json::Value) -> Self {
        self.filter = Some(filter);
        self.filter_lang = Some("cql2-json".to_string());
        self
    }

    /// Sets the fields specification.
    pub fn with_fields(mut self, fields: FieldsSpec) -> Self {
        self.fields = Some(fields);
        self
    }

    /// Sets the pagination token.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }
}

/// Context metadata included in a paginated item collection response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchContext {
    /// Number of items actually returned in this page.
    pub returned: u64,
    /// The limit that was requested (may differ from returned).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Total number of items that match the query (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<u64>,
}

/// A paginated GeoJSON `FeatureCollection` of STAC items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemCollection {
    /// Always `"FeatureCollection"`.
    #[serde(rename = "type")]
    pub collection_type: String,

    /// STAC items serialised as raw JSON values for flexibility.
    pub features: Vec<serde_json::Value>,

    /// Navigation links (e.g., `"next"`, `"prev"`, `"self"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Link>>,

    /// Context (pagination metadata).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<SearchContext>,

    /// Total items matching the query (OGC API – Features field).
    #[serde(rename = "numberMatched", skip_serializing_if = "Option::is_none")]
    pub number_matched: Option<u64>,

    /// Items returned in this page (OGC API – Features field).
    #[serde(rename = "numberReturned", skip_serializing_if = "Option::is_none")]
    pub number_returned: Option<u64>,
}

impl ItemCollection {
    /// Creates a new item collection with the given features.
    pub fn new(features: Vec<serde_json::Value>) -> Self {
        let n = features.len() as u64;
        Self {
            collection_type: "FeatureCollection".to_string(),
            features,
            links: None,
            context: None,
            number_matched: None,
            number_returned: Some(n),
        }
    }

    /// Returns the next-page link, if present.
    pub fn next_link(&self) -> Option<&Link> {
        self.links
            .as_ref()
            .and_then(|ls| ls.iter().find(|l| l.rel == "next"))
    }

    /// Returns `true` if a `"next"` link is present.
    pub fn has_next_page(&self) -> bool {
        self.next_link().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_request_default() {
        let req = SearchRequest::new();
        assert!(req.bbox.is_none());
        assert!(req.collections.is_empty());
    }

    #[test]
    fn test_with_bbox() {
        let req = SearchRequest::new().with_bbox([-180.0, -90.0, 180.0, 90.0]);
        assert_eq!(req.bbox, Some([-180.0, -90.0, 180.0, 90.0]));
    }

    #[test]
    fn test_with_collections() {
        let req = SearchRequest::new().with_collections(["sentinel-2-l2a"]);
        assert_eq!(req.collections, vec!["sentinel-2-l2a"]);
    }

    #[test]
    fn test_with_sort() {
        let req = SearchRequest::new().with_sort("datetime", SortDirection::Desc);
        assert_eq!(req.sortby.len(), 1);
        assert_eq!(req.sortby[0].direction, SortDirection::Desc);
    }

    #[test]
    fn test_search_request_roundtrip() {
        let req = SearchRequest::new()
            .with_bbox([-10.0, -10.0, 10.0, 10.0])
            .with_datetime("2023-01-01/2024-01-01")
            .with_collections(["my-collection"])
            .with_limit(50);
        let json = serde_json::to_string(&req).expect("serialize");
        let back: SearchRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(req, back);
    }

    #[test]
    fn test_item_collection_new() {
        let fc = ItemCollection::new(vec![]);
        assert_eq!(fc.collection_type, "FeatureCollection");
        assert_eq!(fc.number_returned, Some(0));
    }

    #[test]
    fn test_has_next_page() {
        let mut fc = ItemCollection::new(vec![]);
        assert!(!fc.has_next_page());
        fc.links = Some(vec![Link::new("next", "https://example.com?token=abc")]);
        assert!(fc.has_next_page());
    }

    #[test]
    fn test_fields_spec() {
        let fs = FieldsSpec::new()
            .include("properties.datetime")
            .exclude("assets");
        assert_eq!(fs.include, vec!["properties.datetime"]);
        assert_eq!(fs.exclude, vec!["assets"]);
    }
}
