//! Vector tile cache system with HTTP/2 server push hints.
//!
//! Provides LRU tile caching, prefetching, HTTP/2 push hint generation,
//! ETag validation, and unified tile serving logic.

pub mod cache;
pub mod http2_push;

pub use cache::{
    CacheStats, CachedTile, TileCache, TileEncoding, TileFormat, TileKey, TilePrefetcher,
};
pub use http2_push::{
    ETagValidator, PushHint, PushPolicy, PushRel, TileResponse, TileResponseStatus, TileServer,
};
