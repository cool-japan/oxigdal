//! OGC API – Features resource types used by STAC API.
//!
//! These lightweight types model the landing page, collections list, and
//! single-collection response that a STAC API exposes in addition to the
//! Item Search endpoint.

use serde::{Deserialize, Serialize};

use super::search::Link;

/// OGC API / STAC API landing page response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LandingPage {
    /// Service title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Service description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// STAC API version.
    #[serde(rename = "stac_version")]
    pub stac_version: String,

    /// Conformance URIs (mirrors `/conformance`).
    #[serde(rename = "conformsTo", default, skip_serializing_if = "Vec::is_empty")]
    pub conforms_to: Vec<String>,

    /// Navigation / capability links.
    pub links: Vec<Link>,
}

impl LandingPage {
    /// Creates a minimal landing page.
    pub fn new(stac_version: impl Into<String>, links: Vec<Link>) -> Self {
        Self {
            title: None,
            description: None,
            stac_version: stac_version.into(),
            conforms_to: Vec::new(),
            links,
        }
    }

    /// Adds a title to the landing page.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a description to the landing page.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Adds conformance URIs.
    pub fn with_conforms_to(mut self, uris: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.conforms_to = uris.into_iter().map(Into::into).collect();
        self
    }
}

/// A list of collection summaries as returned by `GET /collections`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionsList {
    /// Collection summaries.
    pub collections: Vec<CollectionSummary>,
    /// Navigation links.
    pub links: Vec<Link>,
}

/// Summary information for a STAC Collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionSummary {
    /// Unique collection identifier.
    pub id: String,
    /// Human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Brief description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// STAC version.
    #[serde(rename = "stac_version")]
    pub stac_version: String,
    /// Navigation links (e.g., `"self"`, `"items"`).
    pub links: Vec<Link>,
    /// Spatial extent bounding box(es) `[[west, south, east, north]]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<serde_json::Value>,
}

impl CollectionSummary {
    /// Creates a minimal collection summary.
    pub fn new(id: impl Into<String>, stac_version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: None,
            description: None,
            stac_version: stac_version.into(),
            links: Vec::new(),
            extent: None,
        }
    }

    /// Adds a title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Adds a navigation link.
    pub fn add_link(mut self, link: Link) -> Self {
        self.links.push(link);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_landing_page_new() {
        let page = LandingPage::new("1.0.0", vec![]).with_title("My STAC API");
        assert_eq!(page.title, Some("My STAC API".to_string()));
        assert_eq!(page.stac_version, "1.0.0");
    }

    #[test]
    fn test_landing_page_roundtrip() {
        let page = LandingPage::new("1.0.0", vec![Link::new("self", "https://example.com")])
            .with_title("Test API")
            .with_description("A test STAC API");
        let json = serde_json::to_string(&page).expect("serialize");
        let back: LandingPage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(page, back);
    }

    #[test]
    fn test_collection_summary() {
        let cs = CollectionSummary::new("sentinel-2-l2a", "1.0.0")
            .with_title("Sentinel-2 L2A")
            .add_link(Link::new(
                "items",
                "https://example.com/collections/sentinel-2-l2a/items",
            ));
        assert_eq!(cs.id, "sentinel-2-l2a");
        assert_eq!(cs.links.len(), 1);
    }

    #[test]
    fn test_collections_list_roundtrip() {
        let list = CollectionsList {
            collections: vec![CollectionSummary::new("my-col", "1.0.0")],
            links: vec![Link::new("self", "https://example.com/collections")],
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: CollectionsList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(list, back);
    }
}
