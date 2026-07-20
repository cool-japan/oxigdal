//! `GpqLiveError` — typed error surface for remote GeoParquet queries.
//!
//! Covers fetch failures, non-prefetched range reads, filter-expression
//! rejections, too-broad query guards, and wrapped Parquet / GeoParquet
//! errors. On wasm32 a `From<GpqLiveError> for JsValue` impl serializes
//! `{code, message, detail}` JSON for the JavaScript caller.
//!
//! Implemented by WP C2 (GeoParquet Live lane); stub created by WP W0.

// Consumed by the wasm-only `session` bindings and the `filter_expr` lowering
// (WP C3/C4); until those land some variants/helpers look unused.
#![allow(dead_code)]

use thiserror::Error;

/// Errors surfaced by the remote GeoParquet query pipeline.
///
/// Every variant carries enough structured context to drive the demo UI:
/// `Fetch` reports the HTTP status and URL, `TooBroad` reports the guard inputs,
/// and the wrapped `Parquet` / `Geo` variants preserve the underlying decoder
/// error. On wasm the [`From`] conversion to `JsValue` renders a stable
/// `{code, message, detail}` JSON payload for JavaScript error handling.
#[derive(Debug, Error)]
pub enum GpqLiveError {
    /// An HTTP range request did not return usable partial content.
    #[error("HTTP fetch failed (status {status}) for {url}")]
    Fetch {
        /// HTTP status code (`0` indicates a network / JS-level failure).
        status: u16,
        /// The URL that was requested.
        url: String,
    },

    /// A decoder read fell outside every prefetched byte segment.
    #[error("range {start}+{len} not prefetched")]
    RangeNotPrefetched {
        /// Absolute start offset of the missed read.
        start: u64,
        /// Length of the missed read in bytes.
        len: u64,
    },

    /// A `WHERE`-clause fragment could not be lowered to a supported filter.
    #[error("invalid filter expression: {msg}")]
    FilterExpr {
        /// Human-readable reason naming the unsupported construct.
        msg: String,
    },

    /// The planned query would scan too many row groups / bytes.
    #[error("query too broad: {row_groups} row groups (~{estimated_bytes} bytes)")]
    TooBroad {
        /// Number of surviving row groups the plan would scan.
        row_groups: usize,
        /// Estimated bytes the plan would download.
        estimated_bytes: u64,
    },

    /// A wrapped Parquet decoding / metadata error.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// A wrapped GeoParquet driver error.
    #[error("geoparquet error: {0}")]
    Geo(#[from] oxigeo_geoparquet::GeoParquetError),
}

impl GpqLiveError {
    /// A stable, machine-readable code identifying the error variant.
    ///
    /// The JavaScript demo switches on these codes to render tailored guidance
    /// (e.g. widen/narrow the query for `too_broad`).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Fetch { .. } => "fetch",
            Self::RangeNotPrefetched { .. } => "range_not_prefetched",
            Self::FilterExpr { .. } => "filter_expr",
            Self::TooBroad { .. } => "too_broad",
            Self::Parquet(_) => "parquet",
            Self::Geo(_) => "geo",
        }
    }

    /// Structured, variant-specific detail as a JSON value.
    ///
    /// Returns `serde_json::Value::Null` for the opaque wrapped variants.
    #[must_use]
    pub fn detail(&self) -> serde_json::Value {
        match self {
            Self::Fetch { status, url } => serde_json::json!({ "status": status, "url": url }),
            Self::RangeNotPrefetched { start, len } => {
                serde_json::json!({ "start": start, "len": len })
            }
            Self::FilterExpr { msg } => serde_json::json!({ "msg": msg }),
            Self::TooBroad {
                row_groups,
                estimated_bytes,
            } => serde_json::json!({
                "rowGroups": row_groups,
                "estimatedBytes": estimated_bytes,
            }),
            Self::Parquet(_) | Self::Geo(_) => serde_json::Value::Null,
        }
    }

    /// The full `{code, message, detail}` payload as a JSON value.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code(),
            "message": self.to_string(),
            "detail": self.detail(),
        })
    }
}

#[cfg(target_arch = "wasm32")]
impl From<GpqLiveError> for wasm_bindgen::JsValue {
    fn from(err: GpqLiveError) -> Self {
        // Serialize to a compact JSON string; the JS caller does `JSON.parse`.
        wasm_bindgen::JsValue::from_str(&err.to_json().to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_per_variant() {
        assert_eq!(
            GpqLiveError::Fetch {
                status: 404,
                url: "u".into()
            }
            .code(),
            "fetch"
        );
        assert_eq!(
            GpqLiveError::RangeNotPrefetched { start: 1, len: 2 }.code(),
            "range_not_prefetched"
        );
        assert_eq!(
            GpqLiveError::FilterExpr { msg: "x".into() }.code(),
            "filter_expr"
        );
        assert_eq!(
            GpqLiveError::TooBroad {
                row_groups: 9,
                estimated_bytes: 100
            }
            .code(),
            "too_broad"
        );
    }

    #[test]
    fn too_broad_detail_uses_camel_case_keys() {
        let e = GpqLiveError::TooBroad {
            row_groups: 5000,
            estimated_bytes: 6_000_000,
        };
        let d = e.detail();
        assert_eq!(d["rowGroups"], 5000);
        assert_eq!(d["estimatedBytes"], 6_000_000u64);
    }

    #[test]
    fn fetch_detail_carries_status_and_url() {
        let e = GpqLiveError::Fetch {
            status: 503,
            url: "https://example/x.parquet".into(),
        };
        let d = e.detail();
        assert_eq!(d["status"], 503);
        assert_eq!(d["url"], "https://example/x.parquet");
    }

    #[test]
    fn to_json_bundles_code_message_detail() {
        let e = GpqLiveError::RangeNotPrefetched { start: 42, len: 7 };
        let j = e.to_json();
        assert_eq!(j["code"], "range_not_prefetched");
        assert_eq!(j["message"], "range 42+7 not prefetched");
        assert_eq!(j["detail"]["start"], 42);
        assert_eq!(j["detail"]["len"], 7);
    }

    #[test]
    fn wrapped_variants_have_null_detail() {
        let e = GpqLiveError::Parquet(parquet::errors::ParquetError::General("boom".into()));
        assert_eq!(e.code(), "parquet");
        assert!(e.detail().is_null());
        assert!(e.to_string().contains("boom"));
    }

    #[test]
    fn from_parquet_error_conversion() {
        let pe = parquet::errors::ParquetError::General("x".into());
        let e: GpqLiveError = pe.into();
        assert_eq!(e.code(), "parquet");
    }
}
