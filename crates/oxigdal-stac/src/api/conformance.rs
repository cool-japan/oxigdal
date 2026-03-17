//! STAC API conformance class definitions and validation.
//!
//! <https://github.com/radiantearth/stac-api-spec>

use serde::{Deserialize, Serialize};

/// Well-known STAC API v1.0 conformance class URIs.
pub mod uris {
    /// STAC API – Core.
    pub const CORE: &str = "https://api.stacspec.org/v1.0.0/core";
    /// STAC API – Browseable.
    pub const BROWSEABLE: &str = "https://api.stacspec.org/v1.0.0/browseable";
    /// STAC API – Item Search.
    pub const ITEM_SEARCH: &str = "https://api.stacspec.org/v1.0.0/item-search";
    /// STAC API – Item Search – Filter.
    pub const ITEM_SEARCH_FILTER: &str = "https://api.stacspec.org/v1.0.0/item-search#filter";
    /// STAC API – Item Search – Sort.
    pub const ITEM_SEARCH_SORT: &str = "https://api.stacspec.org/v1.0.0/item-search#sort";
    /// STAC API – Item Search – Fields.
    pub const ITEM_SEARCH_FIELDS: &str = "https://api.stacspec.org/v1.0.0/item-search#fields";
    /// OGC API – Features – Core.
    pub const OGCAPI_FEATURES: &str = "http://www.opengis.net/spec/ogcapi-features-1/1.0/conf/core";
    /// STAC API – Transaction Extension.
    pub const TRANSACTION: &str =
        "https://api.stacspec.org/v1.0.0/ogcapi-features/extensions/transaction";
    /// STAC API – Children.
    pub const CHILDREN: &str = "https://api.stacspec.org/v1.0.0/children";
}

/// Conformance declaration response (`GET /conformance`).
///
/// Lists the conformance classes that a server implements, enabling clients
/// to discover which features and operations are available.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConformanceDeclaration {
    /// List of conformance class URIs that the server conforms to.
    #[serde(rename = "conformsTo")]
    pub conforms_to: Vec<String>,
}

impl ConformanceDeclaration {
    /// Creates a conformance declaration from an iterable of URI strings.
    pub fn new(classes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            conforms_to: classes.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns `true` if the declaration includes the given conformance class URI.
    pub fn supports(&self, class: &str) -> bool {
        self.conforms_to.iter().any(|c| c == class)
    }

    /// Builds the standard STAC API v1.0 conformance declaration.
    ///
    /// Includes: Core, Browseable, Item Search (+ filter, sort, fields),
    /// OGC API Features, and Children.
    pub fn standard() -> Self {
        Self::new([
            uris::CORE,
            uris::BROWSEABLE,
            uris::ITEM_SEARCH,
            uris::ITEM_SEARCH_FILTER,
            uris::ITEM_SEARCH_SORT,
            uris::ITEM_SEARCH_FIELDS,
            uris::OGCAPI_FEATURES,
            uris::CHILDREN,
        ])
    }

    /// Adds the Transaction extension conformance class to this declaration.
    pub fn with_transaction(mut self) -> Self {
        self.conforms_to.push(uris::TRANSACTION.to_string());
        self
    }

    /// Adds an arbitrary conformance class URI to this declaration.
    pub fn with_class(mut self, uri: impl Into<String>) -> Self {
        self.conforms_to.push(uri.into());
        self
    }

    /// Returns the number of conformance classes declared.
    pub fn len(&self) -> usize {
        self.conforms_to.len()
    }

    /// Returns `true` if the declaration contains no conformance classes.
    pub fn is_empty(&self) -> bool {
        self.conforms_to.is_empty()
    }
}

impl Default for ConformanceDeclaration {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_includes_core() {
        let decl = ConformanceDeclaration::standard();
        assert!(decl.supports(uris::CORE));
    }

    #[test]
    fn test_standard_includes_item_search() {
        let decl = ConformanceDeclaration::standard();
        assert!(decl.supports(uris::ITEM_SEARCH));
    }

    #[test]
    fn test_standard_includes_ogc() {
        let decl = ConformanceDeclaration::standard();
        assert!(decl.supports(uris::OGCAPI_FEATURES));
    }

    #[test]
    fn test_supports_false_for_unknown() {
        let decl = ConformanceDeclaration::standard();
        assert!(!decl.supports("https://example.com/unknown"));
    }

    #[test]
    fn test_with_transaction() {
        let decl = ConformanceDeclaration::standard().with_transaction();
        assert!(decl.supports(uris::TRANSACTION));
    }

    #[test]
    fn test_custom_class() {
        let decl = ConformanceDeclaration::new(["https://my.server/custom"]);
        assert!(decl.supports("https://my.server/custom"));
    }

    #[test]
    fn test_json_roundtrip() {
        let decl = ConformanceDeclaration::standard();
        let json = serde_json::to_string(&decl).expect("serialize");
        let back: ConformanceDeclaration = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decl, back);
    }

    #[test]
    fn test_standard_count() {
        let decl = ConformanceDeclaration::standard();
        // Core, Browseable, Item Search, Filter, Sort, Fields, OGC Features, Children = 8
        assert_eq!(decl.len(), 8);
    }
}
