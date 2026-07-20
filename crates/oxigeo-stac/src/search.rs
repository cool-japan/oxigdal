//! STAC API search client.
//!
//! This module provides an async HTTP client for searching STAC APIs.

use crate::{
    error::{Result, StacError},
    item::Item,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "async")]
use reqwest::Client as HttpClient;

/// Result of a successful STAC Transaction API HTTP operation.
///
/// Returned by `StacClient::create_item`, `StacClient::update_item`,
/// `StacClient::upsert_item`, and `StacClient::delete_item` (all
/// feature-gated behind `reqwest` + `async`).
#[cfg(feature = "async")]
#[derive(Debug, Clone)]
pub struct HttpTransactionResult {
    /// HTTP status code returned by the server.
    pub status: u16,
    /// Value of the `Location` header, if present (typically the created item URL).
    pub location: Option<String>,
}

/// STAC API client for searching catalogs.
#[cfg(feature = "async")]
#[derive(Debug, Clone)]
pub struct StacClient {
    /// Base URL of the STAC API.
    base_url: String,
    /// HTTP client.
    client: HttpClient,
    /// Cached conformance classes from the server's landing page.
    ///
    /// `None` means `with_conformance()` has not yet been called.
    /// `Some(set)` contains the classes declared by the server.
    conformance_classes:
        std::sync::Arc<std::sync::Mutex<Option<std::collections::HashSet<String>>>>,
}

#[cfg(feature = "async")]
impl StacClient {
    /// Creates a new STAC API client.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL of the STAC API
    ///
    /// # Returns
    ///
    /// A new StacClient instance
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();

        // Validate URL
        url::Url::parse(&base_url)?;

        let client = HttpClient::builder()
            .user_agent("oxigeo-stac/0.2.0")
            .build()
            .map_err(|e| StacError::Http(e.to_string()))?;

        Ok(Self {
            base_url,
            client,
            conformance_classes: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }

    /// Creates a new search query builder.
    ///
    /// # Returns
    ///
    /// A new SearchBuilder instance
    pub fn search(&self) -> SearchBuilder {
        SearchBuilder::new(self.clone())
    }

    /// Executes a search request.
    ///
    /// # Arguments
    ///
    /// * `params` - Search parameters
    ///
    /// # Returns
    ///
    /// Search results
    #[cfg(feature = "async")]
    pub async fn execute_search(&self, params: &SearchParams) -> Result<SearchResults> {
        let url = format!("{}/search", self.base_url);

        let response = self.client.post(&url).json(params).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(StacError::ApiResponse(format!(
                "HTTP {} - {}",
                status, body
            )));
        }

        let results: SearchResults = response.json().await?;
        Ok(results)
    }

    /// Gets an item by ID.
    ///
    /// # Arguments
    ///
    /// * `collection_id` - Collection ID
    /// * `item_id` - Item ID
    ///
    /// # Returns
    ///
    /// The requested item
    #[cfg(feature = "async")]
    pub async fn get_item(&self, collection_id: &str, item_id: &str) -> Result<Item> {
        let url = format!(
            "{}/collections/{}/items/{}",
            self.base_url, collection_id, item_id
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(StacError::ApiResponse(format!(
                "HTTP {} - Item not found",
                status
            )));
        }

        let item: Item = response.json().await?;
        Ok(item)
    }

    // ── W4: HTTP-backed Transaction Extension ─────────────────────────────────

    /// Create a STAC item in a collection (POST).
    ///
    /// # Errors
    ///
    /// Returns error on HTTP failure, serialization error, or network error.
    #[cfg(feature = "async")]
    pub async fn create_item(
        &self,
        collection_id: &str,
        item: &serde_json::Value,
    ) -> Result<HttpTransactionResult> {
        let url = format!("{}/collections/{}/items", self.base_url, collection_id);
        let response = self
            .client
            .post(&url)
            .json(item)
            .send()
            .await
            .map_err(StacError::from)?;

        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        match status {
            200 | 201 => Ok(HttpTransactionResult { status, location }),
            404 => Err(StacError::NotFound(format!(
                "Collection {collection_id} not found"
            ))),
            409 => Err(StacError::AlreadyExists(format!(
                "Item already exists in {collection_id}"
            ))),
            _ => {
                let body = response.text().await.unwrap_or_default();
                let snippet = body.chars().take(200).collect::<String>();
                Err(StacError::ApiResponse(format!("{status} {snippet}")))
            }
        }
    }

    /// Update a STAC item in a collection (PUT).
    ///
    /// # Errors
    ///
    /// Returns error on HTTP failure or network error.
    #[cfg(feature = "async")]
    pub async fn update_item(
        &self,
        collection_id: &str,
        item_id: &str,
        item: &serde_json::Value,
    ) -> Result<HttpTransactionResult> {
        let url = format!(
            "{}/collections/{}/items/{}",
            self.base_url, collection_id, item_id
        );
        let response = self
            .client
            .put(&url)
            .json(item)
            .send()
            .await
            .map_err(StacError::from)?;

        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        match status {
            200 | 204 => Ok(HttpTransactionResult { status, location }),
            404 => Err(StacError::NotFound(format!(
                "Item {item_id} not found in {collection_id}"
            ))),
            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(StacError::ApiResponse(format!(
                    "{status} {}",
                    body.chars().take(200).collect::<String>()
                )))
            }
        }
    }

    /// Create or update a STAC item (upsert).
    ///
    /// Tries create first; if server returns 409 Conflict, falls back to update.
    ///
    /// # Errors
    ///
    /// Returns error on network failure, or when the item JSON has no `"id"` field.
    #[cfg(feature = "async")]
    pub async fn upsert_item(
        &self,
        collection_id: &str,
        item: &serde_json::Value,
    ) -> Result<HttpTransactionResult> {
        match self.create_item(collection_id, item).await {
            Ok(r) => Ok(r),
            Err(StacError::AlreadyExists(_)) => {
                let item_id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| StacError::MissingField("id".to_string()))?;
                self.update_item(collection_id, item_id, item).await
            }
            Err(e) => Err(e),
        }
    }

    /// Delete a STAC item from a collection (DELETE).
    ///
    /// # Errors
    ///
    /// Returns error on HTTP failure or network error.
    #[cfg(feature = "async")]
    pub async fn delete_item(
        &self,
        collection_id: &str,
        item_id: &str,
    ) -> Result<HttpTransactionResult> {
        let url = format!(
            "{}/collections/{}/items/{}",
            self.base_url, collection_id, item_id
        );
        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(StacError::from)?;

        let status = response.status().as_u16();
        match status {
            200 | 204 => Ok(HttpTransactionResult {
                status,
                location: None,
            }),
            404 => Err(StacError::NotFound(format!(
                "Item {item_id} not found in {collection_id}"
            ))),
            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(StacError::ApiResponse(format!(
                    "{status} {}",
                    body.chars().take(200).collect::<String>()
                )))
            }
        }
    }

    // ── W5: Conformance-class auto-detection ──────────────────────────────────

    /// Fetch the server's landing page and cache its `conformsTo` classes.
    ///
    /// Tolerates fetch failures — if the landing page is not available, the
    /// conformance cache remains empty and [`supports()`] returns `false` for
    /// all URIs.
    ///
    /// # Errors
    ///
    /// Returns an error only on serialization issues (malformed JSON); never
    /// on HTTP 404 or network timeout.
    ///
    /// [`supports()`]: StacClient::supports
    #[cfg(feature = "async")]
    pub async fn with_conformance(self) -> Result<Self> {
        let url = format!("{}/", self.base_url.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Err(_) => {}
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(body) = response.text().await {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                            let classes: std::collections::HashSet<String> = json
                                .get("conformsTo")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();
                            if let Ok(mut guard) = self.conformance_classes.lock() {
                                *guard = Some(classes);
                            }
                        }
                    }
                }
            }
        }
        Ok(self)
    }

    /// Check whether the server declared a specific conformance class URI.
    ///
    /// Returns `false` if `with_conformance()` (feature-gated behind
    /// `reqwest` + `async`) has not been called or if the server did not
    /// declare this class.
    pub fn supports(&self, class_uri: &str) -> bool {
        self.conformance_classes
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|set| set.contains(class_uri)))
            .unwrap_or(false)
    }
}

/// Builder for STAC search queries.
#[cfg(feature = "async")]
#[derive(Debug, Clone)]
pub struct SearchBuilder {
    #[allow(dead_code)]
    client: StacClient,
    params: SearchParams,
}

#[cfg(feature = "async")]
impl SearchBuilder {
    /// Creates a new search builder.
    ///
    /// # Arguments
    ///
    /// * `client` - STAC client
    ///
    /// # Returns
    ///
    /// A new SearchBuilder instance
    pub fn new(client: StacClient) -> Self {
        Self {
            client,
            params: SearchParams::default(),
        }
    }

    /// Sets the collections to search.
    ///
    /// # Arguments
    ///
    /// * `collections` - Vector of collection IDs
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn collections(mut self, collections: Vec<impl Into<String>>) -> Self {
        self.params.collections = Some(collections.into_iter().map(|c| c.into()).collect());
        self
    }

    /// Sets the bounding box to search within.
    ///
    /// # Arguments
    ///
    /// * `bbox` - Bounding box [west, south, east, north]
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn bbox(mut self, bbox: [f64; 4]) -> Self {
        self.params.bbox = Some(bbox.to_vec());
        self
    }

    /// Sets the datetime filter.
    ///
    /// # Arguments
    ///
    /// * `datetime` - Datetime string (RFC 3339 or interval)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn datetime(mut self, datetime: impl Into<String>) -> Self {
        self.params.datetime = Some(datetime.into());
        self
    }

    /// Sets the datetime range filter.
    ///
    /// # Arguments
    ///
    /// * `start` - Start datetime
    /// * `end` - End datetime
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn datetime_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        let datetime_str = format!("{}/{}", start.to_rfc3339(), end.to_rfc3339());
        self.params.datetime = Some(datetime_str);
        self
    }

    /// Sets the maximum number of results.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of results
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn limit(mut self, limit: u32) -> Self {
        self.params.limit = Some(limit);
        self
    }

    /// Adds a query filter.
    ///
    /// # Arguments
    ///
    /// * `key` - Property key
    /// * `value` - Filter value
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn query(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        match &mut self.params.query {
            Some(query) => {
                query.insert(key.into(), value);
            }
            None => {
                let mut query = HashMap::new();
                query.insert(key.into(), value);
                self.params.query = Some(query);
            }
        }
        self
    }

    /// Sets a CQL2 filter.
    ///
    /// # Arguments
    ///
    /// * `filter` - CQL2 filter expression
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn filter(mut self, filter: serde_json::Value) -> Self {
        self.params.filter = Some(filter);
        self.params.filter_lang = Some("cql2-json".to_string());
        self
    }

    /// Sets fields to include in the response.
    ///
    /// # Arguments
    ///
    /// * `fields` - Field names to include
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn fields(mut self, fields: Vec<impl Into<String>>) -> Self {
        self.params.fields = Some(fields.into_iter().map(|f| f.into()).collect());
        self
    }

    /// Adds a sort specification.
    ///
    /// # Arguments
    ///
    /// * `field` - Field to sort by
    /// * `direction` - Sort direction
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn sort_by(mut self, field: impl Into<String>, direction: SortDirection) -> Self {
        let sort = SortBy {
            field: field.into(),
            direction,
        };

        match &mut self.params.sortby {
            Some(sortby) => sortby.push(sort),
            None => self.params.sortby = Some(vec![sort]),
        }
        self
    }

    /// Executes the search.
    ///
    /// # Returns
    ///
    /// Search results
    #[cfg(feature = "async")]
    pub async fn execute(self) -> Result<SearchResults> {
        self.client.execute_search(&self.params).await
    }

    /// Creates a paginator for iterating through results.
    ///
    /// # Returns
    ///
    /// A paginator for the search
    #[cfg(feature = "async")]
    pub fn paginate(self) -> crate::pagination::Paginator {
        crate::pagination::Paginator::new(self.client, self.params)
    }
}

/// STAC search parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchParams {
    /// Collections to search in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<String>>,

    /// Bounding box [west, south, east, north].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<f64>>,

    /// Datetime string (RFC 3339 or interval).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datetime: Option<String>,

    /// Maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Query filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<HashMap<String, serde_json::Value>>,

    /// CQL2 filter (Common Query Language 2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,

    /// Filter language (e.g., "cql2-json", "cql2-text").
    #[serde(rename = "filter-lang", skip_serializing_if = "Option::is_none")]
    pub filter_lang: Option<String>,

    /// Page token for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,

    /// Fields to include in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,

    /// Sortby specifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sortby: Option<Vec<SortBy>>,
}

/// Sort specification for search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortBy {
    /// Field to sort by.
    pub field: String,

    /// Sort direction.
    pub direction: SortDirection,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// STAC search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Type must be "FeatureCollection".
    #[serde(rename = "type")]
    pub type_: String,

    /// Features (STAC Items) in the results.
    pub features: Vec<Item>,

    /// Links to related resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<crate::item::Link>>,

    /// Number of items returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_returned: Option<u32>,

    /// Number of items matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_matched: Option<u32>,

    /// Context information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<SearchContext>,
}

/// Context information for search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContext {
    /// Number of items returned.
    pub returned: u32,

    /// Limit specified in the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Number of items matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<u32>,
}

impl SearchResults {
    /// Gets the next page link if available.
    ///
    /// # Returns
    ///
    /// The next page link if it exists
    pub fn get_next_link(&self) -> Option<&crate::item::Link> {
        self.links
            .as_ref()
            .and_then(|links| links.iter().find(|link| link.rel == "next"))
    }

    /// Checks if there are more results available.
    ///
    /// # Returns
    ///
    /// `true` if there are more results
    pub fn has_more(&self) -> bool {
        self.get_next_link().is_some()
    }

    /// Validates the search results.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        if self.type_ != "FeatureCollection" {
            return Err(StacError::InvalidType {
                expected: "FeatureCollection".to_string(),
                found: self.type_.clone(),
            });
        }

        // Validate all items
        for (i, item) in self.features.iter().enumerate() {
            item.validate().map_err(|e| StacError::InvalidFieldValue {
                field: format!("features[{}]", i),
                reason: e.to_string(),
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[cfg(feature = "async")]
mod tests {
    use super::*;

    #[test]
    fn test_stac_client_new() {
        let client = StacClient::new("https://earth-search.aws.element84.com/v1");
        assert!(client.is_ok());

        let invalid = StacClient::new("not-a-url");
        assert!(invalid.is_err());
    }

    #[test]
    fn test_search_builder() {
        let client = StacClient::new("https://earth-search.aws.element84.com/v1")
            .expect("Failed to create client");
        let builder = client
            .search()
            .collections(vec!["sentinel-2-l2a"])
            .bbox([-122.5, 37.5, -122.0, 38.0])
            .limit(10);

        assert_eq!(
            builder.params.collections,
            Some(vec!["sentinel-2-l2a".to_string()])
        );
        assert_eq!(builder.params.bbox, Some(vec![-122.5, 37.5, -122.0, 38.0]));
        assert_eq!(builder.params.limit, Some(10));
    }

    #[test]
    fn test_search_params_serialization() {
        let params = SearchParams {
            collections: Some(vec!["test".to_string()]),
            bbox: Some(vec![-180.0, -90.0, 180.0, 90.0]),
            datetime: Some("2023-01-01/2023-12-31".to_string()),
            limit: Some(100),
            query: None,
            filter: None,
            filter_lang: None,
            page_token: None,
            fields: None,
            sortby: None,
        };

        let json = serde_json::to_string(&params);
        assert!(json.is_ok());
    }
}
