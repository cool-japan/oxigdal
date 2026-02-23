//! GraphQL query resolvers.

/// Query helper functions.
pub mod helpers {
    use crate::error::Result;

    /// Parses pagination parameters.
    pub fn parse_pagination(limit: Option<i32>, offset: Option<i32>) -> (i32, i32) {
        let limit = limit.unwrap_or(10).min(100); // Max 100 items
        let offset = offset.unwrap_or(0).max(0);
        (limit, offset)
    }

    /// Builds a search filter.
    pub fn build_search_filter(query: &str) -> Result<String> {
        Ok(format!("%{}%", query))
    }
}
