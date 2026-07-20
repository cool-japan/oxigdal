//! Pagination support for STAC API searches.
//!
//! This module provides utilities for paginated STAC API searches.

#[cfg(feature = "async")]
use crate::error::Result;
use crate::search::{SearchParams, SearchResults, StacClient};

/// Paginator for iterating through STAC search results.
#[cfg(feature = "async")]
#[derive(Debug, Clone)]
pub struct Paginator {
    /// STAC client.
    client: StacClient,
    /// Search parameters.
    params: SearchParams,
    /// Current page token.
    page_token: Option<String>,
    /// Whether there are more pages.
    has_more: bool,
}

#[cfg(feature = "async")]
impl Paginator {
    /// Creates a new paginator.
    ///
    /// # Arguments
    ///
    /// * `client` - STAC client
    /// * `params` - Search parameters
    ///
    /// # Returns
    ///
    /// A new Paginator instance
    pub fn new(client: StacClient, params: SearchParams) -> Self {
        Self {
            client,
            params,
            page_token: None,
            has_more: true,
        }
    }

    /// Fetches the next page of results.
    ///
    /// # Returns
    ///
    /// Next page of search results, or `None` if no more pages
    #[cfg(feature = "async")]
    pub async fn next_page(&mut self) -> Result<Option<SearchResults>> {
        if !self.has_more {
            return Ok(None);
        }

        // Add page token to params if present
        let mut params = self.params.clone();
        if let Some(token) = &self.page_token {
            params.page_token = Some(token.clone());
        }

        let results = self.client.execute_search(&params).await?;

        // Update paginator state
        self.has_more = results.has_more();
        self.page_token = results.get_next_token();

        Ok(Some(results))
    }

    /// Collects all results across all pages.
    ///
    /// # Returns
    ///
    /// All items from all pages
    ///
    /// # Warning
    ///
    /// This can be memory-intensive for large result sets.
    #[cfg(feature = "async")]
    pub async fn collect_all(&mut self) -> Result<Vec<crate::item::Item>> {
        let mut all_items = Vec::new();

        while let Some(results) = self.next_page().await? {
            all_items.extend(results.features);
        }

        Ok(all_items)
    }

    /// Collects up to a maximum number of results across pages.
    ///
    /// # Arguments
    ///
    /// * `max_items` - Maximum number of items to collect
    ///
    /// # Returns
    ///
    /// Items up to the specified maximum
    #[cfg(feature = "async")]
    pub async fn collect_up_to(&mut self, max_items: usize) -> Result<Vec<crate::item::Item>> {
        let mut all_items = Vec::new();

        while let Some(results) = self.next_page().await? {
            for item in results.features {
                if all_items.len() >= max_items {
                    return Ok(all_items);
                }
                all_items.push(item);
            }
        }

        Ok(all_items)
    }

    /// Returns a lazy streaming iterator over all items across all pages.
    ///
    /// Unlike [`collect_all`][Self::collect_all], this does **not** buffer the entire result set in
    /// memory.  Items are fetched one page at a time and yielded individually as they become
    /// available.  This is the correct choice for large catalogs (e.g. sentinel-2-l2a 2024 with
    /// >10⁶ items).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # #[cfg(feature = "async")]
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// use futures::{StreamExt, pin_mut};
    /// use oxigdal_stac::{SearchParams, StacClient};
    ///
    /// let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;
    /// let paginator = client.search().limit(100).paginate();
    ///
    /// let stream = paginator.stream();
    /// pin_mut!(stream);
    /// while let Some(result) = stream.next().await {
    ///     let item = result?;
    ///     println!("{}", item.id);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "async")]
    pub fn stream(self) -> impl futures::Stream<Item = Result<crate::item::Item>> {
        futures::stream::unfold(
            // State: (paginator, buffered items from current page, next index to yield)
            (self, Vec::<crate::item::Item>::new(), 0usize),
            |(mut pag, mut items, mut idx)| async move {
                loop {
                    // If there are items remaining in the current page buffer, yield one.
                    if idx < items.len() {
                        let item = items[idx].clone();
                        idx += 1;
                        return Some((Ok(item), (pag, items, idx)));
                    }

                    // Buffer exhausted — fetch the next page.
                    match pag.next_page().await {
                        Ok(Some(results)) => {
                            items = results.features;
                            idx = 0;
                            // Loop back: either yield the first item of the new page,
                            // or fetch yet another page if this one was empty.
                        }
                        Ok(None) => return None,
                        Err(e) => {
                            // Mark the paginator as exhausted so that if the caller
                            // polls the stream again after an error we do not attempt
                            // another HTTP request — we simply end the stream.
                            pag.has_more = false;
                            return Some((Err(e), (pag, Vec::new(), 0)));
                        }
                    }
                }
            },
        )
    }

    /// Resets the paginator to start from the beginning.
    pub fn reset(&mut self) {
        self.page_token = None;
        self.has_more = true;
    }

    /// Returns whether there are more pages to fetch.
    pub fn has_more(&self) -> bool {
        self.has_more
    }
}

/// Cursor-based pagination strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPagination {
    /// Cursor token for the next page.
    pub next: Option<String>,
    /// Cursor token for the previous page.
    pub prev: Option<String>,
}

/// Token-based pagination strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPagination {
    /// Page token for the next page.
    pub token: String,
}

/// Page-based pagination strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagePagination {
    /// Current page number (1-indexed).
    pub page: u32,
    /// Number of items per page.
    pub per_page: u32,
    /// Total number of pages.
    pub total_pages: Option<u32>,
    /// Total number of items.
    pub total_items: Option<u64>,
}

impl PagePagination {
    /// Creates a new page pagination.
    ///
    /// `page` is 1-indexed; `0` is normalized to `1` (there is no "page
    /// zero") so that callers can never construct an instance whose
    /// [`offset()`][Self::offset] computation would underflow or panic.
    pub fn new(page: u32, per_page: u32) -> Self {
        Self {
            page: page.max(1),
            per_page,
            total_pages: None,
            total_items: None,
        }
    }

    /// Returns the offset for the current page.
    ///
    /// `page` is documented as 1-indexed, but this struct derives
    /// `Deserialize` with fully public fields, so a value with `page == 0`
    /// can still reach this method (e.g. parsed from an untrusted STAC
    /// server's pagination metadata, or built via a struct literal that
    /// bypasses [`new()`][Self::new]). Treat `page == 0` the same as
    /// `page == 1` (offset `0`) instead of underflowing, and widen to `u64`
    /// before multiplying to avoid a `u32` overflow for large `per_page`.
    pub fn offset(&self) -> u64 {
        u64::from(self.page.saturating_sub(1)) * u64::from(self.per_page)
    }

    /// Returns whether there is a next page.
    pub fn has_next(&self) -> bool {
        self.total_pages.is_none_or(|total| self.page < total)
    }

    /// Returns whether there is a previous page.
    pub fn has_prev(&self) -> bool {
        self.page > 1
    }

    /// Returns the next page number.
    pub fn next_page(&self) -> Option<u32> {
        if self.has_next() {
            Some(self.page + 1)
        } else {
            None
        }
    }

    /// Returns the previous page number.
    pub fn prev_page(&self) -> Option<u32> {
        if self.has_prev() {
            Some(self.page - 1)
        } else {
            None
        }
    }
}

use serde::{Deserialize, Serialize};

impl SearchResults {
    /// Gets the next page token if available.
    ///
    /// # Returns
    ///
    /// The next page token if it exists
    pub fn get_next_token(&self) -> Option<String> {
        self.links.as_ref().and_then(|links| {
            links
                .iter()
                .find(|link| link.rel == "next")
                .and_then(|link| {
                    // Try to extract token from URL query params
                    if let Ok(url) = url::Url::parse(&link.href) {
                        url.query_pairs()
                            .find(|(k, _)| k == "token" || k == "page_token" || k == "next")
                            .map(|(_, v)| v.to_string())
                    } else {
                        None
                    }
                })
        })
    }
}

#[cfg(test)]
#[cfg(feature = "async")]
mod tests {
    use super::*;

    #[test]
    fn test_page_pagination_new() {
        let pagination = PagePagination::new(1, 10);
        assert_eq!(pagination.page, 1);
        assert_eq!(pagination.per_page, 10);
        assert_eq!(pagination.offset(), 0);
    }

    #[test]
    fn test_page_pagination_offset() {
        let pagination = PagePagination::new(3, 20);
        assert_eq!(pagination.offset(), 40);
    }

    #[test]
    fn test_page_pagination_new_normalizes_page_zero() {
        // `new(0, ..)` must not underflow/panic; page 0 is treated as page 1.
        let pagination = PagePagination::new(0, 10);
        assert_eq!(pagination.page, 1);
        assert_eq!(pagination.offset(), 0);
    }

    #[test]
    fn test_page_pagination_offset_page_zero_struct_literal() {
        // Public fields let callers bypass `new()` entirely; `offset()` must
        // still not underflow/panic when `page == 0`.
        let pagination = PagePagination {
            page: 0,
            per_page: 10,
            total_pages: None,
            total_items: None,
        };
        assert_eq!(pagination.offset(), 0);
    }

    #[test]
    fn test_page_pagination_offset_page_zero_deserialized() {
        // An untrusted STAC server's pagination metadata could deserialize
        // to `page: 0`; `offset()` must not underflow/panic in that case.
        let pagination: PagePagination =
            serde_json::from_str(r#"{"page":0,"per_page":10}"#).expect("valid JSON");
        assert_eq!(pagination.page, 0);
        assert_eq!(pagination.offset(), 0);
    }

    #[test]
    fn test_page_pagination_navigation() {
        let mut pagination = PagePagination::new(2, 10);
        pagination.total_pages = Some(5);

        assert!(pagination.has_prev());
        assert!(pagination.has_next());
        assert_eq!(pagination.prev_page(), Some(1));
        assert_eq!(pagination.next_page(), Some(3));
    }

    #[test]
    fn test_page_pagination_boundaries() {
        let mut pagination = PagePagination::new(1, 10);
        pagination.total_pages = Some(1);

        assert!(!pagination.has_prev());
        assert!(!pagination.has_next());
        assert_eq!(pagination.prev_page(), None);
        assert_eq!(pagination.next_page(), None);
    }

    #[test]
    fn test_paginator_creation() {
        let client = StacClient::new("https://earth-search.aws.element84.com/v1")
            .expect("Failed to create client");
        let params = SearchParams::default();
        let paginator = Paginator::new(client, params);

        assert!(paginator.has_more());
        assert!(paginator.page_token.is_none());
    }

    #[test]
    fn test_paginator_reset() {
        let client = StacClient::new("https://earth-search.aws.element84.com/v1")
            .expect("Failed to create client");
        let params = SearchParams::default();
        let mut paginator = Paginator::new(client, params);

        paginator.has_more = false;
        paginator.page_token = Some("token".to_string());

        paginator.reset();

        assert!(paginator.has_more());
        assert!(paginator.page_token.is_none());
    }
}
