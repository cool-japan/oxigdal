//! STAC API request types.
//!
//! Provides the [`StacApiRequest`] enum that models every STAC API operation,
//! plus the [`SearchRequest`] builder for the Item Search endpoint.

use serde::{Deserialize, Serialize};

use super::ClientError;

// ---------------------------------------------------------------------------
// SortDirection
// ---------------------------------------------------------------------------

/// Direction of a sort field in a STAC search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending order (smallest value first).
    Ascending,
    /// Descending order (largest value first).
    Descending,
}

// ---------------------------------------------------------------------------
// SortField
// ---------------------------------------------------------------------------

/// A sort specification binding a property name to a sort direction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SortField {
    /// Property path to sort by (e.g. `"properties.datetime"`).
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

impl SortField {
    /// Construct a new sort field.
    pub fn new(field: impl Into<String>, direction: SortDirection) -> Self {
        Self {
            field: field.into(),
            direction,
        }
    }

    /// Convenience: ascending sort on `field`.
    pub fn asc(field: impl Into<String>) -> Self {
        Self::new(field, SortDirection::Ascending)
    }

    /// Convenience: descending sort on `field`.
    pub fn desc(field: impl Into<String>) -> Self {
        Self::new(field, SortDirection::Descending)
    }
}

// ---------------------------------------------------------------------------
// FieldsSpec
// ---------------------------------------------------------------------------

/// Field inclusion / exclusion specification for the STAC fields extension.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FieldsSpec {
    /// Properties to include in the response (dot-notation paths).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Properties to exclude from the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

impl FieldsSpec {
    /// Create an empty fields specification.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a field to include.
    pub fn include(mut self, field: impl Into<String>) -> Self {
        self.include.push(field.into());
        self
    }

    /// Add a field to exclude.
    pub fn exclude(mut self, field: impl Into<String>) -> Self {
        self.exclude.push(field.into());
        self
    }
}

// ---------------------------------------------------------------------------
// SearchRequest
// ---------------------------------------------------------------------------

/// A STAC API Item Search request.
///
/// Use the fluent builder methods to build up the request, then either
/// convert to JSON (for `POST /search`) or query parameters (for `GET /search`).
///
/// # Example
///
/// ```rust
/// use oxigdal_stac::client::SearchRequest;
///
/// let req = SearchRequest::new()
///     .with_bbox([-10.0, -10.0, 10.0, 10.0])
///     .with_collections(vec!["sentinel-2-l2a".to_string()])
///     .with_limit(50);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SearchRequest {
    /// Restrict to items with these IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,

    /// Restrict to items belonging to these collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<String>>,

    /// Bounding-box filter `[west, south, east, north]` in WGS 84.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[f64; 4]>,

    /// GeoJSON geometry for spatial intersection filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intersects: Option<serde_json::Value>,

    /// RFC 3339 datetime or interval (`"start/end"` or `"../end"` etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime: Option<String>,

    /// Maximum number of items to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Offset-based page number (1-indexed, server-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,

    /// Cursor-based pagination token (opaque, returned by previous response).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// CQL2-Text or CQL2-JSON filter expression (string form).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,

    /// Filter language identifier, e.g. `"cql2-text"` or `"cql2-json"`.
    #[serde(rename = "filter-lang", skip_serializing_if = "Option::is_none")]
    pub filter_lang: Option<String>,

    /// Sort specifications (evaluated in order).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<Vec<SortField>>,

    /// Field inclusion / exclusion specification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<FieldsSpec>,
}

impl SearchRequest {
    /// Create a new, empty search request.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by bounding box `[west, south, east, north]`.
    pub fn with_bbox(mut self, bbox: [f64; 4]) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Restrict to given collections.
    pub fn with_collections(mut self, collections: Vec<String>) -> Self {
        self.collections = Some(collections);
        self
    }

    /// Set the datetime / interval filter.
    pub fn with_datetime(mut self, datetime: impl Into<String>) -> Self {
        self.datetime = Some(datetime.into());
        self
    }

    /// Set the page-size limit.
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Filter by item IDs.
    pub fn with_ids(mut self, ids: Vec<String>) -> Self {
        self.ids = Some(ids);
        self
    }

    /// Set a GeoJSON intersection geometry.
    pub fn with_intersects(mut self, geometry: serde_json::Value) -> Self {
        self.intersects = Some(geometry);
        self
    }

    /// Set a CQL2 filter string.
    pub fn with_filter(mut self, filter: impl Into<String>, lang: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self.filter_lang = Some(lang.into());
        self
    }

    /// Append a sort specification.
    pub fn with_sort(mut self, sort_field: SortField) -> Self {
        self.sort_by.get_or_insert_with(Vec::new).push(sort_field);
        self
    }

    /// Set the fields inclusion/exclusion specification.
    pub fn with_fields(mut self, fields: FieldsSpec) -> Self {
        self.fields = Some(fields);
        self
    }

    /// Set the pagination token.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Set the page number (offset-based pagination).
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Serialize this request to a JSON string suitable for `POST /search`.
    pub fn to_json(&self) -> Result<String, ClientError> {
        serde_json::to_string(self).map_err(ClientError::SerdeError)
    }

    /// Deserialize a `SearchRequest` from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, ClientError> {
        serde_json::from_str(json).map_err(ClientError::SerdeError)
    }

    /// Build a list of `(key, value)` query parameters for `GET /search`.
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();

        if let Some(ref ids) = self.ids {
            if !ids.is_empty() {
                params.push(("ids".to_string(), ids.join(",")));
            }
        }
        if let Some(ref cols) = self.collections {
            if !cols.is_empty() {
                params.push(("collections".to_string(), cols.join(",")));
            }
        }
        if let Some(bbox) = self.bbox {
            params.push((
                "bbox".to_string(),
                format!("{},{},{},{}", bbox[0], bbox[1], bbox[2], bbox[3]),
            ));
        }
        if let Some(ref dt) = self.datetime {
            params.push(("datetime".to_string(), dt.clone()));
        }
        if let Some(limit) = self.limit {
            params.push(("limit".to_string(), limit.to_string()));
        }
        if let Some(page) = self.page {
            params.push(("page".to_string(), page.to_string()));
        }
        if let Some(ref token) = self.token {
            params.push(("token".to_string(), token.clone()));
        }
        if let Some(ref filter) = self.filter {
            params.push(("filter".to_string(), filter.clone()));
        }
        if let Some(ref lang) = self.filter_lang {
            params.push(("filter-lang".to_string(), lang.clone()));
        }
        if let Some(ref sorts) = self.sort_by {
            for sf in sorts {
                let dir = match sf.direction {
                    SortDirection::Ascending => "+",
                    SortDirection::Descending => "-",
                };
                params.push(("sortby".to_string(), format!("{}{}", dir, sf.field)));
            }
        }
        if let Some(ref fields) = self.fields {
            let mut parts: Vec<String> = fields.include.to_vec();
            parts.extend(fields.exclude.iter().map(|f| format!("-{}", f)));
            if !parts.is_empty() {
                params.push(("fields".to_string(), parts.join(",")));
            }
        }
        params
    }
}

// ---------------------------------------------------------------------------
// StacApiRequest
// ---------------------------------------------------------------------------

/// All STAC API operations as a single enum.
///
/// Each variant maps to one endpoint. Use [`StacApiRequest::method`],
/// [`StacApiRequest::path`], [`StacApiRequest::query_params`], and
/// [`StacApiRequest::body_json`] to build the HTTP request; use
/// [`StacApiRequest::to_url`] to build the full URL.
#[derive(Debug, Clone, PartialEq)]
pub enum StacApiRequest {
    /// `GET /` — landing page.
    GetLanding,

    /// `GET /conformance` — conformance declaration.
    GetConformance,

    /// `GET /collections` — list all collections.
    GetCollections {
        /// Optional page-size limit.
        limit: Option<u32>,
    },

    /// `GET /collections/{id}` — get a single collection.
    GetCollection {
        /// Collection identifier.
        id: String,
    },

    /// `GET /collections/{id}/items` — list items in a collection.
    GetItems {
        /// Collection identifier.
        collection_id: String,
        /// Optional page-size limit.
        limit: Option<u32>,
        /// Optional offset (0-based).
        offset: Option<u32>,
        /// Optional bounding-box filter.
        bbox: Option<[f64; 4]>,
        /// Optional datetime / interval filter.
        datetime: Option<String>,
        /// Optional property fields to include.
        fields: Option<Vec<String>>,
    },

    /// `GET /collections/{collection_id}/items/{item_id}` — get a single item.
    GetItem {
        /// Collection identifier.
        collection_id: String,
        /// Item identifier.
        item_id: String,
    },

    /// `POST /search` — item search with a JSON body.
    Search(SearchRequest),

    /// `GET /search` — item search using query parameters.
    SearchGet(SearchRequest),
}

impl StacApiRequest {
    /// HTTP method string (`"GET"` or `"POST"`).
    pub fn method(&self) -> &'static str {
        match self {
            Self::Search(_) => "POST",
            _ => "GET",
        }
    }

    /// URL path relative to the API base (no leading slash for base-relative joining).
    pub fn path(&self) -> String {
        match self {
            Self::GetLanding => String::new(),
            Self::GetConformance => "conformance".to_string(),
            Self::GetCollections { .. } => "collections".to_string(),
            Self::GetCollection { id } => format!("collections/{}", id),
            Self::GetItems { collection_id, .. } => {
                format!("collections/{}/items", collection_id)
            }
            Self::GetItem {
                collection_id,
                item_id,
            } => format!("collections/{}/items/{}", collection_id, item_id),
            Self::Search(_) => "search".to_string(),
            Self::SearchGet(_) => "search".to_string(),
        }
    }

    /// URL query parameters as `(key, value)` pairs.
    pub fn query_params(&self) -> Vec<(String, String)> {
        match self {
            Self::GetCollections { limit } => {
                let mut p = Vec::new();
                if let Some(l) = limit {
                    p.push(("limit".to_string(), l.to_string()));
                }
                p
            }
            Self::GetItems {
                limit,
                offset,
                bbox,
                datetime,
                fields,
                ..
            } => {
                let mut p = Vec::new();
                if let Some(l) = limit {
                    p.push(("limit".to_string(), l.to_string()));
                }
                if let Some(o) = offset {
                    p.push(("offset".to_string(), o.to_string()));
                }
                if let Some(b) = bbox {
                    p.push((
                        "bbox".to_string(),
                        format!("{},{},{},{}", b[0], b[1], b[2], b[3]),
                    ));
                }
                if let Some(dt) = datetime {
                    p.push(("datetime".to_string(), dt.clone()));
                }
                if let Some(flds) = fields {
                    if !flds.is_empty() {
                        p.push(("fields".to_string(), flds.join(",")));
                    }
                }
                p
            }
            Self::SearchGet(req) => req.to_query_params(),
            _ => Vec::new(),
        }
    }

    /// JSON body for `POST` requests, or `None` for `GET` requests.
    pub fn body_json(&self) -> Option<String> {
        match self {
            Self::Search(req) => serde_json::to_string(req).ok(),
            _ => None,
        }
    }

    /// Build the full URL by appending the path and query string to `base_url`.
    ///
    /// A trailing slash on `base_url` is normalised away before appending.
    pub fn to_url(&self, base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        let path = self.path();
        let params = self.query_params();

        let full = if path.is_empty() {
            base.to_string()
        } else {
            format!("{}/{}", base, path)
        };

        if params.is_empty() {
            return full;
        }

        let qs: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        format!("{}?{}", full, qs)
    }
}

// ---------------------------------------------------------------------------
// Minimal percent-encoding (only critical characters)
// ---------------------------------------------------------------------------

/// Percent-encode a query-parameter key or value (minimal, RFC 3986 subset).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            // Unreserved characters — pass through
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            // Safe in query values
            '+' | ',' | ':' | '/' | '@' | '!' | '$' | '\'' | '(' | ')' | '*' | ';' => out.push(ch),
            // Encode space and everything else
            ' ' => out.push_str("%20"),
            c => {
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                for byte in encoded.bytes() {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_request_builder() {
        let req = SearchRequest::new()
            .with_bbox([-10.0, -10.0, 10.0, 10.0])
            .with_collections(vec!["col-a".to_string()])
            .with_limit(25);
        assert_eq!(req.bbox, Some([-10.0, -10.0, 10.0, 10.0]));
        assert_eq!(req.collections, Some(vec!["col-a".to_string()]));
        assert_eq!(req.limit, Some(25));
    }

    #[test]
    fn test_search_request_json_roundtrip() {
        let req = SearchRequest::new()
            .with_bbox([-5.0, -5.0, 5.0, 5.0])
            .with_datetime("2023-01-01T00:00:00Z")
            .with_limit(10);
        let json = req.to_json().expect("serialize");
        let back = SearchRequest::from_json(&json).expect("deserialize");
        assert_eq!(req, back);
    }

    #[test]
    fn test_get_landing_method_and_path() {
        let r = StacApiRequest::GetLanding;
        assert_eq!(r.method(), "GET");
        assert_eq!(r.path(), "");
    }

    #[test]
    fn test_get_conformance_path() {
        let r = StacApiRequest::GetConformance;
        assert_eq!(r.path(), "conformance");
    }

    #[test]
    fn test_search_method_is_post() {
        let r = StacApiRequest::Search(SearchRequest::new());
        assert_eq!(r.method(), "POST");
    }

    #[test]
    fn test_search_body_json_is_some() {
        let r = StacApiRequest::Search(SearchRequest::new().with_limit(5));
        assert!(r.body_json().is_some());
    }

    #[test]
    fn test_get_items_query_params_bbox() {
        let r = StacApiRequest::GetItems {
            collection_id: "test".to_string(),
            limit: None,
            offset: None,
            bbox: Some([-10.0, -20.0, 10.0, 20.0]),
            datetime: None,
            fields: None,
        };
        let params = r.query_params();
        let bbox_param = params.iter().find(|(k, _)| k == "bbox");
        assert!(bbox_param.is_some());
        assert!(bbox_param.expect("bbox param").1.contains("-10"));
    }

    #[test]
    fn test_to_url_landing() {
        let r = StacApiRequest::GetLanding;
        assert_eq!(
            r.to_url("https://example.com/stac"),
            "https://example.com/stac"
        );
    }

    #[test]
    fn test_to_url_collections() {
        let r = StacApiRequest::GetCollections { limit: Some(5) };
        let url = r.to_url("https://example.com");
        assert!(url.starts_with("https://example.com/collections"));
        assert!(url.contains("limit=5"));
    }

    #[test]
    fn test_sort_field_asc_desc() {
        let asc = SortField::asc("datetime");
        assert_eq!(asc.direction, SortDirection::Ascending);
        let desc = SortField::desc("created");
        assert_eq!(desc.direction, SortDirection::Descending);
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
