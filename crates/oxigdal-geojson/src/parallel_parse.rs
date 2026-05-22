//! Optional rayon-backed parallel parsing for large GeoJSON
//! `FeatureCollection` documents.
//!
//! This module is only compiled when the `parallel` feature is enabled.
//!
//! # Overview
//!
//! Parsing a very large `FeatureCollection` is dominated by the per-feature
//! work of walking each feature's JSON value and materialising the typed
//! [`GeoJsonGeometry`] / [`GeoJsonFeature`] representation.  That work is
//! embarrassingly parallel: every feature is independent.
//!
//! [`parse_features_parallel`] first parses the whole input once into a
//! [`serde_json::Value`] (this part is inherently sequential — it is a single
//! contiguous JSON document), validates that it is a `FeatureCollection`, and
//! then distributes the per-feature parse across a rayon thread pool.  Every
//! feature is parsed through the exact same code path the sequential parser
//! uses ([`crate::parser::parse_feature_value`]), so the resulting features are
//! byte-for-byte identical, and the reconstructed
//! [`FeatureCollection`](crate::parser::FeatureCollection) preserves the same
//! top-level members (`bbox`, 3-D bbox, `crs`, `name`).
//!
//! # Order guarantees
//!
//! With [`ParallelParseOptions::preserve_order`] set to `true` (the default)
//! the output feature order is identical to the input order, and the operation
//! short-circuits on the first malformed feature.  With `preserve_order` set to
//! `false` the *set* of features is identical but the order is unspecified;
//! this can be marginally faster for workloads that do not care about order.
//!
//! # Thread control
//!
//! By default the global rayon pool is used.  Setting
//! [`ParallelParseOptions::threads`] to `Some(n)` builds a *local*
//! [`rayon::ThreadPool`] with `n` threads and runs the parse inside
//! `pool.install(..)`.  Because this is a local pool — not the process-wide
//! global pool installed by `rayon::ThreadPoolBuilder::build_global` — the
//! thread-count override is safe to use repeatedly within a single process.

use crate::error::GeoJsonError;
use crate::parser::{FeatureCollection, feature_collection_from_parts, parse_feature_value};
use crate::types::GeoJsonFeature;

use rayon::prelude::*;

// ─── Options ──────────────────────────────────────────────────────────────────

/// Configuration for [`parse_features_parallel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelParseOptions {
    /// Number of features handed to each rayon work item.
    ///
    /// Larger chunks reduce scheduling overhead; smaller chunks improve load
    /// balancing when per-feature cost varies.  A value of `0` is treated as
    /// `1`.  Defaults to `256`.
    pub chunk_size: usize,
    /// Optional explicit worker-thread count.
    ///
    /// `None` (the default) uses the process-wide global rayon pool.  `Some(n)`
    /// builds a *local* [`rayon::ThreadPool`] with `n` threads for the duration
    /// of the parse.
    ///
    /// NOTE: rayon's [`rayon::ThreadPoolBuilder::build_global`] may be called
    /// only once per process.  This option deliberately avoids it: it uses
    /// [`rayon::ThreadPoolBuilder::build`] to create a local pool and runs the
    /// parallel section inside [`rayon::ThreadPool::install`], so it is safe to
    /// call repeatedly (e.g. across many tests or many parse invocations).
    pub threads: Option<usize>,
    /// When `true` (the default) the output preserves input feature order and
    /// short-circuits on the first parse error.  When `false` the resulting set
    /// of features is identical but order is unspecified.
    pub preserve_order: bool,
    /// Optional coordinate precision.
    ///
    /// `Some(d)` rounds every coordinate of every parsed geometry to `d`
    /// decimal places.  `None` (the default) preserves coordinates exactly as
    /// parsed, matching the sequential parser.
    pub coordinate_precision: Option<u8>,
}

impl Default for ParallelParseOptions {
    fn default() -> Self {
        Self {
            chunk_size: 256,
            threads: None,
            preserve_order: true,
            coordinate_precision: None,
        }
    }
}

impl ParallelParseOptions {
    /// Create options with default settings (identical to [`Default`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the per-work-item feature chunk size (builder style).
    ///
    /// A value of `0` is normalised to `1` when the parse runs.
    #[must_use]
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Set the explicit worker-thread count (builder style).
    ///
    /// Pass `Some(n)` to use a local pool of `n` threads, or `None` to use the
    /// global pool.  See [`ParallelParseOptions::threads`] for details on why
    /// this is safe to call repeatedly.
    #[must_use]
    pub fn with_threads(mut self, threads: Option<usize>) -> Self {
        self.threads = threads;
        self
    }

    /// Set whether input order is preserved in the output (builder style).
    #[must_use]
    pub fn with_preserve_order(mut self, preserve_order: bool) -> Self {
        self.preserve_order = preserve_order;
        self
    }

    /// Set the optional coordinate precision (builder style).
    #[must_use]
    pub fn with_coordinate_precision(mut self, coordinate_precision: Option<u8>) -> Self {
        self.coordinate_precision = coordinate_precision;
        self
    }

    /// Effective chunk size, clamping `0` to `1`.
    #[inline]
    fn effective_chunk_size(&self) -> usize {
        self.chunk_size.max(1)
    }
}

// ─── Parsing ──────────────────────────────────────────────────────────────────

/// Parse a GeoJSON `FeatureCollection` string in parallel.
///
/// The whole input is first parsed once into a [`serde_json::Value`]
/// (sequentially), validated to be a `FeatureCollection` carrying a `features`
/// array, and then each feature is parsed in parallel through the same code
/// path as the sequential parser.
///
/// # Errors
///
/// Returns [`GeoJsonError`] if:
/// - the input is not valid JSON,
/// - the top-level `type` is not `"FeatureCollection"` (or is missing),
/// - the `features` member is missing or is not an array,
/// - any individual feature is malformed,
/// - a local thread pool could not be constructed (only when
///   [`ParallelParseOptions::threads`] is `Some`).
pub fn parse_features_parallel(
    s: &str,
    options: &ParallelParseOptions,
) -> Result<FeatureCollection, GeoJsonError> {
    // 1. Parse the whole document once (sequential — single JSON document).
    let value: serde_json::Value = serde_json::from_str(s)?;

    // Validate it is a FeatureCollection.
    let type_ = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GeoJsonError::MissingField("type".into()))?;
    if type_ != "FeatureCollection" {
        return Err(GeoJsonError::InvalidType {
            expected: "FeatureCollection".into(),
            got: type_.into(),
        });
    }

    // 2. Borrow the features array.
    let features_arr = value
        .get("features")
        .and_then(|f| f.as_array())
        .ok_or_else(|| GeoJsonError::MissingField("features".into()))?;

    // 3. Drive the parallel parse.
    let features = parse_feature_slice(features_arr, options)?;

    // 7. Reconstruct the collection with the same top-level members the
    //    sequential parser preserves (bbox / 3-D bbox / crs / name).
    Ok(feature_collection_from_parts(features, &value))
}

/// Parse a `FeatureCollection` string in parallel using
/// [`ParallelParseOptions::default`].
///
/// # Errors
///
/// See [`parse_features_parallel`].
pub fn parse_features_parallel_default(s: &str) -> Result<FeatureCollection, GeoJsonError> {
    parse_features_parallel(s, &ParallelParseOptions::default())
}

// ─── Internal driver ────────────────────────────────────────────────────────

/// Parse a slice of feature JSON values into typed features, honouring the
/// configured thread pool, chunk size and ordering policy.
fn parse_feature_slice(
    features_arr: &[serde_json::Value],
    options: &ParallelParseOptions,
) -> Result<Vec<GeoJsonFeature>, GeoJsonError> {
    let chunk_size = options.effective_chunk_size();
    let precision = options.coordinate_precision;
    let preserve_order = options.preserve_order;

    // The actual parallel computation, expressed once and run either on the
    // global pool or inside a local pool.
    let run = || -> Result<Vec<GeoJsonFeature>, GeoJsonError> {
        if preserve_order {
            parse_chunks_ordered(features_arr, chunk_size, precision)
        } else {
            parse_chunks_unordered(features_arr, chunk_size, precision)
        }
    };

    match options.threads {
        Some(n) => {
            // 4. Local pool — avoids the once-per-process `build_global`.
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build()
                .map_err(|e| {
                    GeoJsonError::InvalidCoordinates(format!(
                        "failed to build rayon thread pool with {n} threads: {e}"
                    ))
                })?;
            pool.install(run)
        }
        // 4. Global pool — just use `par_iter` directly.
        None => run(),
    }
}

/// Order-preserving parallel parse.
///
/// Each chunk is parsed independently into its own `Vec`; the per-chunk results
/// are then concatenated in input order.  Collecting into
/// `Result<Vec<Vec<_>>, _>` short-circuits on the first error encountered.
fn parse_chunks_ordered(
    features_arr: &[serde_json::Value],
    chunk_size: usize,
    precision: Option<u8>,
) -> Result<Vec<GeoJsonFeature>, GeoJsonError> {
    let per_chunk: Vec<Vec<GeoJsonFeature>> = features_arr
        .par_chunks(chunk_size)
        .map(|chunk| {
            chunk
                .iter()
                .map(|v| parse_feature_value(v, precision))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<Vec<_>>, _>>()?;

    let total: usize = per_chunk.iter().map(Vec::len).sum();
    let mut features = Vec::with_capacity(total);
    for chunk in per_chunk {
        features.extend(chunk);
    }
    Ok(features)
}

/// Order-agnostic parallel parse.
///
/// Produces the same *set* of features as [`parse_chunks_ordered`] but does not
/// guarantee their order.  Still collects into `Result<_, _>` so the first
/// malformed feature aborts the whole parse.
fn parse_chunks_unordered(
    features_arr: &[serde_json::Value],
    chunk_size: usize,
    precision: Option<u8>,
) -> Result<Vec<GeoJsonFeature>, GeoJsonError> {
    features_arr
        .par_chunks(chunk_size)
        .map(|chunk| {
            chunk
                .iter()
                .map(|v| parse_feature_value(v, precision))
                .collect::<Result<Vec<_>, _>>()
        })
        .reduce(
            || Ok(Vec::new()),
            |acc, next| match (acc, next) {
                (Ok(mut a), Ok(b)) => {
                    a.extend(b);
                    Ok(a)
                }
                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
            },
        )
}
