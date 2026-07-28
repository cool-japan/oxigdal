//! # OxiGeo WASM GeoParquet — remote GeoParquet queries in the browser
//!
//! WebAssembly bindings that let a browser query a multi-gigabyte remote
//! GeoParquet file with **no server component**: the Parquet footer is
//! fetched once via HTTP range requests, bounding-box and attribute
//! predicates are planned against row-group metadata (predicate pushdown),
//! and only the surviving column chunks are downloaded and decoded.
//!
//! ## Modules
//!
//! - `sparse` — [`parquet`] `ChunkReader` over sparse prefetched byte segments
//! - `coalesce` — column-chunk byte ranges deduped and merged into few HTTP fetches
//! - `chunk_cache` — byte-capacity-bounded true-LRU cache for fetched column chunks
//! - `error` — typed error surface (`GpqLiveError`), JS-serializable on wasm
//! - `filter_expr` — SQL `WHERE` fragment → `AttributeFilter` lowering (sqlparser)
//! - `convert` — Arrow `RecordBatch` → GeoJSON `FeatureCollection` output
//! - `fetch` (wasm32 only) — HTTP range fetching via `web_sys` with byte accounting
//! - `session` (wasm32 only) — `RemoteGeoParquet` open / plan / query bindings
//!
//! This crate deliberately does **not** depend on `oxigeo-wasm`: that
//! crate's `#[wasm_bindgen]` exports are link roots and would pull the
//! whole COG viewer into this bundle.
//!
//! Module scaffold created by WP W0; implementations land in WP C2-C4.

mod chunk_cache;
mod coalesce;
mod convert;
mod error;
mod filter_expr;
mod sparse;

#[cfg(target_arch = "wasm32")]
mod fetch;
#[cfg(target_arch = "wasm32")]
mod session;
