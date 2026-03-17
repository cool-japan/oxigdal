//! OGC Features API server request handler.

use chrono::Utc;

use super::error::FeaturesError;
use super::query::QueryParams;
use super::types::{
    Collection, Collections, ConformanceClasses, Feature, FeatureCollection, LandingPage, Link,
};

/// Maximum features per request the server will honour
pub const MAX_LIMIT: u32 = 10_000;

/// Stateless request handler for OGC API - Features endpoints
pub struct FeaturesServer {
    /// Service title
    pub title: String,
    /// Service description
    pub description: String,
    /// Base URL (e.g. `"https://example.com/ogcapi"`)
    pub base_url: String,
    /// Registered collections
    pub collections: Vec<Collection>,
}

impl FeaturesServer {
    /// Create a new server with the given title and base URL.
    pub fn new(title: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: String::new(),
            base_url: base_url.into(),
            collections: vec![],
        }
    }

    /// Register a collection.
    pub fn add_collection(&mut self, collection: Collection) {
        self.collections.push(collection);
    }

    /// Build the landing page response.
    pub fn landing_page(&self) -> LandingPage {
        let base = &self.base_url;
        LandingPage {
            title: self.title.clone(),
            description: if self.description.is_empty() {
                None
            } else {
                Some(self.description.clone())
            },
            links: vec![
                Link::new(base.clone(), "self")
                    .with_type("application/json")
                    .with_title("This document"),
                Link::new(format!("{base}/api"), "service-desc")
                    .with_type("application/vnd.oai.openapi+json;version=3.0")
                    .with_title("The API definition"),
                Link::new(format!("{base}/conformance"), "conformance")
                    .with_type("application/json")
                    .with_title("Conformance classes"),
                Link::new(format!("{base}/collections"), "data")
                    .with_type("application/json")
                    .with_title("Access the data"),
            ],
        }
    }

    /// Build the conformance declaration response (Part 1 + Part 2).
    pub fn conformance(&self) -> ConformanceClasses {
        ConformanceClasses::with_crs()
    }

    /// Build the collections listing response.
    pub fn list_collections(&self) -> Collections {
        let base = &self.base_url;
        Collections {
            links: vec![
                Link::new(format!("{base}/collections"), "self")
                    .with_type("application/json")
                    .with_title("Collections"),
            ],
            collections: self.collections.clone(),
        }
    }

    /// Look up a collection by id.
    pub fn get_collection(&self, id: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    /// Build a paginated `FeatureCollection` response.
    ///
    /// The caller supplies the full `features` vector (already filtered but not
    /// yet paginated) together with `total_matched` (which may differ when
    /// server-side filtering is applied outside this function).
    ///
    /// This function:
    /// 1. Validates the limit against `MAX_LIMIT`.
    /// 2. Returns `FeaturesError::CollectionNotFound` if the collection is
    ///    not registered.
    /// 3. Applies offset / limit slicing.
    /// 4. Attaches `numberMatched`, `numberReturned`, and `timeStamp`.
    /// 5. Attaches `next` and `prev` pagination links.
    pub fn build_items_response(
        &self,
        collection_id: &str,
        features: Vec<Feature>,
        params: &QueryParams,
        total_matched: Option<u64>,
    ) -> Result<FeatureCollection, FeaturesError> {
        // Validate collection exists
        if self.get_collection(collection_id).is_none() {
            return Err(FeaturesError::CollectionNotFound(collection_id.to_string()));
        }

        let limit = params.effective_limit();
        if limit > MAX_LIMIT {
            return Err(FeaturesError::LimitExceeded {
                requested: limit,
                max: MAX_LIMIT,
            });
        }

        let offset = params.effective_offset() as usize;
        let limit_usize = limit as usize;
        let total = features.len();

        // Apply pagination slice
        let page: Vec<Feature> = features
            .into_iter()
            .skip(offset)
            .take(limit_usize)
            .collect();

        let number_returned = page.len() as u64;
        let number_matched = total_matched.unwrap_or(total as u64);

        // Build pagination links
        let base = &self.base_url;
        let items_base = format!("{base}/collections/{collection_id}/items");
        let mut links: Vec<Link> = vec![
            Link::new(
                format!("{items_base}?limit={limit}&offset={offset}"),
                "self",
            )
            .with_type("application/geo+json")
            .with_title("This page"),
        ];

        // next link
        let next_offset = offset + limit_usize;
        if next_offset < total {
            links.push(
                Link::new(
                    format!("{items_base}?limit={limit}&offset={next_offset}"),
                    "next",
                )
                .with_type("application/geo+json")
                .with_title("Next page"),
            );
        }

        // prev link
        if offset > 0 {
            let prev_offset = offset.saturating_sub(limit_usize);
            links.push(
                Link::new(
                    format!("{items_base}?limit={limit}&offset={prev_offset}"),
                    "prev",
                )
                .with_type("application/geo+json")
                .with_title("Previous page"),
            );
        }

        Ok(FeatureCollection {
            type_: "FeatureCollection".to_string(),
            features: page,
            links: Some(links),
            time_stamp: Some(Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
            number_matched: Some(number_matched),
            number_returned: Some(number_returned),
        })
    }
}
