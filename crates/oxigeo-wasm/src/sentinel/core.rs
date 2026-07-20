//! `GeoSentinel` scene management and JS-facing change-detection bindings.
//!
//! Holds two scene slots, each with persistent `WasmCogReader`s (red / NIR /
//! optional true-colour), drives the pure pipeline in [`super::pipeline`], and
//! exposes overlay / true-colour RGBA buffers to the browser. Keeping the COG
//! readers alive across calls (rather than re-opening per read) is the whole
//! point of this type — a session opens each band once, then `detectChanges`
//! only issues HTTP range reads for the AOI window.
//!
//! The heavy lifting (NDVI, thresholding, polygonization, reprojection, area)
//! lives in [`super::pipeline`] and is natively unit-tested there; this module
//! is the thin async/wasm shell that reads pixels and marshals JSON / RGBA
//! across the JS boundary.
//!
//! Implemented by WP A4 (GeoSentinel lane); stub registered by WP W0.

use wasm_bindgen::prelude::*;

use crate::cog_reader::{CogMetadata, WasmCogReader};
use crate::to_js_error;

use super::pipeline::{
    ChangeOutput, Params, SceneGeo, WindowPlan, assert_alignment, detect_changes_core,
    diff_to_rgba, plan_window,
};
use super::utm::UtmZone;

/// A loaded scene: persistent band readers plus its geo-registration.
struct Scene {
    red: WasmCogReader,
    nir: WasmCogReader,
    visual: Option<WasmCogReader>,
    /// `earthsearch:boa_offset_applied` for this item.
    boa: bool,
    geo: SceneGeo,
}

/// Cached result of the last [`GeoSentinel::detect_changes`] call, retained so
/// the overlay / true-colour accessors can serve buffers without recomputing.
struct ComputeState {
    diff: Vec<f32>,
    width: u32,
    height: u32,
    bounds: [f64; 4],
    /// AOI bbox of the computation (reused to plan the true-colour window).
    bbox: [f64; 4],
    /// Longest-side pixel budget of the computation.
    max_dim: u32,
}

/// In-browser Sentinel-2 change detector.
///
/// ```text
/// const gs = new GeoSentinel();
/// await gs.loadScene(0, redUrlA, nirUrlA, tciUrlA, boaA);
/// await gs.loadScene(1, redUrlB, nirUrlB, tciUrlB, boaB);
/// const res = JSON.parse(await gs.detectChanges(paramsJson));
/// const rgba = gs.diffOverlayRgba();
/// ```
#[wasm_bindgen]
pub struct GeoSentinel {
    scenes: [Option<Scene>; 2],
    last: Option<ComputeState>,
}

impl Default for GeoSentinel {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive a [`SceneGeo`] from a freshly opened COG's GeoTIFF metadata.
fn scene_geo_from(meta: &CogMetadata) -> Result<SceneGeo, String> {
    let epsg = meta
        .epsg_code
        .ok_or_else(|| "COG is missing a projected EPSG code".to_string())?;
    let zone = UtmZone::from_epsg(epsg).ok_or_else(|| {
        format!("EPSG {epsg} is not a WGS84 / UTM zone (32601–32660 / 32701–32760)")
    })?;

    let geo_x = meta
        .tiepoint_geo_x
        .ok_or_else(|| "COG is missing a model tiepoint (X)".to_string())?;
    let geo_y = meta
        .tiepoint_geo_y
        .ok_or_else(|| "COG is missing a model tiepoint (Y)".to_string())?;
    let scale = meta
        .pixel_scale_x
        .ok_or_else(|| "COG is missing a model pixel scale".to_string())?;
    if scale <= 0.0 {
        return Err("COG pixel scale is non-positive".to_string());
    }

    // Account for a non-zero tiepoint raster position (usually 0,0 for S-2).
    let tie_px = meta.tiepoint_pixel_x.unwrap_or(0.0);
    let tie_py = meta.tiepoint_pixel_y.unwrap_or(0.0);
    let origin_e = geo_x - tie_px * scale;
    let origin_n = geo_y + tie_py * scale;

    let level_ratios: Vec<f64> = if meta.levels.is_empty() {
        vec![1.0]
    } else {
        meta.levels
            .iter()
            .map(|lvl| {
                if lvl.width == 0 {
                    1.0
                } else {
                    meta.width as f64 / lvl.width as f64
                }
            })
            .collect()
    };

    Ok(SceneGeo {
        zone,
        origin_e,
        origin_n,
        pixel_scale: scale,
        full_width: meta.width,
        full_height: meta.height,
        level_ratios,
    })
}

#[wasm_bindgen]
impl GeoSentinel {
    /// Create an empty detector with both scene slots unloaded.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> GeoSentinel {
        GeoSentinel {
            scenes: [None, None],
            last: None,
        }
    }

    /// Open the red / NIR (and optional true-colour) COGs for slot `0` or `1`.
    ///
    /// Returns a JSON summary of the loaded scene:
    /// `{"slot","epsg","width","height","levels","boaOffsetApplied"}`.
    ///
    /// # Errors
    /// Fails if `slot > 1`, if any COG cannot be opened, or if the red COG
    /// lacks the GeoTIFF tags needed to georeference the scene.
    #[wasm_bindgen(js_name = loadScene)]
    pub async fn load_scene(
        &mut self,
        slot: u8,
        red_url: String,
        nir_url: String,
        visual_url: Option<String>,
        boa_offset_applied: bool,
    ) -> Result<String, JsValue> {
        if slot > 1 {
            return Err(JsValue::from_str("slot must be 0 or 1"));
        }

        let red = WasmCogReader::open(red_url)
            .await
            .map_err(|e| to_js_error(&e))?;
        let nir = WasmCogReader::open(nir_url)
            .await
            .map_err(|e| to_js_error(&e))?;
        let visual = match visual_url {
            Some(url) => Some(
                WasmCogReader::open(url)
                    .await
                    .map_err(|e| to_js_error(&e))?,
            ),
            None => None,
        };

        let geo = scene_geo_from(red.metadata()).map_err(|e| JsValue::from_str(&e))?;

        let summary = serde_json::json!({
            "slot": slot,
            "epsg": geo.zone.epsg(),
            "width": geo.full_width,
            "height": geo.full_height,
            "levels": geo.level_ratios.len(),
            "boaOffsetApplied": boa_offset_applied,
        })
        .to_string();

        self.scenes[slot as usize] = Some(Scene {
            red,
            nir,
            visual,
            boa: boa_offset_applied,
            geo,
        });

        Ok(summary)
    }

    /// Run change detection over both loaded scenes for the given AOI.
    ///
    /// `params_json` is a [`Params`] payload:
    /// `{"bbox":[w,s,e,n],"maxDim":1024,"threshold":0.15,"useOtsu":false,
    /// "minAreaHa":0.5}`.
    ///
    /// Returns:
    /// `{"totalHa","polygonCount","thresholdUsed","level","windowPx":[x0,y0,w,h],
    /// "boundsWgs84":[w,s,e,n],"fc":<GeoJSON FeatureCollection>}`.
    ///
    /// # Errors
    /// Fails if a scene is not loaded, the scenes are not on the same UTM grid
    /// (design D8), the AOI misses the scene, or a tile read / polygonization
    /// fails.
    #[wasm_bindgen(js_name = detectChanges)]
    pub async fn detect_changes(&mut self, params_json: String) -> Result<String, JsValue> {
        let params = Params::from_json(&params_json).map_err(|e| JsValue::from_str(&e))?;

        // Copy the per-scene BOA flags up front so the scene borrows can drop
        // before the final mutable `self.last` write.
        let boa_a = self.scene(0)?.boa;
        let boa_b = self.scene(1)?.boa;

        let plan: WindowPlan = {
            let a = self.scene(0)?;
            let b = self.scene(1)?;
            assert_alignment(&a.geo, &b.geo).map_err(|e| JsValue::from_str(&e))?;
            plan_window(&a.geo, params.bbox, params.max_dim).map_err(|e| JsValue::from_str(&e))?
        };
        let (level, x0, y0, w, h) = (plan.level, plan.x0, plan.y0, plan.w, plan.h);

        // Fetch the four AOI windows (each read re-borrows a scene slot and
        // releases it at the end of the statement).
        let red_a = self
            .scene(0)?
            .red
            .read_window_u16(level, x0, y0, w, h)
            .await
            .map_err(|e| to_js_error(&e))?;
        let nir_a = self
            .scene(0)?
            .nir
            .read_window_u16(level, x0, y0, w, h)
            .await
            .map_err(|e| to_js_error(&e))?;
        let red_b = self
            .scene(1)?
            .red
            .read_window_u16(level, x0, y0, w, h)
            .await
            .map_err(|e| to_js_error(&e))?;
        let nir_b = self
            .scene(1)?
            .nir
            .read_window_u16(level, x0, y0, w, h)
            .await
            .map_err(|e| to_js_error(&e))?;

        let out: ChangeOutput = detect_changes_core(
            &red_a, &nir_a, &red_b, &nir_b, w, h, &params, &plan, boa_a, boa_b,
        )
        .map_err(|e| JsValue::from_str(&e))?;

        let result = serde_json::json!({
            "totalHa": out.total_ha,
            "polygonCount": out.polygon_count,
            "thresholdUsed": out.threshold_used,
            "level": level,
            "windowPx": [x0, y0, w, h],
            "boundsWgs84": plan.bounds_wgs84,
            "fc": out.fc,
        })
        .to_string();

        self.last = Some(ComputeState {
            diff: out.diff,
            width: out.width,
            height: out.height,
            bounds: plan.bounds_wgs84,
            bbox: params.bbox,
            max_dim: params.max_dim,
        });

        Ok(result)
    }

    /// RGBA buffer of the cached change overlay (diverging red/green colormap;
    /// `|drop| < 0.05` transparent). Length is `width × height × 4`.
    ///
    /// # Errors
    /// Fails if no detection has been run yet.
    #[wasm_bindgen(js_name = diffOverlayRgba)]
    pub fn diff_overlay_rgba(&self) -> Result<Vec<u8>, JsValue> {
        let state = self
            .last
            .as_ref()
            .ok_or_else(|| JsValue::from_str("no detection has been run yet"))?;
        Ok(diff_to_rgba(&state.diff))
    }

    /// Geometry of the cached overlay: `{"width","height","boundsWgs84"}`.
    ///
    /// # Errors
    /// Fails if no detection has been run yet.
    #[wasm_bindgen(js_name = overlayInfo)]
    pub fn overlay_info(&self) -> Result<String, JsValue> {
        let state = self
            .last
            .as_ref()
            .ok_or_else(|| JsValue::from_str("no detection has been run yet"))?;
        Ok(serde_json::json!({
            "width": state.width,
            "height": state.height,
            "boundsWgs84": state.bounds,
        })
        .to_string())
    }

    /// RGBA true-colour buffer for the last AOI window of scene `slot`,
    /// together with its own pixel dimensions.
    ///
    /// Reads the scene's true-colour (TCI) COG over the same AOI as the last
    /// detection, planned against the TCI's own pyramid — which, since the
    /// TCI carries its own overview set, may differ in resolution from the
    /// reflectance-band overlay computed by `detectChanges`. Returning
    /// `width`/`height` alongside the buffer (rather than just the raw
    /// bytes) lets the caller size a canvas correctly without having to
    /// guess dimensions from the reflectance-band overlay's own `w`/`h`.
    ///
    /// # Errors
    /// Fails if no detection has been run, the slot is invalid or unloaded, the
    /// scene has no true-colour asset, or the read fails.
    #[wasm_bindgen(js_name = trueColorRgba)]
    pub async fn true_color_rgba(&mut self, slot: u8) -> Result<TrueColorImage, JsValue> {
        if slot > 1 {
            return Err(JsValue::from_str("slot must be 0 or 1"));
        }
        let (bbox, max_dim) = {
            let state = self
                .last
                .as_ref()
                .ok_or_else(|| JsValue::from_str("no detection has been run yet"))?;
            (state.bbox, state.max_dim)
        };

        // Plan against the TCI's own geo (same UTM grid, but its overview set
        // may differ from the reflectance bands').
        let plan = {
            let scene = self.scene(slot)?;
            let visual = scene
                .visual
                .as_ref()
                .ok_or_else(|| JsValue::from_str("scene has no true-colour asset"))?;
            let vgeo = scene_geo_from(visual.metadata()).map_err(|e| JsValue::from_str(&e))?;
            plan_window(&vgeo, bbox, max_dim).map_err(|e| JsValue::from_str(&e))?
        };

        let rgb = self
            .scene(slot)?
            .visual
            .as_ref()
            .ok_or_else(|| JsValue::from_str("scene has no true-colour asset"))?
            .read_window_rgb8(plan.level, plan.x0, plan.y0, plan.w, plan.h)
            .await
            .map_err(|e| to_js_error(&e))?;

        let mut data = Vec::with_capacity(rgb.len() / 3 * 4);
        for px in rgb.chunks_exact(3) {
            data.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
        Ok(TrueColorImage {
            data,
            width: plan.w,
            height: plan.h,
        })
    }
}

/// An RGBA true-colour raster plus the pixel dimensions it was decoded at.
///
/// `data.len() == width as usize * height as usize * 4` always holds for a
/// successfully constructed value.
#[wasm_bindgen]
pub struct TrueColorImage {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

#[wasm_bindgen]
impl TrueColorImage {
    /// Interleaved RGBA pixel bytes, row-major, `width * height * 4` long.
    #[wasm_bindgen(getter)]
    pub fn data(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// Raster width in pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Raster height in pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }
}

impl GeoSentinel {
    /// Borrow a loaded scene slot or produce a typed JS error.
    fn scene(&self, slot: u8) -> Result<&Scene, JsValue> {
        self.scenes
            .get(slot as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| JsValue::from_str(&format!("scene slot {slot} is not loaded")))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::cog_reader::IfdMetadata;

    fn ifd(width: u64, height: u64) -> IfdMetadata {
        IfdMetadata {
            width,
            height,
            tile_width: 512,
            tile_height: 512,
            bits_per_sample: 16,
            samples_per_pixel: 1,
            sample_format: 1,
            compression: 8,
            photometric_interpretation: 1,
            predictor: 1,
            tile_offsets: Vec::new(),
            tile_byte_counts: Vec::new(),
            pixel_scale_x: Some(10.0),
            pixel_scale_y: Some(10.0),
            tiepoint_pixel_x: Some(0.0),
            tiepoint_pixel_y: Some(0.0),
            tiepoint_geo_x: Some(399_960.0),
            tiepoint_geo_y: Some(3_900_000.0),
            epsg_code: Some(32654),
        }
    }

    fn meta_with_levels() -> CogMetadata {
        CogMetadata {
            width: 10_980,
            height: 10_980,
            tile_width: 512,
            tile_height: 512,
            bits_per_sample: 16,
            samples_per_pixel: 1,
            sample_format: 1,
            compression: 8,
            photometric_interpretation: 1,
            predictor: 1,
            tile_offsets: Vec::new(),
            tile_byte_counts: Vec::new(),
            pixel_scale_x: Some(10.0),
            pixel_scale_y: Some(10.0),
            tiepoint_pixel_x: Some(0.0),
            tiepoint_pixel_y: Some(0.0),
            tiepoint_geo_x: Some(399_960.0),
            tiepoint_geo_y: Some(3_900_000.0),
            overview_count: 3,
            overviews: Vec::new(),
            epsg_code: Some(32654),
            levels: vec![
                ifd(10_980, 10_980),
                ifd(5_490, 5_490),
                ifd(2_745, 2_745),
                ifd(1_373, 1_373),
            ],
        }
    }

    #[test]
    fn scene_geo_from_metadata() {
        let geo = scene_geo_from(&meta_with_levels()).expect("geo");
        assert_eq!(geo.zone, UtmZone::from_epsg(32654).unwrap());
        assert!((geo.origin_e - 399_960.0).abs() < 1e-9);
        assert!((geo.origin_n - 3_900_000.0).abs() < 1e-9);
        assert!((geo.pixel_scale - 10.0).abs() < 1e-9);
        assert_eq!(geo.full_width, 10_980);
        assert_eq!(geo.level_ratios.len(), 4);
        // Ratios approximately double each level.
        assert!((geo.level_ratios[0] - 1.0).abs() < 1e-9);
        assert!((geo.level_ratios[1] - 2.0).abs() < 0.01);
        assert!((geo.level_ratios[3] - 8.0).abs() < 0.05);
    }

    #[test]
    fn scene_geo_from_rejects_non_utm() {
        let mut meta = meta_with_levels();
        meta.epsg_code = Some(4326);
        assert!(scene_geo_from(&meta).is_err());
        meta.epsg_code = None;
        assert!(scene_geo_from(&meta).is_err());
    }

    #[test]
    fn scene_geo_honours_tiepoint_offset() {
        let mut meta = meta_with_levels();
        meta.tiepoint_pixel_x = Some(2.0);
        meta.tiepoint_pixel_y = Some(3.0);
        let geo = scene_geo_from(&meta).expect("geo");
        // origin = geo - tie_px*scale (X), geo + tie_py*scale (Y).
        assert!((geo.origin_e - (399_960.0 - 20.0)).abs() < 1e-9);
        assert!((geo.origin_n - (3_900_000.0 + 30.0)).abs() < 1e-9);
    }

    /// Regression test for the `trueColorRgba` dimension bug: the JS side
    /// used to have to *guess* width/height from the reflectance overlay's
    /// own dims (`fitDims` in pipeline.js), which is wrong whenever the TCI
    /// pyramid's chosen level differs from the reflectance bands'. The
    /// returned `TrueColorImage` must always self-describe its own buffer.
    #[test]
    fn true_color_image_getters_and_invariant() {
        let width = 4u32;
        let height = 3u32;
        let data = vec![7u8; (width * height * 4) as usize];
        let img = TrueColorImage {
            data: data.clone(),
            width,
            height,
        };
        assert_eq!(img.width(), width);
        assert_eq!(img.height(), height);
        assert_eq!(img.data(), data);
        assert_eq!(
            img.data().len() as u32,
            img.width() * img.height() * 4,
            "data length must always be width * height * 4"
        );
    }

    /// A TCI window that happens to be non-square (the common case once the
    /// pyramid levels of the TCI and the reflectance bands diverge) must
    /// still report exactly the dimensions the caller needs — this is the
    /// concrete shape the old `fitDims` factorization heuristic could get
    /// wrong (it searches for *a* factor pair near the reflectance overlay's
    /// dims, not necessarily *the* TCI window's actual w×h).
    #[test]
    fn true_color_image_non_square_invariant() {
        let width = 517u32;
        let height = 233u32;
        let data = vec![0u8; (width * height * 4) as usize];
        let img = TrueColorImage {
            data,
            width,
            height,
        };
        assert_eq!(img.data().len() as u32, width * height * 4);
        assert_eq!(img.width(), 517);
        assert_eq!(img.height(), 233);
    }
}
