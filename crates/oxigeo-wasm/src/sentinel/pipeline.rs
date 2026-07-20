//! Pure change-detection core: NDVI difference, thresholding, polygons.
//!
//! [`detect_changes_core`] operates on already-fetched `u16` windows: BOA
//! offset handling, self-contained NDVI computation, drop thresholding (fixed
//! or self-contained Otsu), polygonization via `oxigeo-algorithms`, UTM →
//! WGS84 ring reprojection and Karney geodesic areas. Everything in this
//! module is natively testable — there are no wasm / web-sys imports here, so
//! the whole pipeline is exercised by the inline unit tests under
//! `cargo test -p oxigeo-wasm sentinel`.
//!
//! ## Data flow (mirrors the A4 design contract)
//!
//! 1. Alignment assert ([`assert_alignment`], design D8): the two scenes must
//!    share the same UTM zone, tiepoint origin and pixel scale (ε = 1e-6) so
//!    that the two `u16` windows are pixel-registered.
//! 2. AOI bbox → UTM → full-res pixel window → clamp → smallest pyramid level
//!    whose window fits `maxDim` (cap 2048) — [`plan_window`].
//! 3. Per pixel: BOA correction, NDVI for both dates, `drop = ndvi_a − ndvi_b`.
//! 4. Threshold: the fixed `drop ≥ t` default, or a self-contained 256-bin
//!    Otsu on the positive-drop half.
//! 5. Binary mask → [`oxigeo_algorithms`] `polygonize` (eight-connectivity,
//!    nodata `Some(0.0)`, north-up UTM `GeoTransform`, `min_area` in m²,
//!    pixel-edge boundaries, Douglas–Peucker simplify at one pixel).
//! 6. Each polygon's rings are reprojected UTM → WGS84 and its area is the
//!    Karney geodesic area (m² → hectares); the result is a GeoJSON
//!    `FeatureCollection`.
//! 7. A signed diff grid is cached for the diverging change overlay.
//!
//! Implemented by WP A4 (GeoSentinel lane); stub registered by WP W0.

use oxigeo_algorithms::raster::polygonize::{
    BoundaryMethod, Connectivity, PolygonizeOptions, polygonize,
};
use oxigeo_algorithms::{AreaMethod, area};
use oxigeo_core::types::GeoTransform;
use oxigeo_core::vector::{Coordinate, Geometry, LineString, Polygon};
use serde::Deserialize;
use serde_json::{Value, json};

use super::utm::{UtmZone, utm_to_wgs84, wgs84_to_utm};

/// Default AOI window budget (pixels along the longest side).
fn default_max_dim() -> u32 {
    1024
}

/// Default fixed NDVI-drop threshold.
fn default_threshold() -> f64 {
    0.15
}

/// Default minimum reported change-polygon area (hectares).
fn default_min_area() -> f64 {
    0.5
}

/// Hard cap on the window's longest side, regardless of `max_dim`.
const MAX_DIM_CAP: u32 = 2048;

/// Alignment tolerance for the two scenes' grid origin / scale (metres).
const ALIGN_EPS: f64 = 1e-6;

/// The Sentinel-2 BOA radiometric add-offset (processing baseline ≥ 04.00).
const BOA_OFFSET: f32 = 1000.0;

/// Magnitude below which the diff overlay is fully transparent.
const OVERLAY_DEADZONE: f32 = 0.05;

/// JS-supplied change-detection parameters (`detectChanges` payload).
///
/// Deserialized from `{"bbox":[w,s,e,n],"maxDim":1024,"threshold":0.15,
/// "useOtsu":false,"minAreaHa":0.5}`. All fields but `bbox` default.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    /// Area of interest as `[west, south, east, north]` in WGS84 degrees.
    pub bbox: [f64; 4],
    /// Longest-side pixel budget for the read window (capped at 2048).
    #[serde(default = "default_max_dim")]
    pub max_dim: u32,
    /// Fixed NDVI-drop threshold used when `use_otsu` is false.
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// If true, derive the threshold from a 256-bin Otsu on the drop half.
    #[serde(default)]
    pub use_otsu: bool,
    /// Minimum reported polygon area, in hectares.
    #[serde(default = "default_min_area")]
    pub min_area_ha: f64,
}

impl Params {
    /// Parse a `detectChanges` JSON payload.
    ///
    /// # Errors
    /// Returns the serde error message if the JSON is malformed or `bbox`
    /// is missing.
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| format!("invalid params JSON: {e}"))
    }
}

/// Geo-registration of one scene, sufficient to plan a read window.
///
/// Built from a COG's GeoTIFF tags (EPSG → UTM zone, tiepoint → grid origin,
/// pixel scale) plus the per-level downsample ratios (`levels[0]` is 1.0).
#[derive(Debug, Clone)]
pub struct SceneGeo {
    /// UTM zone of the scene's projected CRS.
    pub zone: UtmZone,
    /// Easting of the upper-left corner of pixel (0, 0), metres.
    pub origin_e: f64,
    /// Northing of the upper-left corner of pixel (0, 0), metres.
    pub origin_n: f64,
    /// Full-resolution pixel size, metres (nominally 10 for S-2 bands).
    pub pixel_scale: f64,
    /// Full-resolution image width, pixels.
    pub full_width: u64,
    /// Full-resolution image height, pixels.
    pub full_height: u64,
    /// Downsample ratio per pyramid level; `level_ratios[0] == 1.0`.
    pub level_ratios: Vec<f64>,
}

/// A resolved read window: which pyramid level and which pixel rectangle to
/// fetch, plus the UTM georeferencing of that rectangle.
#[derive(Debug, Clone)]
pub struct WindowPlan {
    /// UTM zone (carried through for ring reprojection).
    pub zone: UtmZone,
    /// Chosen pyramid level (0 = full resolution).
    pub level: usize,
    /// Window origin column at `level`, pixels.
    pub x0: u64,
    /// Window origin row at `level`, pixels.
    pub y0: u64,
    /// Window width at `level`, pixels.
    pub w: u32,
    /// Window height at `level`, pixels.
    pub h: u32,
    /// Pixel size at `level`, metres (`pixel_scale × level_ratio`).
    pub px_m: f64,
    /// Easting of the window's upper-left corner, metres.
    pub origin_e: f64,
    /// Northing of the window's upper-left corner, metres.
    pub origin_n: f64,
    /// WGS84 bounds of the window as `[west, south, east, north]` degrees.
    pub bounds_wgs84: [f64; 4],
}

/// Output of [`detect_changes_core`].
#[derive(Debug, Clone)]
pub struct ChangeOutput {
    /// Total changed area across all reported polygons, hectares.
    pub total_ha: f64,
    /// Number of reported change polygons.
    pub polygon_count: usize,
    /// Threshold actually applied (fixed value or the Otsu-derived one).
    pub threshold_used: f64,
    /// GeoJSON `FeatureCollection` (WGS84) of the change polygons.
    pub fc: Value,
    /// Signed NDVI-drop grid (`w × h`, row-major) for the overlay.
    pub diff: Vec<f32>,
    /// Grid width, pixels.
    pub width: u32,
    /// Grid height, pixels.
    pub height: u32,
}

/// Assert the two scenes share a pixel-registered UTM grid (design D8).
///
/// # Errors
/// Returns a human-readable message if the zones differ, or if the grid
/// origin / pixel scale differ by more than `ALIGN_EPS` metres.
pub fn assert_alignment(a: &SceneGeo, b: &SceneGeo) -> Result<(), String> {
    if a.zone != b.zone {
        return Err(format!(
            "scene CRS mismatch: EPSG {} vs {} — GeoSentinel v1 pairs scenes \
             sharing an MGRS tile / UTM grid",
            a.zone.epsg(),
            b.zone.epsg()
        ));
    }
    if (a.origin_e - b.origin_e).abs() > ALIGN_EPS || (a.origin_n - b.origin_n).abs() > ALIGN_EPS {
        return Err(format!(
            "scene grid-origin mismatch: ({}, {}) vs ({}, {}) metres",
            a.origin_e, a.origin_n, b.origin_e, b.origin_n
        ));
    }
    if (a.pixel_scale - b.pixel_scale).abs() > ALIGN_EPS {
        return Err(format!(
            "scene pixel-scale mismatch: {} vs {} metres",
            a.pixel_scale, b.pixel_scale
        ));
    }
    Ok(())
}

/// Pick the finest pyramid level whose window fits `cap` pixels on both axes.
///
/// Levels are searched from finest (index 0) to coarsest; the first level whose
/// scaled window fits is returned. If none fit, the coarsest available level is
/// used (best effort). `ratios[i]` is the level's downsample factor.
fn select_level(wf: f64, hf: f64, cap: u32, ratios: &[f64]) -> usize {
    let cap = f64::from(cap.max(1));
    let coarsest = ratios.len().saturating_sub(1);
    for (i, &ratio) in ratios.iter().enumerate() {
        if ratio <= 0.0 {
            continue;
        }
        if (wf / ratio).max(hf / ratio) <= cap {
            return i;
        }
    }
    coarsest
}

/// Reproject an axis-aligned UTM rectangle's four corners and take the WGS84
/// bounding box `[west, south, east, north]`.
fn utm_rect_to_wgs84_bounds(
    zone: &UtmZone,
    e_min: f64,
    e_max: f64,
    n_min: f64,
    n_max: f64,
) -> [f64; 4] {
    let mut west = f64::INFINITY;
    let mut south = f64::INFINITY;
    let mut east = f64::NEG_INFINITY;
    let mut north = f64::NEG_INFINITY;
    for &(e, n) in &[
        (e_min, n_min),
        (e_min, n_max),
        (e_max, n_min),
        (e_max, n_max),
    ] {
        let (lon, lat) = utm_to_wgs84(zone, e, n);
        west = west.min(lon);
        south = south.min(lat);
        east = east.max(lon);
        north = north.max(lat);
    }
    [west, south, east, north]
}

/// Plan the read window for an AOI against one scene's geo-registration.
///
/// The AOI bbox corners are projected to UTM, the covering full-resolution
/// pixel rectangle is clamped to the image, the coarsest-but-fitting pyramid
/// level is chosen (`select_level`) and the window is expressed in that
/// level's pixel coordinates together with its UTM origin and WGS84 bounds.
///
/// # Errors
/// Returns an error if the AOI does not intersect the scene.
pub fn plan_window(geo: &SceneGeo, bbox: [f64; 4], max_dim: u32) -> Result<WindowPlan, String> {
    let [w_lon, s_lat, e_lon, n_lat] = bbox;

    // AOI corners → UTM extent (the AOI is not axis-aligned in UTM, so all four
    // corners matter).
    let mut min_e = f64::INFINITY;
    let mut max_e = f64::NEG_INFINITY;
    let mut min_n = f64::INFINITY;
    let mut max_n = f64::NEG_INFINITY;
    for &(lon, lat) in &[
        (w_lon, s_lat),
        (w_lon, n_lat),
        (e_lon, s_lat),
        (e_lon, n_lat),
    ] {
        let (e, n) = wgs84_to_utm(&geo.zone, lon, lat);
        min_e = min_e.min(e);
        max_e = max_e.max(e);
        min_n = min_n.min(n);
        max_n = max_n.max(n);
    }

    let scale = geo.pixel_scale;
    if scale <= 0.0 {
        return Err("scene has a non-positive pixel scale".to_string());
    }

    // Full-res pixel rectangle. Rows increase southward (northing decreases).
    let fw = geo.full_width as f64;
    let fh = geo.full_height as f64;
    let x0f = ((min_e - geo.origin_e) / scale).floor().clamp(0.0, fw);
    let x1f = ((max_e - geo.origin_e) / scale).ceil().clamp(0.0, fw);
    let y0f = ((geo.origin_n - max_n) / scale).floor().clamp(0.0, fh);
    let y1f = ((geo.origin_n - min_n) / scale).ceil().clamp(0.0, fh);
    let wf = x1f - x0f;
    let hf = y1f - y0f;
    if wf < 1.0 || hf < 1.0 {
        return Err("AOI does not intersect the scene".to_string());
    }

    let cap = max_dim.clamp(1, MAX_DIM_CAP);
    let level = select_level(wf, hf, cap, &geo.level_ratios);
    let ratio = geo.level_ratios.get(level).copied().unwrap_or(1.0);
    let ratio = if ratio > 0.0 { ratio } else { 1.0 };
    let px_m = scale * ratio;

    let x0 = (x0f / ratio).floor() as u64;
    let y0 = (y0f / ratio).floor() as u64;
    let x1 = (x1f / ratio).ceil() as u64;
    let y1 = (y1f / ratio).ceil() as u64;
    let w = (x1.saturating_sub(x0).max(1)) as u32;
    let h = (y1.saturating_sub(y0).max(1)) as u32;

    let origin_e = geo.origin_e + (x0 as f64) * px_m;
    let origin_n = geo.origin_n - (y0 as f64) * px_m;
    let e_max = origin_e + f64::from(w) * px_m;
    let n_min = origin_n - f64::from(h) * px_m;
    let bounds_wgs84 = utm_rect_to_wgs84_bounds(&geo.zone, origin_e, e_max, n_min, origin_n);

    Ok(WindowPlan {
        zone: geo.zone,
        level,
        x0,
        y0,
        w,
        h,
        px_m,
        origin_e,
        origin_n,
        bounds_wgs84,
    })
}

/// Undo the Sentinel-2 BOA radiometric offset for one DN.
///
/// When the scene already had the offset applied (`boa_applied == true`) the DN
/// is a corrected reflectance-proportional value and is used directly.
/// Otherwise the +1000 offset is removed and the result is clamped at zero, as
/// specified by the A4 contract (`v = max(dn − 1000, 0)`).
fn boa_correct(dn: u16, boa_applied: bool) -> f32 {
    let v = f32::from(dn);
    if boa_applied {
        v
    } else {
        (v - BOA_OFFSET).max(0.0)
    }
}

/// NDVI from BOA-corrected red / NIR values.
///
/// Returns `None` for invalid pixels — either band exactly zero (no-data /
/// clamped) or a zero denominator — so they are excluded from thresholding,
/// masking and the overlay.
fn ndvi(red: f32, nir: f32) -> Option<f32> {
    if red == 0.0 || nir == 0.0 {
        return None;
    }
    let denom = nir + red;
    if denom == 0.0 {
        return None;
    }
    Some((nir - red) / denom)
}

/// Self-contained 256-bin Otsu threshold over the histogram, returning the
/// bin index `0..=255` that maximizes between-class variance.
fn otsu_256(hist: &[u32; 256]) -> u8 {
    let total: u64 = hist.iter().map(|&c| u64::from(c)).sum();
    if total == 0 {
        return 0;
    }
    let total_f = total as f64;
    let sum_all: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * f64::from(c))
        .sum();

    let mut weight_bg = 0.0f64;
    let mut sum_bg = 0.0f64;
    let mut max_var = -1.0f64;
    let mut threshold = 0u8;

    for (t, &count) in hist.iter().enumerate() {
        weight_bg += f64::from(count);
        if weight_bg == 0.0 {
            continue;
        }
        let weight_fg = total_f - weight_bg;
        if weight_fg <= 0.0 {
            break;
        }
        sum_bg += t as f64 * f64::from(count);
        let mean_bg = sum_bg / weight_bg;
        let mean_fg = (sum_all - sum_bg) / weight_fg;
        let between = weight_bg * weight_fg * (mean_bg - mean_fg) * (mean_bg - mean_fg);
        if between > max_var {
            max_var = between;
            threshold = t as u8;
        }
    }
    threshold
}

/// Derive an NDVI-drop threshold from the positive-drop half via Otsu.
///
/// Positive drops are clamped to `[0, 1]`, quantized to `u8`, binned and passed
/// through [`otsu_256`]; the returned threshold is `otsu / 255`. Falls back to
/// the fixed default when there is no positive-drop signal.
fn otsu_threshold(drops: &[Option<f32>]) -> f64 {
    let mut hist = [0u32; 256];
    let mut any = false;
    for drop in drops.iter().flatten() {
        if *drop <= 0.0 {
            continue;
        }
        any = true;
        let bin = (drop.clamp(0.0, 1.0) * 255.0).round() as usize;
        hist[bin.min(255)] += 1;
    }
    if !any {
        return default_threshold();
    }
    f64::from(otsu_256(&hist)) / 255.0
}

/// Convert one UTM ring to `(lon, lat)` vertices.
fn ring_to_lonlat(ring: &LineString, zone: &UtmZone) -> Vec<(f64, f64)> {
    ring.coords
        .iter()
        .map(|c| utm_to_wgs84(zone, c.x, c.y))
        .collect()
}

/// Build a WGS84 [`Polygon`] and its GeoJSON ring arrays from a UTM polygon.
fn reproject_polygon(poly: &Polygon, zone: &UtmZone) -> Result<(Polygon, Vec<Value>), String> {
    let ext_ll = ring_to_lonlat(&poly.exterior, zone);
    let ext_coords: Vec<Coordinate> = ext_ll
        .iter()
        .map(|&(lon, lat)| Coordinate::new_2d(lon, lat))
        .collect();
    let ext_ls = LineString::new(ext_coords).map_err(|e| e.to_string())?;

    let mut holes = Vec::with_capacity(poly.interiors.len());
    let mut rings_json: Vec<Value> = Vec::with_capacity(poly.interiors.len() + 1);
    rings_json.push(Value::Array(
        ext_ll.iter().map(|&(lo, la)| json!([lo, la])).collect(),
    ));
    for interior in &poly.interiors {
        let ll = ring_to_lonlat(interior, zone);
        let coords: Vec<Coordinate> = ll
            .iter()
            .map(|&(lon, lat)| Coordinate::new_2d(lon, lat))
            .collect();
        holes.push(LineString::new(coords).map_err(|e| e.to_string())?);
        rings_json.push(Value::Array(
            ll.iter().map(|&(lo, la)| json!([lo, la])).collect(),
        ));
    }

    let wgs = Polygon::new(ext_ls, holes).map_err(|e| e.to_string())?;
    Ok((wgs, rings_json))
}

/// Pure change-detection core operating on already-fetched `u16` windows.
///
/// `red_a`/`nir_a`/`red_b`/`nir_b` are dense `w × h` row-major windows at the
/// planned pyramid level (both dates pixel-registered per [`assert_alignment`]).
/// `boa_a`/`boa_b` are each scene's `earthsearch:boa_offset_applied` flag.
///
/// # Errors
/// Returns an error if the four windows disagree with `w × h`, or if
/// polygonization / area computation fails.
#[allow(clippy::too_many_arguments)]
pub fn detect_changes_core(
    red_a: &[u16],
    nir_a: &[u16],
    red_b: &[u16],
    nir_b: &[u16],
    w: u32,
    h: u32,
    params: &Params,
    plan: &WindowPlan,
    boa_a: bool,
    boa_b: bool,
) -> Result<ChangeOutput, String> {
    let n = (w as usize) * (h as usize);
    if red_a.len() != n || nir_a.len() != n || red_b.len() != n || nir_b.len() != n {
        return Err(format!(
            "window buffer size mismatch: expected {n} samples ({w}×{h}), got \
             red_a={}, nir_a={}, red_b={}, nir_b={}",
            red_a.len(),
            nir_a.len(),
            red_b.len(),
            nir_b.len()
        ));
    }

    // Per-pixel NDVI drop (None where either date is invalid).
    let mut drops: Vec<Option<f32>> = Vec::with_capacity(n);
    for i in 0..n {
        let ra = boa_correct(red_a[i], boa_a);
        let na = boa_correct(nir_a[i], boa_a);
        let rb = boa_correct(red_b[i], boa_b);
        let nb = boa_correct(nir_b[i], boa_b);
        let drop = match (ndvi(ra, na), ndvi(rb, nb)) {
            (Some(va), Some(vb)) => Some(va - vb),
            _ => None,
        };
        drops.push(drop);
    }

    let threshold = if params.use_otsu {
        otsu_threshold(&drops)
    } else {
        params.threshold
    };

    // Binary mask (1.0 = changed) and signed diff grid for the overlay.
    let mut mask = vec![0.0f64; n];
    let mut diff = vec![0.0f32; n];
    for (i, drop) in drops.iter().enumerate() {
        if let Some(d) = drop {
            diff[i] = *d;
            if f64::from(*d) >= threshold {
                mask[i] = 1.0;
            }
        }
    }

    // Polygonize the mask in UTM metres (north-up transform at the window
    // origin), so `min_area` and simplify are metric.
    let transform = GeoTransform::north_up(plan.origin_e, plan.origin_n, plan.px_m, -plan.px_m);
    let opts = PolygonizeOptions {
        connectivity: Connectivity::Eight,
        nodata: Some(0.0),
        nodata_tolerance: 1e-10,
        transform: Some(transform),
        simplify_tolerance: plan.px_m,
        min_area: params.min_area_ha * 10_000.0,
        boundary_method: BoundaryMethod::PixelEdge,
    };
    let result = polygonize(&mask, w as usize, h as usize, &opts).map_err(|e| e.to_string())?;

    let mut features: Vec<Value> = Vec::with_capacity(result.polygons.len());
    let mut total_ha = 0.0f64;
    for feat in &result.polygons {
        let (wgs_poly, rings_json) = reproject_polygon(&feat.polygon, &plan.zone)?;
        let m2 = area(&Geometry::Polygon(wgs_poly), AreaMethod::KarneyGeodesic)
            .map_err(|e| e.to_string())?;
        let ha = m2 / 10_000.0;
        total_ha += ha;
        features.push(json!({
            "type": "Feature",
            "properties": { "area_ha": ha, "area_m2": m2 },
            "geometry": { "type": "Polygon", "coordinates": rings_json },
        }));
    }

    let polygon_count = features.len();
    let fc = json!({ "type": "FeatureCollection", "features": features });

    Ok(ChangeOutput {
        total_ha,
        polygon_count,
        threshold_used: threshold,
        fc,
        diff,
        width: w,
        height: h,
    })
}

/// Render the cached signed diff grid to an RGBA buffer for a canvas overlay.
///
/// Diverging colormap: positive drop (vegetation *loss*) → red, negative drop
/// (vegetation *gain*) → green, with alpha ramping from the dead-zone edge to
/// full opacity at |drop| ≥ 1. Values with `|drop| < OVERLAY_DEADZONE` are
/// fully transparent. Output length is `diff.len() × 4`.
pub fn diff_to_rgba(diff: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(diff.len() * 4);
    for &d in diff {
        if d.abs() < OVERLAY_DEADZONE {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let magnitude = d.abs().min(1.0);
        let alpha = (60.0 + 195.0 * magnitude).round().clamp(0.0, 255.0) as u8;
        if d > 0.0 {
            out.extend_from_slice(&[220, 45, 45, alpha]);
        } else {
            out.extend_from_slice(&[45, 200, 70, alpha]);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn zone54() -> UtmZone {
        UtmZone::from_epsg(32654).expect("valid zone")
    }

    /// A near-central-meridian window plan so UTM ↔ WGS84 area distortion is
    /// minimal (the tests assert areas to ~1%).
    fn test_plan(w: u32, h: u32) -> WindowPlan {
        WindowPlan {
            zone: zone54(),
            level: 0,
            x0: 0,
            y0: 0,
            w,
            h,
            px_m: 10.0,
            origin_e: 500_000.0,
            origin_n: 3_900_000.0,
            bounds_wgs84: [0.0; 4],
        }
    }

    fn base_params() -> Params {
        Params {
            bbox: [0.0; 4],
            max_dim: 1024,
            threshold: 0.15,
            use_otsu: false,
            min_area_ha: 0.0,
        }
    }

    /// Build 8×8 red/NIR window pairs with two separated 2×2 vegetation-loss
    /// blobs. `offset` is added to every DN and `boa` toggles the correction so
    /// the same physical scene can be expressed with or without the +1000 BOA
    /// offset.
    fn two_blob_windows(offset: u16) -> (Vec<u16>, Vec<u16>, Vec<u16>, Vec<u16>) {
        let n = 8 * 8;
        // Background: red == nir → NDVI 0 on both dates → drop 0.
        let mut red_a = vec![1000 + offset; n];
        let mut nir_a = vec![1000 + offset; n];
        let mut red_b = vec![1000 + offset; n];
        let mut nir_b = vec![1000 + offset; n];
        // Date A vegetation (NDVI ≈ 0.8) inside the blobs; date B bare (0).
        for &(bx, by) in &[(1u32, 1u32), (5, 5)] {
            for dy in 0..2u32 {
                for dx in 0..2u32 {
                    let idx = ((by + dy) * 8 + (bx + dx)) as usize;
                    red_a[idx] = 1000 + offset;
                    nir_a[idx] = 9000 + offset;
                    red_b[idx] = 1000 + offset;
                    nir_b[idx] = 1000 + offset;
                }
            }
        }
        (red_a, nir_a, red_b, nir_b)
    }

    #[test]
    fn two_blob_integration_two_polygons_area_within_one_percent() {
        let (ra, na, rb, nb) = two_blob_windows(0);
        let plan = test_plan(8, 8);
        let params = base_params();
        let out = detect_changes_core(&ra, &na, &rb, &nb, 8, 8, &params, &plan, true, true)
            .expect("detect ok");

        assert_eq!(out.polygon_count, 2, "expected two separate change blobs");
        // 8 changed pixels × (10 m)² = 800 m² = 0.08 ha.
        let expected_ha = 8.0 * 100.0 / 10_000.0;
        let rel = (out.total_ha - expected_ha).abs() / expected_ha;
        assert!(
            rel < 0.01,
            "total {} ha vs expected {} ha (rel {:.4})",
            out.total_ha,
            expected_ha,
            rel
        );

        // FeatureCollection shape.
        assert_eq!(out.fc["type"], "FeatureCollection");
        assert_eq!(out.fc["features"].as_array().map(Vec::len), Some(2));
        let first_ring = &out.fc["features"][0]["geometry"]["coordinates"][0];
        assert!(first_ring.as_array().is_some_and(|r| r.len() >= 4));
        // Reprojected vertices are near the zone-54 central meridian (141°E).
        let lon = first_ring[0][0].as_f64().expect("lon");
        assert!((lon - 141.0).abs() < 0.5, "unexpected lon {lon}");
    }

    #[test]
    fn boa_path_matches_offset_scene() {
        // Same physical scene, once BOA-applied and once with the +1000 offset
        // still present — results must be identical.
        let plan = test_plan(8, 8);
        let params = base_params();

        let (ra0, na0, rb0, nb0) = two_blob_windows(0);
        let applied = detect_changes_core(&ra0, &na0, &rb0, &nb0, 8, 8, &params, &plan, true, true)
            .expect("applied ok");

        let (ra1, na1, rb1, nb1) = two_blob_windows(1000);
        let raw = detect_changes_core(&ra1, &na1, &rb1, &nb1, 8, 8, &params, &plan, false, false)
            .expect("raw ok");

        assert_eq!(applied.polygon_count, raw.polygon_count);
        assert!((applied.total_ha - raw.total_ha).abs() < 1e-9);
    }

    #[test]
    fn ndvi_5x5_hand_parity() {
        // Hand-computed NDVI / BOA parity on a handful of pixels.
        assert!((ndvi(1000.0, 9000.0).unwrap() - 0.8).abs() < 1e-6);
        assert!((ndvi(2000.0, 2000.0).unwrap() - 0.0).abs() < 1e-6);
        assert!(ndvi(0.0, 5000.0).is_none());
        assert!(ndvi(5000.0, 0.0).is_none());

        // BOA correction.
        assert!((boa_correct(2000, false) - 1000.0).abs() < 1e-6);
        assert!((boa_correct(2000, true) - 2000.0).abs() < 1e-6);
        assert!((boa_correct(500, false) - 0.0).abs() < 1e-6); // clamped

        // 5×5 drop grid: a single central changed pixel (NDVI 0.8 → 0.0).
        let n = 25;
        let mut ra = vec![1000u16; n];
        let mut na = vec![1000u16; n];
        let rb = vec![1000u16; n];
        let nb = vec![1000u16; n];
        let center = 2 * 5 + 2;
        ra[center] = 1000;
        na[center] = 9000; // date A NDVI 0.8 at centre, date B 0.0 ⇒ drop 0.8
        let plan = test_plan(5, 5);
        let params = base_params();
        let out =
            detect_changes_core(&ra, &na, &rb, &nb, 5, 5, &params, &plan, true, true).unwrap();
        assert_eq!(out.polygon_count, 1);
        // The centre diff is 0.8; all other cells are 0.
        assert!((out.diff[center] - 0.8).abs() < 1e-5);
        assert_eq!(out.diff.iter().filter(|&&d| d.abs() > 1e-6).count(), 1);
    }

    #[test]
    fn otsu_bimodal_separates_clusters() {
        // Two well-separated drop clusters (0.10 and 0.60); Otsu must land the
        // threshold strictly between them.
        let mut drops: Vec<Option<f32>> = Vec::new();
        for _ in 0..64 {
            drops.push(Some(0.10));
        }
        for _ in 0..64 {
            drops.push(Some(0.60));
        }
        let t = otsu_threshold(&drops);
        assert!(t > 0.10 && t < 0.60, "otsu threshold {t} not between modes");
    }

    #[test]
    fn otsu_empty_signal_falls_back() {
        let drops: Vec<Option<f32>> = vec![None, Some(-0.2), Some(0.0)];
        assert!((otsu_threshold(&drops) - default_threshold()).abs() < 1e-12);
    }

    #[test]
    fn overview_selection_from_synthetic_metadata() {
        let ratios = [1.0, 2.0, 4.0, 8.0];
        // 2000-px window at cap 1024 ⇒ level 0 (2000) fails, level 1 (1000) fits.
        assert_eq!(select_level(2000.0, 2000.0, 1024, &ratios), 1);
        // Small window ⇒ full resolution.
        assert_eq!(select_level(500.0, 500.0, 1024, &ratios), 0);
        // Nothing fits ⇒ coarsest level.
        assert_eq!(select_level(9000.0, 9000.0, 1024, &ratios), 3);
    }

    #[test]
    fn plan_window_selects_level_and_metrics() {
        // Zone-54 tile-like geo near the central meridian.
        let geo = SceneGeo {
            zone: zone54(),
            origin_e: 399_960.0,
            origin_n: 3_900_000.0,
            pixel_scale: 10.0,
            full_width: 10_980,
            full_height: 10_980,
            level_ratios: vec![1.0, 2.0, 4.0, 8.0],
        };
        let (lon0, lat0) = utm_to_wgs84(&geo.zone, 500_000.0, 3_890_000.0);
        // ~15 km AOI centred there ⇒ full-res longest side in (1024, 2048] ⇒
        // level 0 fails, level 1 (÷2) fits at cap 1024.
        let d = 0.07;
        let bbox = [lon0 - d, lat0 - d, lon0 + d, lat0 + d];
        let plan = plan_window(&geo, bbox, 1024).expect("plan ok");
        assert_eq!(plan.level, 1);
        assert!((plan.px_m - 20.0).abs() < 1e-9);
        assert!(u64::from(plan.w) <= 1024 && u64::from(plan.h) <= 1024);
        // Window stays inside the image.
        assert!(plan.x0 + u64::from(plan.w) <= geo.full_width / 2 + 1);
        // Bounds bracket the AOI centre.
        assert!(plan.bounds_wgs84[0] < lon0 && plan.bounds_wgs84[2] > lon0);
        assert!(plan.bounds_wgs84[1] < lat0 && plan.bounds_wgs84[3] > lat0);

        // A tiny AOI selects full resolution.
        let tiny = 0.005;
        let bbox_small = [lon0 - tiny, lat0 - tiny, lon0 + tiny, lat0 + tiny];
        let plan_small = plan_window(&geo, bbox_small, 1024).expect("plan ok");
        assert_eq!(plan_small.level, 0);
        assert!((plan_small.px_m - 10.0).abs() < 1e-9);
    }

    #[test]
    fn plan_window_rejects_disjoint_aoi() {
        let geo = SceneGeo {
            zone: zone54(),
            origin_e: 399_960.0,
            origin_n: 3_900_000.0,
            pixel_scale: 10.0,
            full_width: 10_980,
            full_height: 10_980,
            level_ratios: vec![1.0],
        };
        // Far western AOI that projects to negative easting for this tile.
        let bbox = [120.0, 35.0, 120.01, 35.01];
        assert!(plan_window(&geo, bbox, 1024).is_err());
    }

    #[test]
    fn assert_alignment_accepts_equal_and_rejects_drift() {
        let a = SceneGeo {
            zone: zone54(),
            origin_e: 399_960.0,
            origin_n: 3_900_000.0,
            pixel_scale: 10.0,
            full_width: 10_980,
            full_height: 10_980,
            level_ratios: vec![1.0],
        };
        let mut b = a.clone();
        assert!(assert_alignment(&a, &b).is_ok());
        b.origin_e += 1.0;
        assert!(assert_alignment(&a, &b).is_err());
        let mut c = a.clone();
        c.zone = UtmZone::from_epsg(32653).unwrap();
        assert!(assert_alignment(&a, &c).is_err());
    }

    #[test]
    fn diff_to_rgba_diverging_and_deadzone() {
        let diff = vec![0.8f32, -0.8, 0.01, 0.0];
        let rgba = diff_to_rgba(&diff);
        assert_eq!(rgba.len(), 4 * 4);
        // Loss → red, opaque-ish.
        assert_eq!(&rgba[0..3], &[220, 45, 45]);
        assert!(rgba[3] > 128);
        // Gain → green.
        assert_eq!(&rgba[4..7], &[45, 200, 70]);
        // Dead-zone → transparent.
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0]);
        assert_eq!(&rgba[12..16], &[0, 0, 0, 0]);
    }

    #[test]
    fn params_from_json_defaults() {
        let p = Params::from_json(r#"{"bbox":[1.0,2.0,3.0,4.0]}"#).expect("parse");
        assert_eq!(p.bbox, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(p.max_dim, 1024);
        assert!((p.threshold - 0.15).abs() < 1e-12);
        assert!(!p.use_otsu);
        assert!((p.min_area_ha - 0.5).abs() < 1e-12);

        let p2 = Params::from_json(
            r#"{"bbox":[0,0,0,0],"maxDim":512,"threshold":0.3,"useOtsu":true,"minAreaHa":2.0}"#,
        )
        .expect("parse");
        assert_eq!(p2.max_dim, 512);
        assert!(p2.use_otsu);
        assert!((p2.min_area_ha - 2.0).abs() < 1e-12);

        assert!(Params::from_json("not json").is_err());
    }
}
