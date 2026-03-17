//! Pagination strategies and iterators for STAC API item search.
//!
//! This module is transport-agnostic: it provides the *logic* of pagination
//! (which request to issue next, how to apply offsets / tokens) without
//! performing any I/O itself.  The caller supplies a closure that executes
//! the actual HTTP fetch and returns a parsed [`ItemCollection`].

use super::{ClientError, ItemCollection, SearchRequest};

// ---------------------------------------------------------------------------
// PaginationStrategy
// ---------------------------------------------------------------------------

/// How to paginate through STAC API results.
///
/// Three strategies are supported:
///
/// 1. **Offset-based** — use `offset` / `limit` query parameters.
/// 2. **Token-based** — use an opaque cursor token returned by each page.
/// 3. **Link-based** — follow the `"next"` link returned in each response.
#[derive(Debug, Clone, PartialEq)]
pub enum PaginationStrategy {
    /// Offset / limit pagination (`?offset=N&limit=M`).
    OffsetBased {
        /// Current offset (0-based).
        offset: u32,
        /// Page size.
        limit: u32,
    },
    /// Token / cursor-based pagination.
    TokenBased {
        /// Opaque token from the previous response (`None` for the first page).
        token: Option<String>,
        /// Page size.
        limit: u32,
    },
    /// Follow the `"next"` link returned in each [`ItemCollection`] response.
    LinkBased,
}

impl PaginationStrategy {
    /// Build an offset-based strategy starting at page 1 (offset 0).
    pub fn first_page(limit: u32) -> Self {
        Self::OffsetBased { offset: 0, limit }
    }

    /// Build a token-based strategy for the first page (no prior token).
    pub fn first_token_page(limit: u32) -> Self {
        Self::TokenBased { token: None, limit }
    }

    /// Derive the strategy for the *next* page from the current page's
    /// response.  Returns `None` when the response indicates there are no
    /// more pages.
    pub fn next_page(&self, response: &ItemCollection) -> Option<Self> {
        match self {
            Self::OffsetBased { offset, limit } => {
                // If the server returned fewer items than requested, we have
                // reached the last page.
                let returned = response.features.len() as u32;
                if returned < *limit || response.features.is_empty() {
                    return None;
                }
                Some(Self::OffsetBased {
                    offset: offset + limit,
                    limit: *limit,
                })
            }
            Self::TokenBased { limit, .. } => {
                // A "next" token in the response drives the next page.
                let next_token = response.next_page_token()?;
                Some(Self::TokenBased {
                    token: Some(next_token),
                    limit: *limit,
                })
            }
            Self::LinkBased => {
                // A "next" link in the response indicates another page exists.
                // The caller is responsible for extracting the URL from the
                // link; we just signal that there is a next page.
                if response.next_link().is_some() {
                    Some(Self::LinkBased)
                } else {
                    None
                }
            }
        }
    }

    /// Mutate a [`SearchRequest`] to apply this strategy's pagination
    /// parameters (sets `limit`, `offset`, and/or `token`).
    pub fn apply_to_request(&self, request: &mut SearchRequest) {
        match self {
            Self::OffsetBased { offset, limit } => {
                request.limit = Some(*limit);
                // Use `page` field to approximate offset-based semantics.
                // Some APIs use `page` (1-indexed), others use `offset`.
                // We store the raw offset in `page` here; the `StacApiClient`
                // translates appropriately via query params.
                if *offset > 0 {
                    let limit_val = if *limit == 0 { 1 } else { *limit };
                    request.page = Some(offset / limit_val + 1);
                } else {
                    request.page = None;
                }
            }
            Self::TokenBased { token, limit } => {
                request.limit = Some(*limit);
                request.token = token.clone();
            }
            Self::LinkBased => {
                // No mutation needed; caller extracts the next URL from the link.
            }
        }
    }

    /// Returns the current offset for offset-based strategies, or 0 otherwise.
    pub fn current_offset(&self) -> u32 {
        match self {
            Self::OffsetBased { offset, .. } => *offset,
            _ => 0,
        }
    }

    /// Returns the limit for strategies that carry one, or `None` for
    /// [`PaginationStrategy::LinkBased`].
    pub fn limit(&self) -> Option<u32> {
        match self {
            Self::OffsetBased { limit, .. } | Self::TokenBased { limit, .. } => Some(*limit),
            Self::LinkBased => None,
        }
    }
}

// ---------------------------------------------------------------------------
// PageIterator
// ---------------------------------------------------------------------------

/// A lazy iterator over STAC API result pages.
///
/// The caller supplies a `fetch` closure that takes a [`SearchRequest`] and
/// returns a `Result<ItemCollection, ClientError>`.  The iterator applies
/// the chosen [`PaginationStrategy`] automatically, stopping when there are
/// no more pages, the response is empty, or `max_pages` is reached.
///
/// # Example
///
/// ```rust
/// use oxigdal_stac::client::{
///     PageIterator, PaginationStrategy, SearchRequest, ItemCollection, ClientError,
/// };
///
/// fn mock_fetch(req: SearchRequest) -> Result<ItemCollection, ClientError> {
///     Ok(ItemCollection::empty())
/// }
///
/// let req = SearchRequest::new().with_limit(10);
/// let strategy = PaginationStrategy::first_page(10);
/// let mut pages = PageIterator::new(req, strategy, mock_fetch);
///
/// for page_result in &mut pages {
///     let page = page_result.expect("fetch failed");
///     println!("Got {} items", page.len());
/// }
/// ```
pub struct PageIterator<F>
where
    F: Fn(SearchRequest) -> Result<ItemCollection, ClientError>,
{
    request: SearchRequest,
    strategy: PaginationStrategy,
    fetch: F,
    done: bool,
    pages_fetched: u32,
    max_pages: Option<u32>,
}

impl<F> PageIterator<F>
where
    F: Fn(SearchRequest) -> Result<ItemCollection, ClientError>,
{
    /// Create a new page iterator.
    ///
    /// - `request` — the base search request (the strategy will modify it).
    /// - `strategy` — the pagination strategy to use.
    /// - `fetch` — closure that executes one HTTP fetch.
    pub fn new(request: SearchRequest, strategy: PaginationStrategy, fetch: F) -> Self {
        Self {
            request,
            strategy,
            fetch,
            done: false,
            pages_fetched: 0,
            max_pages: None,
        }
    }

    /// Cap the iterator at `max` pages (prevents infinite loops).
    pub fn max_pages(mut self, max: u32) -> Self {
        self.max_pages = Some(max);
        self
    }

    /// Number of pages successfully fetched so far.
    pub fn pages_fetched(&self) -> u32 {
        self.pages_fetched
    }
}

impl<F> Iterator for PageIterator<F>
where
    F: Fn(SearchRequest) -> Result<ItemCollection, ClientError>,
{
    type Item = Result<ItemCollection, ClientError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        // Enforce max_pages cap.
        if let Some(max) = self.max_pages {
            if self.pages_fetched >= max {
                self.done = true;
                return None;
            }
        }

        // Build the request for this page.
        let mut page_request = self.request.clone();
        self.strategy.apply_to_request(&mut page_request);

        // Execute the fetch.
        let response = match (self.fetch)(page_request) {
            Ok(r) => r,
            Err(e) => {
                self.done = true;
                return Some(Err(e));
            }
        };

        self.pages_fetched += 1;

        // Check if we should continue.
        if response.is_empty() {
            self.done = true;
            // Still return this (empty) page so callers see it.
            return Some(Ok(response));
        }

        // Compute the next strategy before returning the page.
        match self.strategy.next_page(&response) {
            Some(next) => self.strategy = next,
            None => self.done = true,
        }

        Some(Ok(response))
    }
}

// ---------------------------------------------------------------------------
// StacApiClient
// ---------------------------------------------------------------------------

/// A transport-agnostic STAC API client.
///
/// Knows how to build request URLs and parse response JSON, but performs no
/// I/O.  All network calls are delegated to the caller.
///
/// # Example
///
/// ```rust
/// use oxigdal_stac::client::{StacApiClient, SearchRequest};
///
/// let client = StacApiClient::new("https://earth-search.aws.element84.com/v1");
/// let req = SearchRequest::new().with_limit(20);
/// let url = client.search_url(&req);
/// // → "https://earth-search.aws.element84.com/v1/search"
/// println!("{}", url);
/// ```
pub struct StacApiClient {
    /// Base URL of the STAC API endpoint (no trailing slash).
    pub base_url: String,

    /// Default page size applied when a request has no explicit limit.
    pub default_limit: u32,
}

impl StacApiClient {
    /// Construct a client with the default page size of 100.
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut url = base_url.into();
        // Normalise trailing slash.
        if url.ends_with('/') {
            url.pop();
        }
        Self {
            base_url: url,
            default_limit: 100,
        }
    }

    /// Override the default page size.
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.default_limit = limit;
        self
    }

    /// Build the `POST /search` URL.
    pub fn search_url(&self, _req: &SearchRequest) -> String {
        format!("{}/search", self.base_url)
    }

    /// Build a `GET /collections/{id}/items` URL with optional pagination.
    pub fn items_url(
        &self,
        collection_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> String {
        let mut url = format!("{}/collections/{}/items", self.base_url, collection_id);
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(l) = limit {
            params.push(("limit".to_string(), l.to_string()));
        }
        if let Some(o) = offset {
            params.push(("offset".to_string(), o.to_string()));
        }
        if !params.is_empty() {
            let qs: String = params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            url = format!("{}?{}", url, qs);
        }
        url
    }

    /// Given an [`ItemCollection`] response, build the next-page
    /// [`SearchRequest`] by copying the original request and applying the
    /// token / link from the response.
    ///
    /// Returns `None` when there is no next page.
    pub fn next_page_request(
        &self,
        response: &ItemCollection,
        req: &SearchRequest,
    ) -> Option<SearchRequest> {
        // Token-based: next token in the response.
        if let Some(token) = response.next_page_token() {
            let mut next = req.clone();
            next.token = Some(token);
            next.page = None;
            return Some(next);
        }

        // Link-based: a "next" link exists.
        if response.next_link().is_some() {
            // Return the request unchanged; the caller must use the link href.
            return Some(req.clone());
        }

        None
    }

    /// Parse a JSON string into an [`ItemCollection`].
    pub fn parse_item_collection(&self, json: &str) -> Result<ItemCollection, ClientError> {
        serde_json::from_str(json).map_err(ClientError::SerdeError)
    }

    /// Parse a JSON string into a [`super::response::StacCollection`].
    pub fn parse_collection(
        &self,
        json: &str,
    ) -> Result<super::response::StacCollection, ClientError> {
        serde_json::from_str(json).map_err(ClientError::SerdeError)
    }

    /// Parse a JSON string into a [`super::response::StacItem`].
    pub fn parse_item(&self, json: &str) -> Result<super::response::StacItem, ClientError> {
        serde_json::from_str(json).map_err(ClientError::SerdeError)
    }

    /// Build a [`PageIterator`] using offset-based pagination.
    pub fn offset_pages<F>(&self, request: SearchRequest, fetch: F) -> PageIterator<F>
    where
        F: Fn(SearchRequest) -> Result<ItemCollection, ClientError>,
    {
        let limit = request.limit.unwrap_or(self.default_limit);
        PageIterator::new(request, PaginationStrategy::first_page(limit), fetch)
    }

    /// Build a [`PageIterator`] using token-based pagination.
    pub fn token_pages<F>(&self, request: SearchRequest, fetch: F) -> PageIterator<F>
    where
        F: Fn(SearchRequest) -> Result<ItemCollection, ClientError>,
    {
        let limit = request.limit.unwrap_or(self.default_limit);
        PageIterator::new(request, PaginationStrategy::first_token_page(limit), fetch)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::client::response::{StacItem, StacLink};

    fn make_item(id: &str) -> StacItem {
        StacItem {
            stac_version: "1.0.0".to_string(),
            stac_extensions: Vec::new(),
            id: id.to_string(),
            type_: "Feature".to_string(),
            geometry: None,
            bbox: None,
            properties: serde_json::json!({ "datetime": "2023-01-01T00:00:00Z" }),
            links: Vec::new(),
            assets: HashMap::new(),
            collection: None,
        }
    }

    fn make_page(items: Vec<StacItem>, next_token: Option<&str>) -> ItemCollection {
        let mut links = Vec::new();
        if let Some(tok) = next_token {
            links.push(StacLink::new(
                "next",
                format!("https://api.example.com/search?token={}", tok),
            ));
        }
        ItemCollection {
            type_: "FeatureCollection".to_string(),
            features: items,
            links,
            context: None,
            number_matched: None,
            number_returned: None,
        }
    }

    // ------------------------------------------------------------------
    // PaginationStrategy tests
    // ------------------------------------------------------------------

    #[test]
    fn test_first_page_creates_offset_zero() {
        let s = PaginationStrategy::first_page(10);
        match &s {
            PaginationStrategy::OffsetBased { offset, limit } => {
                assert_eq!(*offset, 0);
                assert_eq!(*limit, 10);
            }
            _ => unreachable!("Expected OffsetBased"),
        }
    }

    #[test]
    fn test_next_page_increments_offset() {
        let s = PaginationStrategy::first_page(5);
        let page = make_page(
            vec![
                make_item("a"),
                make_item("b"),
                make_item("c"),
                make_item("d"),
                make_item("e"),
            ],
            None,
        );
        let next = s.next_page(&page).expect("should have next page");
        match &next {
            PaginationStrategy::OffsetBased { offset, limit } => {
                assert_eq!(*offset, 5);
                assert_eq!(*limit, 5);
            }
            _ => unreachable!("Expected OffsetBased"),
        }
    }

    #[test]
    fn test_next_page_returns_none_when_fewer_items_than_limit() {
        let s = PaginationStrategy::first_page(10);
        // Only 3 items — fewer than limit → last page.
        let page = make_page(vec![make_item("a"), make_item("b"), make_item("c")], None);
        assert!(s.next_page(&page).is_none());
    }

    #[test]
    fn test_next_page_returns_none_on_empty() {
        let s = PaginationStrategy::first_page(10);
        let page = ItemCollection::empty();
        assert!(s.next_page(&page).is_none());
    }

    #[test]
    fn test_token_strategy_next_page_picks_up_token() {
        let s = PaginationStrategy::first_token_page(10);
        let page = make_page(
            (0..10).map(|i| make_item(&i.to_string())).collect(),
            Some("next-cursor"),
        );
        let next = s.next_page(&page).expect("should have next page");
        match &next {
            PaginationStrategy::TokenBased { token, limit } => {
                assert_eq!(token.as_deref(), Some("next-cursor"));
                assert_eq!(*limit, 10);
            }
            _ => unreachable!("Expected TokenBased"),
        }
    }

    #[test]
    fn test_token_strategy_none_when_no_token() {
        let s = PaginationStrategy::first_token_page(5);
        // No "next" link in the response.
        let page = make_page((0..5).map(|i| make_item(&i.to_string())).collect(), None);
        assert!(s.next_page(&page).is_none());
    }

    #[test]
    fn test_apply_to_request_sets_limit_and_page() {
        let s = PaginationStrategy::OffsetBased {
            offset: 20,
            limit: 10,
        };
        let mut req = SearchRequest::new();
        s.apply_to_request(&mut req);
        assert_eq!(req.limit, Some(10));
        // offset 20 / limit 10 + 1 = page 3
        assert_eq!(req.page, Some(3));
    }

    #[test]
    fn test_apply_to_request_token() {
        let s = PaginationStrategy::TokenBased {
            token: Some("abc".to_string()),
            limit: 25,
        };
        let mut req = SearchRequest::new();
        s.apply_to_request(&mut req);
        assert_eq!(req.limit, Some(25));
        assert_eq!(req.token, Some("abc".to_string()));
    }

    // ------------------------------------------------------------------
    // PageIterator tests
    // ------------------------------------------------------------------

    #[test]
    fn test_page_iterator_two_pages_then_empty() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();

        let fetch = move |_req: SearchRequest| -> Result<ItemCollection, ClientError> {
            let n = cc.fetch_add(1, Ordering::SeqCst);
            match n {
                0 => Ok(make_page(
                    vec![make_item("a"), make_item("b"), make_item("c")],
                    None,
                )),
                _ => Ok(ItemCollection::empty()),
            }
        };

        let req = SearchRequest::new().with_limit(3);
        let strategy = PaginationStrategy::first_page(3);
        let iter = PageIterator::new(req, strategy, fetch);

        let pages: Vec<_> = iter.collect();
        // First page: 3 items (fewer than limit of 3 ... wait, exactly limit).
        // Actually 3 == 3 so iterator would try page 2; page 2 is empty.
        // So we get 2 results: the 3-item page, then the empty page.
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].as_ref().expect("page 0").len(), 3);
        assert_eq!(pages[1].as_ref().expect("page 1").len(), 0);
    }

    #[test]
    fn test_page_iterator_max_pages() {
        let fetch = |_req: SearchRequest| -> Result<ItemCollection, ClientError> {
            Ok(make_page(
                vec![make_item("x"), make_item("y"), make_item("z")],
                None,
            ))
        };

        let req = SearchRequest::new().with_limit(3);
        let strategy = PaginationStrategy::first_page(3);
        let iter = PageIterator::new(req, strategy, fetch).max_pages(2);

        let pages: Vec<_> = iter.collect();
        assert_eq!(pages.len(), 2);
    }

    #[test]
    fn test_page_iterator_pages_fetched_counter() {
        let fetch = |_req: SearchRequest| -> Result<ItemCollection, ClientError> {
            Ok(ItemCollection::empty())
        };

        let req = SearchRequest::new().with_limit(10);
        let strategy = PaginationStrategy::first_page(10);
        let mut iter = PageIterator::new(req, strategy, fetch);
        let _ = iter.next(); // fetch one page
        assert_eq!(iter.pages_fetched(), 1);
    }

    // ------------------------------------------------------------------
    // StacApiClient tests
    // ------------------------------------------------------------------

    #[test]
    fn test_client_search_url() {
        let c = StacApiClient::new("https://api.example.com");
        let req = SearchRequest::new().with_limit(10);
        assert_eq!(c.search_url(&req), "https://api.example.com/search");
    }

    #[test]
    fn test_client_search_url_trailing_slash() {
        let c = StacApiClient::new("https://api.example.com/");
        let req = SearchRequest::new();
        assert_eq!(c.search_url(&req), "https://api.example.com/search");
    }

    #[test]
    fn test_client_items_url_no_pagination() {
        let c = StacApiClient::new("https://api.example.com");
        let url = c.items_url("my-collection", None, None);
        assert_eq!(
            url,
            "https://api.example.com/collections/my-collection/items"
        );
    }

    #[test]
    fn test_client_items_url_with_pagination() {
        let c = StacApiClient::new("https://api.example.com");
        let url = c.items_url("col", Some(10), Some(20));
        assert!(url.contains("limit=10"));
        assert!(url.contains("offset=20"));
    }

    #[test]
    fn test_client_next_page_request_token() {
        let c = StacApiClient::new("https://api.example.com");
        let mut response = ItemCollection::empty();
        response.links.push(StacLink::new(
            "next",
            "https://api.example.com/search?token=page2",
        ));
        let req = SearchRequest::new().with_limit(10);
        let next = c.next_page_request(&response, &req);
        assert!(next.is_some());
        assert_eq!(
            next.expect("next page request").token,
            Some("page2".to_string())
        );
    }

    #[test]
    fn test_client_next_page_request_none_when_no_next() {
        let c = StacApiClient::new("https://api.example.com");
        let response = ItemCollection::empty();
        let req = SearchRequest::new();
        assert!(c.next_page_request(&response, &req).is_none());
    }

    #[test]
    fn test_client_parse_item_collection() {
        let c = StacApiClient::new("https://api.example.com");
        let json = r#"{"type":"FeatureCollection","features":[]}"#;
        let ic = c.parse_item_collection(json).expect("parse");
        assert!(ic.is_empty());
        assert_eq!(ic.type_, "FeatureCollection");
    }

    #[test]
    fn test_client_parse_item() {
        let c = StacApiClient::new("https://api.example.com");
        let json = r#"{
            "stac_version": "1.0.0",
            "type": "Feature",
            "id": "item-abc",
            "geometry": null,
            "properties": { "datetime": "2024-01-01T00:00:00Z" },
            "links": [],
            "assets": {}
        }"#;
        let item = c.parse_item(json).expect("parse");
        assert_eq!(item.id, "item-abc");
        assert_eq!(item.datetime(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn test_client_with_limit() {
        let c = StacApiClient::new("https://api.example.com").with_limit(50);
        assert_eq!(c.default_limit, 50);
    }
}
