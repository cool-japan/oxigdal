//! Query parameter types for OGC Features API requests.

use super::types::DateTimeFilter;

/// Language used for the `filter` parameter
#[derive(Debug, Clone, PartialEq)]
pub enum FilterLang {
    /// CQL2 text encoding
    Cql2Text,
    /// CQL2 JSON encoding
    Cql2Json,
}

/// Query parameters for `GET /collections/{id}/items`
#[derive(Debug, Clone, Default)]
pub struct QueryParams {
    /// Maximum number of features to return (server default: 10, max: 10 000)
    pub limit: Option<u32>,

    /// Number of features to skip (for pagination)
    pub offset: Option<u32>,

    /// Spatial filter as `[xmin, ymin, xmax, ymax]`
    pub bbox: Option<[f64; 4]>,

    /// CRS for the bbox (Part 2)
    pub bbox_crs: Option<String>,

    /// Temporal filter
    pub datetime: Option<DateTimeFilter>,

    /// CQL2 filter expression
    pub filter: Option<String>,

    /// Language of the filter expression
    pub filter_lang: Option<FilterLang>,

    /// CRS for the response geometry (Part 2)
    pub crs: Option<String>,

    /// Property selection (return only these properties)
    pub properties: Option<Vec<String>>,
}

impl QueryParams {
    /// Effective limit, applying the default of 10.
    pub fn effective_limit(&self) -> u32 {
        self.limit.unwrap_or(10)
    }

    /// Effective offset, applying the default of 0.
    pub fn effective_offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }
}
