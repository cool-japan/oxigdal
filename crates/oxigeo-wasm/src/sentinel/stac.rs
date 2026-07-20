//! STAC pairing client for Sentinel-2 L2A scenes (`WasmStacClient`).
//!
//! Builds `oxigeo-stac` `SearchRequest`s, POSTs them to Earth Search v1
//! via `web_sys::fetch`, and pairs same-grid scenes for two dates with
//! client-side cloud / nodata filtering.
//!
//! # Design (GeoSentinel contract §2.3, WP A3)
//!
//! - One `POST {base}/search` per date with
//!   `collections=["sentinel-2-l2a"]`, the caller's bbox, a
//!   `[date-window, date+window]` datetime interval, and `limit=100`.
//! - Earth Search's Item Search endpoint has no legacy `query` extension,
//!   so cloud-cover and nodata filtering happen **client-side** on the
//!   parsed [`StacItem`]s (`eo:cloud_cover`, `s2:nodata_pixel_percentage`).
//! - Scenes are grouped by MGRS tile (`grid:code`); only grids present on
//!   **both** dates are eligible (contract D8: same-grid pairing guarantees
//!   an identical UTM pixel grid). The primary pair is the grid whose
//!   lowest-cloud scenes have the lowest combined cloud cover; remaining
//!   same-grid scenes are emitted as alternates so the UI can swap either
//!   side without breaking grid alignment.
//! - Asset hrefs are resolved by trying the Earth Search keys
//!   `red`/`nir`/`visual` first, then the band-name keys `B04`/`B08`/`TCI`
//!   used by other Sentinel-2 catalogs.
//!
//! The pairing core ([`pair_candidates`]) is pure and unit-tested natively;
//! only the fetch path (`WasmStacClient::search_pair`) is wasm-gated.
//!
//! Implemented by WP A3 (GeoSentinel lane); stub registered by WP W0.

use std::collections::BTreeMap;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use oxigeo_stac::chrono::{NaiveDate, TimeDelta};
use oxigeo_stac::client::{SearchRequest, StacItem};

#[cfg(target_arch = "wasm32")]
use oxigeo_stac::client::{ItemCollection, StacApiClient};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
#[cfg(target_arch = "wasm32")]
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

/// Default STAC API root: Earth Search v1 (Element 84, Sentinel-2 L2A COGs).
pub const DEFAULT_STAC_BASE_URL: &str = "https://earth-search.aws.element84.com/v1";

/// STAC collection id searched by `WasmStacClient::search_pair`.
pub const SENTINEL2_L2A_COLLECTION: &str = "sentinel-2-l2a";

/// Scenes whose `s2:nodata_pixel_percentage` exceeds this are dropped.
pub const MAX_NODATA_PCT: f64 = 10.0;

/// Page-size limit sent with every search request.
pub const SEARCH_PAGE_LIMIT: u32 = 100;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced while building searches or pairing scene candidates.
#[derive(Debug, thiserror::Error)]
pub enum StacPairError {
    /// The bbox argument was not a JSON array of exactly four finite numbers.
    #[error("invalid bbox JSON (expected [west, south, east, north]): {message}")]
    InvalidBbox {
        /// Underlying parse / validation detail.
        message: String,
    },

    /// A date argument did not start with a `YYYY-MM-DD` calendar date.
    #[error("invalid date '{input}': expected a YYYY-MM-DD prefix")]
    InvalidDate {
        /// The offending input string.
        input: String,
    },

    /// The `date ± window_days` interval left the supported calendar range.
    #[error("date window out of range: {date} ± {window_days} days")]
    WindowOutOfRange {
        /// Center date of the interval.
        date: NaiveDate,
        /// Half-width of the interval in days.
        window_days: u32,
    },

    /// One date's search returned no scene that survived filtering.
    #[error("no usable scene for date {side} after cloud/nodata filtering")]
    NoCandidates {
        /// Which date failed: `'A'` or `'B'`.
        side: char,
    },

    /// No MGRS grid tile had usable scenes on both dates.
    #[error("no MGRS grid tile has usable scenes on both dates")]
    NoCommonGrid,

    /// Serializing a request or the pair result failed.
    #[error("JSON serialization failed: {message}")]
    Serialization {
        /// Underlying serde error detail.
        message: String,
    },

    /// The STAC response body could not be parsed as an item collection.
    #[error("STAC response parse failed: {message}")]
    ResponseParse {
        /// Underlying parse error detail.
        message: String,
    },

    /// The browser fetch failed (network, CORS, or non-2xx status).
    #[error("STAC network error: {message}")]
    Network {
        /// Failure detail (fetch error or HTTP status line).
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Candidate / pair result types
// ---------------------------------------------------------------------------

/// One usable Sentinel-2 scene, extracted from a [`StacItem`].
///
/// Serializes to the contract JSON shape:
/// `{id, datetime, cloud, gridCode, epsg, redHref, nirHref, visualHref,
///   boaOffsetApplied, nodataPct}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneCandidate {
    /// STAC item id (e.g. `S2A_54SUE_20240511_0_L2A`).
    pub id: String,
    /// Acquisition datetime (RFC 3339, from the item's `datetime` property).
    pub datetime: String,
    /// `eo:cloud_cover` percentage (0–100).
    pub cloud: f64,
    /// MGRS tile code (`grid:code`, e.g. `MGRS-54SUE`).
    pub grid_code: String,
    /// UTM EPSG code of the scene grid (`proj:epsg`, e.g. `32654`).
    pub epsg: u32,
    /// Href of the red band COG (asset `red` or `B04`).
    pub red_href: String,
    /// Href of the NIR band COG (asset `nir` or `B08`).
    pub nir_href: String,
    /// Href of the true-color COG (asset `visual` or `TCI`), if present.
    pub visual_href: Option<String>,
    /// `earthsearch:boa_offset_applied` (missing ⇒ `false`).
    pub boa_offset_applied: bool,
    /// `s2:nodata_pixel_percentage` (missing ⇒ `0.0`).
    pub nodata_pct: f64,
}

/// The primary scene pair: lowest combined cloud cover on a common grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScenePair {
    /// Best scene for date A.
    pub a: SceneCandidate,
    /// Best scene for date B.
    pub b: SceneCandidate,
}

/// Full pairing result.
///
/// Serializes to
/// `{"pair":{"a":…,"b":…},"alternatesA":[…],"alternatesB":[…]}`.
/// Alternates are the remaining scenes of the **primary pair's grid**
/// (cloud-ascending), so swapping either side preserves grid alignment.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResult {
    /// The primary pair.
    pub pair: ScenePair,
    /// Other usable date-A scenes on the primary grid, cloud-ascending.
    pub alternates_a: Vec<SceneCandidate>,
    /// Other usable date-B scenes on the primary grid, cloud-ascending.
    pub alternates_b: Vec<SceneCandidate>,
}

// ---------------------------------------------------------------------------
// Pure core: extraction, filtering, pairing
// ---------------------------------------------------------------------------

/// Resolve an asset href by trying `keys` in order.
fn asset_href(item: &StacItem, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| item.assets.get(*key).map(|asset| asset.href.clone()))
}

/// Extract a [`SceneCandidate`] from a STAC item.
///
/// Returns `None` when a required field is missing: `datetime`,
/// `eo:cloud_cover`, `grid:code`, `proj:epsg`, or the red / NIR asset
/// (tried as `red`/`B04` and `nir`/`B08`). The visual asset
/// (`visual`/`TCI`) is optional. A missing
/// `earthsearch:boa_offset_applied` defaults to `false` (the downstream
/// pipeline then applies the DN−1000 offset itself), and a missing
/// `s2:nodata_pixel_percentage` defaults to `0.0`.
pub fn candidate_from_item(item: &StacItem) -> Option<SceneCandidate> {
    let datetime = item.datetime()?.to_string();
    let cloud = item.cloud_cover()?;
    let grid_code: String = item.get_property("grid:code")?;
    let epsg: u32 = item.get_property("proj:epsg")?;
    let red_href = asset_href(item, &["red", "B04"])?;
    let nir_href = asset_href(item, &["nir", "B08"])?;
    let visual_href = asset_href(item, &["visual", "TCI"]);
    let boa_offset_applied = item
        .get_property::<bool>("earthsearch:boa_offset_applied")
        .unwrap_or(false);
    let nodata_pct = item
        .get_property::<f64>("s2:nodata_pixel_percentage")
        .unwrap_or(0.0);

    Some(SceneCandidate {
        id: item.id.clone(),
        datetime,
        cloud,
        grid_code,
        epsg,
        red_href,
        nir_href,
        visual_href,
        boa_offset_applied,
        nodata_pct,
    })
}

/// `true` when the candidate passes the cloud / nodata gates.
///
/// A NaN cloud value never passes (`NaN <= x` is false).
fn is_usable(candidate: &SceneCandidate, max_cloud: f64) -> bool {
    candidate.cloud <= max_cloud && candidate.nodata_pct <= MAX_NODATA_PCT
}

/// Extract and filter candidates from raw STAC items.
fn usable_candidates(items: &[StacItem], max_cloud: f64) -> Vec<SceneCandidate> {
    items
        .iter()
        .filter_map(candidate_from_item)
        .filter(|candidate| is_usable(candidate, max_cloud))
        .collect()
}

/// Group candidates by `grid:code`, each group sorted cloud-ascending
/// (ties broken by item id for determinism).
fn group_by_grid(candidates: Vec<SceneCandidate>) -> BTreeMap<String, Vec<SceneCandidate>> {
    let mut groups: BTreeMap<String, Vec<SceneCandidate>> = BTreeMap::new();
    for candidate in candidates {
        groups
            .entry(candidate.grid_code.clone())
            .or_default()
            .push(candidate);
    }
    for group in groups.values_mut() {
        group.sort_by(|x, y| x.cloud.total_cmp(&y.cloud).then_with(|| x.id.cmp(&y.id)));
    }
    groups
}

/// Pair two search results into a primary scene pair plus alternates.
///
/// Filtering: scenes with `eo:cloud_cover > max_cloud` or
/// `s2:nodata_pixel_percentage > 10` are dropped, as are scenes missing
/// required metadata / assets (see [`candidate_from_item`]).
///
/// Pairing: scenes are grouped by `grid:code` and only grids present on
/// both dates are eligible. The winning grid minimizes the combined cloud
/// cover of its two best scenes (ties resolved toward the
/// lexicographically smallest grid code). The two best scenes form the
/// primary pair; the rest of the winning grid's scenes are returned as
/// cloud-ascending alternates.
///
/// # Errors
///
/// [`StacPairError::NoCandidates`] when one side has no usable scene,
/// [`StacPairError::NoCommonGrid`] when no grid is shared by both dates.
pub fn pair_candidates(
    items_a: &[StacItem],
    items_b: &[StacItem],
    max_cloud: f64,
) -> Result<PairResult, StacPairError> {
    let candidates_a = usable_candidates(items_a, max_cloud);
    if candidates_a.is_empty() {
        return Err(StacPairError::NoCandidates { side: 'A' });
    }
    let candidates_b = usable_candidates(items_b, max_cloud);
    if candidates_b.is_empty() {
        return Err(StacPairError::NoCandidates { side: 'B' });
    }

    let mut groups_a = group_by_grid(candidates_a);
    let mut groups_b = group_by_grid(candidates_b);

    // Select the common grid with the lowest combined best-scene cloud.
    // BTreeMap iteration is key-ascending and the strict `<` keeps the
    // first (lexicographically smallest) grid on ties — deterministic.
    let mut best: Option<(f64, String)> = None;
    for (grid, group_a) in &groups_a {
        let Some(group_b) = groups_b.get(grid) else {
            continue;
        };
        let (Some(best_a), Some(best_b)) = (group_a.first(), group_b.first()) else {
            continue;
        };
        let combined = best_a.cloud + best_b.cloud;
        let improves = match &best {
            None => true,
            Some((current, _)) => combined < *current,
        };
        if improves {
            best = Some((combined, grid.clone()));
        }
    }
    let Some((_, best_grid)) = best else {
        return Err(StacPairError::NoCommonGrid);
    };

    let group_a = groups_a.remove(&best_grid).unwrap_or_default();
    let group_b = groups_b.remove(&best_grid).unwrap_or_default();

    let mut iter_a = group_a.into_iter();
    let mut iter_b = group_b.into_iter();
    let (Some(primary_a), Some(primary_b)) = (iter_a.next(), iter_b.next()) else {
        return Err(StacPairError::NoCommonGrid);
    };

    Ok(PairResult {
        pair: ScenePair {
            a: primary_a,
            b: primary_b,
        },
        alternates_a: iter_a.collect(),
        alternates_b: iter_b.collect(),
    })
}

// ---------------------------------------------------------------------------
// Pure core: request building
// ---------------------------------------------------------------------------

/// Parse a bbox JSON string (`"[west, south, east, north]"`).
///
/// # Errors
///
/// [`StacPairError::InvalidBbox`] when the input is not a JSON array of
/// exactly four finite numbers.
pub fn parse_bbox(bbox_json: &str) -> Result<[f64; 4], StacPairError> {
    let bbox: [f64; 4] =
        serde_json::from_str(bbox_json).map_err(|err| StacPairError::InvalidBbox {
            message: err.to_string(),
        })?;
    if bbox.iter().any(|value| !value.is_finite()) {
        return Err(StacPairError::InvalidBbox {
            message: "coordinates must be finite".to_string(),
        });
    }
    Ok(bbox)
}

/// Parse the leading `YYYY-MM-DD` of a date string (a full RFC 3339
/// timestamp is accepted; only the calendar date is used).
///
/// # Errors
///
/// [`StacPairError::InvalidDate`] when no valid date prefix is present.
pub fn parse_day(input: &str) -> Result<NaiveDate, StacPairError> {
    let head = input.get(0..10).unwrap_or(input);
    NaiveDate::parse_from_str(head, "%Y-%m-%d").map_err(|_| StacPairError::InvalidDate {
        input: input.to_string(),
    })
}

/// Build the RFC 3339 datetime interval `date−w … date+w` (whole days,
/// inclusive: `T00:00:00Z` start, `T23:59:59Z` end).
///
/// # Errors
///
/// [`StacPairError::WindowOutOfRange`] when the interval leaves the
/// supported calendar range.
pub fn datetime_interval(center: NaiveDate, window_days: u32) -> Result<String, StacPairError> {
    let out_of_range = || StacPairError::WindowOutOfRange {
        date: center,
        window_days,
    };
    let delta = TimeDelta::try_days(i64::from(window_days)).ok_or_else(out_of_range)?;
    let start = center.checked_sub_signed(delta).ok_or_else(out_of_range)?;
    let end = center.checked_add_signed(delta).ok_or_else(out_of_range)?;
    Ok(format!(
        "{}T00:00:00Z/{}T23:59:59Z",
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    ))
}

/// Build the per-date Sentinel-2 L2A search request
/// (bbox + `date ± window_days` interval + `limit=100`).
///
/// # Errors
///
/// [`StacPairError::WindowOutOfRange`] — see [`datetime_interval`].
pub fn build_search_request(
    bbox: [f64; 4],
    center: NaiveDate,
    window_days: u32,
) -> Result<SearchRequest, StacPairError> {
    Ok(SearchRequest::new()
        .with_bbox(bbox)
        .with_collections(vec![SENTINEL2_L2A_COLLECTION.to_string()])
        .with_datetime(datetime_interval(center, window_days)?)
        .with_limit(SEARCH_PAGE_LIMIT))
}

// ---------------------------------------------------------------------------
// WasmStacClient
// ---------------------------------------------------------------------------

/// STAC pairing client bound to one API root (default: Earth Search v1).
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct WasmStacClient {
    base_url: String,
}

#[wasm_bindgen]
impl WasmStacClient {
    /// Create a client. `base_url` defaults to Earth Search v1
    /// (`https://earth-search.aws.element84.com/v1`); a trailing slash
    /// is normalized away.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(base_url: Option<String>) -> WasmStacClient {
        let base = base_url
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_STAC_BASE_URL.to_string());
        WasmStacClient {
            base_url: base.trim_end_matches('/').to_string(),
        }
    }

    /// The STAC API root this client talks to.
    #[wasm_bindgen(getter, js_name = baseUrl)]
    #[must_use]
    pub fn base_url(&self) -> String {
        self.base_url.clone()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WasmStacClient {
    /// Search Sentinel-2 L2A scenes around two dates and pair them.
    ///
    /// Runs one `POST /search` per date (`date ± window_days`,
    /// `limit=100`), filters scenes client-side
    /// (`eo:cloud_cover ≤ max_cloud`, `s2:nodata_pixel_percentage ≤ 10`),
    /// and pairs same-`grid:code` scenes — see [`pair_candidates`].
    ///
    /// * `bbox_json` — `"[west, south, east, north]"` (WGS 84).
    /// * `date_a`, `date_b` — `YYYY-MM-DD` (RFC 3339 accepted).
    /// * `window_days` — half-width of each search interval in days.
    /// * `max_cloud` — maximum `eo:cloud_cover` percentage (0–100).
    ///
    /// Resolves to the [`PairResult`] JSON:
    /// `{"pair":{"a":…,"b":…},"alternatesA":[…],"alternatesB":[…]}`.
    ///
    /// # Errors
    ///
    /// Rejects with a message string on invalid arguments, network / HTTP
    /// failure, unparseable responses, or when no pair exists
    /// ([`StacPairError`] rendered via `Display`).
    #[wasm_bindgen(js_name = searchPair)]
    pub async fn search_pair(
        &self,
        bbox_json: String,
        date_a: String,
        date_b: String,
        window_days: u32,
        max_cloud: f64,
    ) -> Result<String, JsValue> {
        let bbox = parse_bbox(&bbox_json).map_err(to_js)?;
        let day_a = parse_day(&date_a).map_err(to_js)?;
        let day_b = parse_day(&date_b).map_err(to_js)?;
        let request_a = build_search_request(bbox, day_a, window_days).map_err(to_js)?;
        let request_b = build_search_request(bbox, day_b, window_days).map_err(to_js)?;

        let items_a = self.post_search(&request_a).await?;
        let items_b = self.post_search(&request_b).await?;

        let result =
            pair_candidates(&items_a.features, &items_b.features, max_cloud).map_err(to_js)?;
        serde_json::to_string(&result).map_err(|err| {
            to_js(StacPairError::Serialization {
                message: err.to_string(),
            })
        })
    }
}

#[cfg(target_arch = "wasm32")]
impl WasmStacClient {
    /// POST one search request to `{base}/search` and parse the response.
    async fn post_search(&self, request: &SearchRequest) -> Result<ItemCollection, JsValue> {
        let body = request.to_json().map_err(|err| {
            to_js(StacPairError::Serialization {
                message: err.to_string(),
            })
        })?;
        let url = format!("{}/search", self.base_url);

        let window = web_sys::window().ok_or_else(|| {
            to_js(StacPairError::Network {
                message: "no window object available".to_string(),
            })
        })?;

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_mode(RequestMode::Cors);

        let headers = Headers::new().map_err(|err| {
            to_js(StacPairError::Network {
                message: format!("failed to create headers: {err:?}"),
            })
        })?;
        headers
            .set("Content-Type", "application/json")
            .map_err(|err| {
                to_js(StacPairError::Network {
                    message: format!("failed to set Content-Type: {err:?}"),
                })
            })?;
        opts.set_headers(&headers);
        opts.set_body(&JsValue::from_str(&body));

        let fetch_request = Request::new_with_str_and_init(&url, &opts).map_err(|err| {
            to_js(StacPairError::Network {
                message: format!("failed to create request: {err:?}"),
            })
        })?;

        let response_value = JsFuture::from(window.fetch_with_request(&fetch_request))
            .await
            .map_err(|err| {
                to_js(StacPairError::Network {
                    message: format!("fetch failed: {err:?}"),
                })
            })?;
        let response: Response = response_value.dyn_into().map_err(|_| {
            to_js(StacPairError::Network {
                message: "fetch result is not a Response".to_string(),
            })
        })?;

        if !response.ok() {
            return Err(to_js(StacPairError::Network {
                message: format!("HTTP {} {}", response.status(), response.status_text()),
            }));
        }

        let text_promise = response.text().map_err(|err| {
            to_js(StacPairError::Network {
                message: format!("failed to read response body: {err:?}"),
            })
        })?;
        let text_value = JsFuture::from(text_promise).await.map_err(|err| {
            to_js(StacPairError::Network {
                message: format!("failed to read response body: {err:?}"),
            })
        })?;
        let text = text_value.as_string().ok_or_else(|| {
            to_js(StacPairError::Network {
                message: "response body is not a string".to_string(),
            })
        })?;

        StacApiClient::new(self.base_url.clone())
            .parse_item_collection(&text)
            .map_err(|err| {
                to_js(StacPairError::ResponseParse {
                    message: err.to_string(),
                })
            })
    }
}

/// Render a pairing error as a JS rejection value.
#[cfg(target_arch = "wasm32")]
fn to_js(err: StacPairError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_stac::client::StacApiClient;
    use serde_json::Value;

    /// Real Earth Search v1 response (captured 2026-07-13 via
    /// `POST https://earth-search.aws.element84.com/v1/search` with
    /// `collections=["sentinel-2-l2a"]`, bbox `[139.69,35.67,139.72,35.70]`,
    /// datetime `2024-05-01/2024-05-31`, limit 6), trimmed to the
    /// properties and assets this module consumes.
    const FIXTURE_TOKYO_MAY_2024: &str = r#"{
 "type": "FeatureCollection",
 "features": [
  {
   "type": "Feature",
   "stac_version": "1.0.0",
   "id": "S2A_54SUE_20240531_0_L2A",
   "geometry": null,
   "bbox": [138.78213014504425, 35.13501116137792, 140.0097096778842, 35.96351532157504],
   "properties": {
    "datetime": "2024-05-31T01:37:27.722000Z",
    "platform": "sentinel-2a",
    "eo:cloud_cover": 99.926066,
    "grid:code": "MGRS-54SUE",
    "s2:nodata_pixel_percentage": 29.023954,
    "proj:epsg": 32654,
    "earthsearch:boa_offset_applied": true
   },
   "assets": {
    "red": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240531_0_L2A/B04.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "Red (band 4) - 10m"
    },
    "nir": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240531_0_L2A/B08.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "NIR 1 (band 8) - 10m"
    },
    "visual": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240531_0_L2A/TCI.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "True color image"
    }
   },
   "collection": "sentinel-2-l2a"
  },
  {
   "type": "Feature",
   "stac_version": "1.0.0",
   "id": "S2A_54SUE_20240531_1_L2A",
   "geometry": null,
   "bbox": [138.77761489199077, 35.53874573339012, 140.0049642896795, 36.14069288925992],
   "properties": {
    "datetime": "2024-05-31T01:37:20.991000Z",
    "platform": "sentinel-2a",
    "eo:cloud_cover": 99.996132,
    "grid:code": "MGRS-54SUE",
    "s2:nodata_pixel_percentage": 52.772796,
    "proj:epsg": 32654,
    "earthsearch:boa_offset_applied": true
   },
   "assets": {
    "red": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240531_1_L2A/B04.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "Red (band 4) - 10m"
    },
    "nir": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240531_1_L2A/B08.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "NIR 1 (band 8) - 10m"
    },
    "visual": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240531_1_L2A/TCI.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "True color image"
    }
   },
   "collection": "sentinel-2-l2a"
  },
  {
   "type": "Feature",
   "stac_version": "1.0.0",
   "id": "S2B_54SUE_20240526_0_L2A",
   "geometry": null,
   "bbox": [138.77761489199077, 35.13501116137792, 140.0097096778842, 36.14069288925992],
   "properties": {
    "datetime": "2024-05-26T01:37:22.178000Z",
    "platform": "sentinel-2b",
    "eo:cloud_cover": 79.668349,
    "grid:code": "MGRS-54SUE",
    "s2:nodata_pixel_percentage": 0,
    "proj:epsg": 32654,
    "earthsearch:boa_offset_applied": true
   },
   "assets": {
    "red": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2B_54SUE_20240526_0_L2A/B04.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "Red (band 4) - 10m"
    },
    "nir": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2B_54SUE_20240526_0_L2A/B08.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "NIR 1 (band 8) - 10m"
    },
    "visual": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2B_54SUE_20240526_0_L2A/TCI.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "True color image"
    }
   },
   "collection": "sentinel-2-l2a"
  },
  {
   "type": "Feature",
   "stac_version": "1.0.0",
   "id": "S2A_54SUE_20240521_0_L2A",
   "geometry": null,
   "bbox": [138.77761489199077, 35.13501116137792, 140.0097096778842, 36.14069288925992],
   "properties": {
    "datetime": "2024-05-21T01:37:22.708000Z",
    "platform": "sentinel-2a",
    "eo:cloud_cover": 76.943237,
    "grid:code": "MGRS-54SUE",
    "s2:nodata_pixel_percentage": 0,
    "proj:epsg": 32654,
    "earthsearch:boa_offset_applied": true
   },
   "assets": {
    "red": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240521_0_L2A/B04.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "Red (band 4) - 10m"
    },
    "nir": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240521_0_L2A/B08.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "NIR 1 (band 8) - 10m"
    },
    "visual": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240521_0_L2A/TCI.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "True color image"
    }
   },
   "collection": "sentinel-2-l2a"
  },
  {
   "type": "Feature",
   "stac_version": "1.0.0",
   "id": "S2A_54SUE_20240511_0_L2A",
   "geometry": null,
   "bbox": [138.77761489199077, 35.13501116137792, 140.0097096778842, 36.14069288925992],
   "properties": {
    "datetime": "2024-05-11T01:37:25.440000Z",
    "platform": "sentinel-2a",
    "eo:cloud_cover": 22.445406,
    "grid:code": "MGRS-54SUE",
    "s2:nodata_pixel_percentage": 0,
    "proj:epsg": 32654,
    "earthsearch:boa_offset_applied": true
   },
   "assets": {
    "red": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240511_0_L2A/B04.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "Red (band 4) - 10m"
    },
    "nir": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240511_0_L2A/B08.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "NIR 1 (band 8) - 10m"
    },
    "visual": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2A_54SUE_20240511_0_L2A/TCI.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "True color image"
    }
   },
   "collection": "sentinel-2-l2a"
  },
  {
   "type": "Feature",
   "stac_version": "1.0.0",
   "id": "S2B_54SUE_20240506_0_L2A",
   "geometry": null,
   "bbox": [138.77761489199077, 35.13501116137792, 140.0097096778842, 36.14069288925992],
   "properties": {
    "datetime": "2024-05-06T01:37:23.018000Z",
    "platform": "sentinel-2b",
    "eo:cloud_cover": 99.997091,
    "grid:code": "MGRS-54SUE",
    "s2:nodata_pixel_percentage": 0,
    "proj:epsg": 32654,
    "earthsearch:boa_offset_applied": true
   },
   "assets": {
    "red": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2B_54SUE_20240506_0_L2A/B04.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "Red (band 4) - 10m"
    },
    "nir": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2B_54SUE_20240506_0_L2A/B08.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "NIR 1 (band 8) - 10m"
    },
    "visual": {
     "href": "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/S/UE/2024/5/S2B_54SUE_20240506_0_L2A/TCI.tif",
     "type": "image/tiff; application=geotiff; profile=cloud-optimized",
     "title": "True color image"
    }
   },
   "collection": "sentinel-2-l2a"
  }
 ]
}"#;

    fn fixture_items() -> Vec<StacItem> {
        let collection = StacApiClient::new(DEFAULT_STAC_BASE_URL)
            .parse_item_collection(FIXTURE_TOKYO_MAY_2024)
            .expect("fixture must parse as an ItemCollection");
        collection.features
    }

    /// Build a synthetic item with the given asset key set.
    fn make_item(
        id: &str,
        grid: &str,
        cloud: f64,
        nodata: f64,
        asset_keys: (&str, &str, Option<&str>),
    ) -> StacItem {
        let (red_key, nir_key, visual_key) = asset_keys;
        let mut assets = serde_json::Map::new();
        assets.insert(
            red_key.to_string(),
            serde_json::json!({"href": format!("https://example.invalid/{id}/red.tif")}),
        );
        assets.insert(
            nir_key.to_string(),
            serde_json::json!({"href": format!("https://example.invalid/{id}/nir.tif")}),
        );
        if let Some(key) = visual_key {
            assets.insert(
                key.to_string(),
                serde_json::json!({"href": format!("https://example.invalid/{id}/visual.tif")}),
            );
        }
        let value = serde_json::json!({
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": id,
            "geometry": null,
            "properties": {
                "datetime": "2024-05-11T01:37:25.440000Z",
                "eo:cloud_cover": cloud,
                "grid:code": grid,
                "s2:nodata_pixel_percentage": nodata,
                "proj:epsg": 32654,
                "earthsearch:boa_offset_applied": true
            },
            "assets": Value::Object(assets)
        });
        serde_json::from_value(value).expect("synthetic item must deserialize")
    }

    // -- fixture parsing ----------------------------------------------------

    #[test]
    fn fixture_parses_via_parse_item_collection() {
        let items = fixture_items();
        assert_eq!(items.len(), 6);
        let first = &items[0];
        assert_eq!(first.id, "S2A_54SUE_20240531_0_L2A");
        assert_eq!(first.cloud_cover(), Some(99.926066));
        assert_eq!(
            first.get_property::<String>("grid:code").as_deref(),
            Some("MGRS-54SUE")
        );
        assert_eq!(first.get_property::<u32>("proj:epsg"), Some(32654));
    }

    // -- candidate extraction ------------------------------------------------

    #[test]
    fn candidate_extraction_prefers_canonical_keys() {
        let items = fixture_items();
        let candidate = candidate_from_item(&items[4]).expect("candidate must extract");
        assert_eq!(candidate.id, "S2A_54SUE_20240511_0_L2A");
        assert_eq!(candidate.datetime, "2024-05-11T01:37:25.440000Z");
        assert!((candidate.cloud - 22.445406).abs() < 1e-9);
        assert_eq!(candidate.grid_code, "MGRS-54SUE");
        assert_eq!(candidate.epsg, 32654);
        assert!(
            candidate
                .red_href
                .ends_with("S2A_54SUE_20240511_0_L2A/B04.tif")
        );
        assert!(
            candidate
                .nir_href
                .ends_with("S2A_54SUE_20240511_0_L2A/B08.tif")
        );
        assert!(
            candidate
                .visual_href
                .as_deref()
                .expect("visual asset present")
                .ends_with("S2A_54SUE_20240511_0_L2A/TCI.tif")
        );
        assert!(candidate.boa_offset_applied);
        assert!(candidate.nodata_pct.abs() < 1e-12);
    }

    #[test]
    fn candidate_extraction_falls_back_to_band_keys() {
        let item = make_item(
            "BAND-KEYS",
            "MGRS-54SUE",
            5.0,
            0.0,
            ("B04", "B08", Some("TCI")),
        );
        let candidate = candidate_from_item(&item).expect("band-key candidate must extract");
        assert!(candidate.red_href.ends_with("/BAND-KEYS/red.tif"));
        assert!(candidate.nir_href.ends_with("/BAND-KEYS/nir.tif"));
        assert!(
            candidate
                .visual_href
                .as_deref()
                .expect("TCI asset present")
                .ends_with("/BAND-KEYS/visual.tif")
        );
    }

    #[test]
    fn candidate_missing_visual_is_allowed_but_missing_nir_rejected() {
        let no_visual = make_item("NO-VISUAL", "MGRS-54SUE", 5.0, 0.0, ("red", "nir", None));
        let candidate = candidate_from_item(&no_visual).expect("visual is optional");
        assert_eq!(candidate.visual_href, None);

        // "swir16" is a real asset key that is neither nir nor B08.
        let no_nir = make_item("NO-NIR", "MGRS-54SUE", 5.0, 0.0, ("red", "swir16", None));
        assert!(candidate_from_item(&no_nir).is_none());
    }

    #[test]
    fn candidate_missing_cloud_cover_rejected() {
        let mut item = make_item("NO-CLOUD", "MGRS-54SUE", 5.0, 0.0, ("red", "nir", None));
        if let Some(props) = item.properties.as_object_mut() {
            props.remove("eo:cloud_cover");
        }
        assert!(candidate_from_item(&item).is_none());
    }

    // -- filtering ------------------------------------------------------------

    #[test]
    fn pair_filters_cloud_and_nodata() {
        let items = fixture_items();
        // max_cloud=80 keeps 79.67 / 76.94 / 22.45; the >99 % scenes drop.
        let result = pair_candidates(&items, &items, 80.0).expect("pair must exist");
        assert_eq!(result.pair.a.id, "S2A_54SUE_20240511_0_L2A");
        assert_eq!(result.pair.b.id, "S2A_54SUE_20240511_0_L2A");
        let alt_ids: Vec<&str> = result
            .alternates_a
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect();
        // Cloud-ascending: 76.94 then 79.67.
        assert_eq!(
            alt_ids,
            vec!["S2A_54SUE_20240521_0_L2A", "S2B_54SUE_20240526_0_L2A"]
        );
    }

    #[test]
    fn nodata_gate_applies_even_when_cloud_allows_everything() {
        let items = fixture_items();
        // max_cloud=100 keeps every scene by cloud, but the two
        // 2024-05-31 scenes exceed the 10 % nodata gate.
        let result = pair_candidates(&items, &items, 100.0).expect("pair must exist");
        assert_eq!(result.pair.a.id, "S2A_54SUE_20240511_0_L2A");
        assert_eq!(result.alternates_a.len(), 3);
        assert!(
            result
                .alternates_a
                .iter()
                .all(|candidate| candidate.nodata_pct <= MAX_NODATA_PCT)
        );
    }

    #[test]
    fn pair_empty_after_filter_errors() {
        let items = fixture_items();
        let err = pair_candidates(&items, &items, 1.0).expect_err("all scenes filtered");
        assert!(matches!(err, StacPairError::NoCandidates { side: 'A' }));

        let clear = vec![make_item(
            "CLEAR",
            "MGRS-54SUE",
            0.5,
            0.0,
            ("red", "nir", None),
        )];
        let err_b = pair_candidates(&clear, &items, 1.0).expect_err("side B filtered");
        assert!(matches!(err_b, StacPairError::NoCandidates { side: 'B' }));
    }

    // -- grid pairing ----------------------------------------------------------

    #[test]
    fn pair_selects_lowest_combined_cloud_common_grid() {
        let items_a = vec![
            make_item("A-G1", "MGRS-53SNA", 1.0, 0.0, ("red", "nir", None)),
            make_item("A-G2-CLOUDY", "MGRS-54SUE", 40.0, 0.0, ("red", "nir", None)),
            make_item("A-G2-CLEAR", "MGRS-54SUE", 10.0, 0.0, ("red", "nir", None)),
            make_item("A-G3", "MGRS-54SVE", 5.0, 0.0, ("red", "nir", None)),
        ];
        let items_b = vec![
            make_item("B-G2", "MGRS-54SUE", 12.0, 0.0, ("red", "nir", None)),
            make_item("B-G3", "MGRS-54SVE", 30.0, 0.0, ("red", "nir", None)),
        ];
        // G1 is A-only. Combined best cloud: G2 = 10+12 = 22, G3 = 5+30 = 35.
        let result = pair_candidates(&items_a, &items_b, 100.0).expect("pair must exist");
        assert_eq!(result.pair.a.id, "A-G2-CLEAR");
        assert_eq!(result.pair.b.id, "B-G2");
        // Alternates come only from the winning grid.
        assert_eq!(result.alternates_a.len(), 1);
        assert_eq!(result.alternates_a[0].id, "A-G2-CLOUDY");
        assert!(result.alternates_b.is_empty());
    }

    #[test]
    fn pair_no_common_grid_errors() {
        let items_a = vec![make_item("A", "MGRS-54SUE", 5.0, 0.0, ("red", "nir", None))];
        let items_b = vec![make_item("B", "MGRS-53SNA", 5.0, 0.0, ("red", "nir", None))];
        let err = pair_candidates(&items_a, &items_b, 100.0).expect_err("grids disjoint");
        assert!(matches!(err, StacPairError::NoCommonGrid));
    }

    // -- JSON shapes -------------------------------------------------------------

    #[test]
    fn candidate_json_uses_contract_camel_case_keys() {
        let items = fixture_items();
        let candidate = candidate_from_item(&items[4]).expect("candidate must extract");
        let json = serde_json::to_value(&candidate).expect("candidate must serialize");
        let object = json.as_object().expect("candidate serializes to an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "boaOffsetApplied",
                "cloud",
                "datetime",
                "epsg",
                "gridCode",
                "id",
                "nirHref",
                "nodataPct",
                "redHref",
                "visualHref",
            ]
        );
    }

    #[test]
    fn pair_result_json_shape_matches_contract() {
        let items = fixture_items();
        let result = pair_candidates(&items, &items, 80.0).expect("pair must exist");
        let json = serde_json::to_value(&result).expect("result must serialize");
        assert_eq!(
            json["pair"]["a"]["id"],
            Value::String("S2A_54SUE_20240511_0_L2A".to_string())
        );
        assert_eq!(
            json["pair"]["b"]["gridCode"],
            Value::String("MGRS-54SUE".to_string())
        );
        assert!(json["alternatesA"].is_array());
        assert!(json["alternatesB"].is_array());
        assert_eq!(json["pair"]["a"]["epsg"], Value::from(32654u32));
    }

    // -- request building -----------------------------------------------------------

    #[test]
    fn search_request_to_json_shape() {
        let center = NaiveDate::from_ymd_opt(2024, 5, 11).expect("valid date");
        let request = build_search_request([139.69, 35.67, 139.72, 35.70], center, 10)
            .expect("request must build");
        let json = request.to_json().expect("request must serialize");
        let value: Value = serde_json::from_str(&json).expect("request JSON must parse");

        assert_eq!(value["collections"], serde_json::json!(["sentinel-2-l2a"]));
        assert_eq!(
            value["bbox"],
            serde_json::json!([139.69, 35.67, 139.72, 35.70])
        );
        assert_eq!(
            value["datetime"],
            Value::String("2024-05-01T00:00:00Z/2024-05-21T23:59:59Z".to_string())
        );
        assert_eq!(value["limit"], Value::from(SEARCH_PAGE_LIMIT));

        // None-valued fields must be omitted entirely (Earth Search rejects
        // unknown/null members of the legacy query extension).
        let object = value.as_object().expect("request serializes to an object");
        for absent in ["ids", "intersects", "query", "filter", "sort_by", "fields"] {
            assert!(!object.contains_key(absent), "unexpected key {absent}");
        }
    }

    #[test]
    fn datetime_interval_spans_window() {
        let center = NaiveDate::from_ymd_opt(2024, 1, 5).expect("valid date");
        assert_eq!(
            datetime_interval(center, 10).expect("interval must build"),
            "2023-12-26T00:00:00Z/2024-01-15T23:59:59Z"
        );
        assert_eq!(
            datetime_interval(center, 0).expect("interval must build"),
            "2024-01-05T00:00:00Z/2024-01-05T23:59:59Z"
        );
    }

    #[test]
    fn parse_day_accepts_date_and_rfc3339() {
        let expected = NaiveDate::from_ymd_opt(2024, 5, 11).expect("valid date");
        assert_eq!(parse_day("2024-05-11").expect("plain date"), expected);
        assert_eq!(
            parse_day("2024-05-11T01:37:25.440000Z").expect("rfc3339"),
            expected
        );
    }

    #[test]
    fn parse_day_rejects_garbage() {
        assert!(matches!(
            parse_day("yesterday"),
            Err(StacPairError::InvalidDate { .. })
        ));
        assert!(matches!(
            parse_day("2024-13-40"),
            Err(StacPairError::InvalidDate { .. })
        ));
    }

    #[test]
    fn parse_bbox_ok_and_rejects_bad_input() {
        assert_eq!(
            parse_bbox("[139.69, 35.67, 139.72, 35.70]").expect("valid bbox"),
            [139.69, 35.67, 139.72, 35.70]
        );
        assert!(matches!(
            parse_bbox("[1, 2, 3]"),
            Err(StacPairError::InvalidBbox { .. })
        ));
        assert!(matches!(
            parse_bbox("[1, 2, 3, \"four\"]"),
            Err(StacPairError::InvalidBbox { .. })
        ));
        assert!(matches!(
            parse_bbox("not json"),
            Err(StacPairError::InvalidBbox { .. })
        ));
    }

    // -- client construction -----------------------------------------------------------

    #[test]
    fn client_base_url_defaults_and_normalizes() {
        assert_eq!(WasmStacClient::new(None).base_url(), DEFAULT_STAC_BASE_URL);
        assert_eq!(
            WasmStacClient::new(Some(String::new())).base_url(),
            DEFAULT_STAC_BASE_URL
        );
        assert_eq!(
            WasmStacClient::new(Some("https://example.invalid/stac/".to_string())).base_url(),
            "https://example.invalid/stac"
        );
    }
}
