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

#[cfg(feature = "reqwest")]
use reqwest::Client as HttpClient;

/// STAC API client for searching catalogs.
#[cfg(feature = "reqwest")]
#[derive(Debug, Clone)]
pub struct StacClient {
    /// Base URL of the STAC API.
    #[allow(dead_code)]
    base_url: String,
    /// HTTP client.
    #[allow(dead_code)]
    client: HttpClient,
}

#[cfg(feature = "reqwest")]
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
            .user_agent("oxigdal-stac/0.1.0")
            .build()
            .map_err(|e| StacError::Http(e.to_string()))?;

        Ok(Self { base_url, client })
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
}

/// Builder for STAC search queries.
#[cfg(feature = "reqwest")]
#[derive(Debug, Clone)]
pub struct SearchBuilder {
    #[allow(dead_code)]
    client: StacClient,
    params: SearchParams,
}

#[cfg(feature = "reqwest")]
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
    #[cfg(feature = "reqwest")]
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
#[cfg(feature = "reqwest")]
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
