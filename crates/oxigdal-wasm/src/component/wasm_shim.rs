//! `wasm-bindgen` shims exposing the pure-Rust [`ComponentProjection`] and
//! [`ComponentTransform`] descriptors to JavaScript.
//!
//! The underlying [`ComponentProjection`] type lives in
//! [`crate::component::projection`] and is Component-Model friendly but not
//! itself `#[wasm_bindgen]`. This module wraps it so the classic
//! (`wasm32-unknown-unknown`) build can hand CRS metadata and simple
//! WGS-84 ⇄ Web-Mercator transforms to the browser.

use wasm_bindgen::prelude::*;

use crate::component::projection::{ComponentCoord, ComponentProjection, ComponentTransform};

/// A browser-facing Coordinate Reference System descriptor.
///
/// This is a value object carrying CRS metadata (EPSG code, WKT, PROJ string,
/// geographic/projected flags). It performs no projection itself; use the free
/// functions [`wgs84_to_web_mercator`] / [`web_mercator_to_wgs84`] for the two
/// supported transforms.
#[wasm_bindgen]
pub struct WasmProjection {
    /// Wrapped pure-Rust descriptor.
    inner: ComponentProjection,
}

#[wasm_bindgen]
impl WasmProjection {
    /// Builds a descriptor from an EPSG authority code.
    #[wasm_bindgen(js_name = fromEpsg)]
    pub fn from_epsg(epsg: u32) -> Self {
        Self {
            inner: ComponentProjection::from_epsg(epsg),
        }
    }

    /// WGS 84 geographic CRS (EPSG:4326).
    #[wasm_bindgen]
    pub fn wgs84() -> Self {
        Self {
            inner: ComponentProjection::wgs84(),
        }
    }

    /// Web Mercator / Pseudo-Mercator (EPSG:3857).
    #[wasm_bindgen(js_name = webMercator)]
    pub fn web_mercator() -> Self {
        Self {
            inner: ComponentProjection::web_mercator(),
        }
    }

    /// Returns the EPSG authority code, if known.
    #[wasm_bindgen(js_name = epsgCode)]
    pub fn epsg_code(&self) -> Option<u32> {
        self.inner.epsg_code
    }

    /// Returns the CRS name (e.g. `"WGS 84"`).
    #[wasm_bindgen]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    /// Returns the units of measure (e.g. `"degree"`, `"metre"`).
    #[wasm_bindgen]
    pub fn units(&self) -> String {
        self.inner.units.clone()
    }

    /// Returns the Well-Known Text (WKT 2) representation.
    #[wasm_bindgen]
    pub fn wkt(&self) -> String {
        self.inner.wkt.clone()
    }

    /// Returns the PROJ string, if available.
    #[wasm_bindgen(js_name = projString)]
    pub fn proj_string(&self) -> Option<String> {
        self.inner.proj_string.clone()
    }

    /// Returns `true` for a geographic CRS (angular units).
    #[wasm_bindgen(js_name = isGeographic)]
    pub fn is_geographic(&self) -> bool {
        self.inner.is_geographic
    }

    /// Returns `true` for a projected CRS (linear units).
    #[wasm_bindgen(js_name = isProjected)]
    pub fn is_projected(&self) -> bool {
        self.inner.is_projected
    }

    /// Returns `true` if both descriptors refer to the same CRS.
    #[wasm_bindgen(js_name = sameCrs)]
    pub fn same_crs(&self, other: &WasmProjection) -> bool {
        self.inner.same_crs(&other.inner)
    }

    /// Serialises the descriptor to a JSON string.
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "epsgCode": self.inner.epsg_code,
            "name": self.inner.name,
            "units": self.inner.units,
            "wkt": self.inner.wkt,
            "projString": self.inner.proj_string,
            "isGeographic": self.inner.is_geographic,
            "isProjected": self.inner.is_projected,
        })
        .to_string()
    }
}

/// Converts an interleaved `[x, y, ...]` slice into coordinate pairs.
///
/// The error is a plain `String` (not `JsValue`) so this is safe to call from
/// native unit tests — constructing a `JsValue` aborts off-wasm.
fn interleaved_to_coords(coords: &[f64]) -> Result<Vec<ComponentCoord>, String> {
    if !coords.len().is_multiple_of(2) {
        return Err(
            "coordinate slice must contain an even number of values ([x, y, ...])".to_string(),
        );
    }
    Ok(coords
        .chunks_exact(2)
        .map(|c| ComponentCoord::new(c[0], c[1]))
        .collect())
}

/// Flattens coordinate pairs back into an interleaved `[x, y, ...]` vector.
fn coords_to_interleaved(coords: &[ComponentCoord]) -> Vec<f64> {
    let mut out = Vec::with_capacity(coords.len() * 2);
    for c in coords {
        out.push(c.x);
        out.push(c.y);
    }
    out
}

/// Projects interleaved WGS-84 `[lon, lat, ...]` to Web-Mercator `[x, y, ...]`.
#[wasm_bindgen(js_name = wgs84ToWebMercator)]
pub fn wgs84_to_web_mercator(coords: &[f64]) -> Result<Vec<f64>, JsValue> {
    let pts = interleaved_to_coords(coords).map_err(|e| JsValue::from_str(&e))?;
    let out = ComponentTransform::wgs84_to_web_mercator(&pts)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(coords_to_interleaved(&out))
}

/// Inverse of [`wgs84_to_web_mercator`]: Web-Mercator `[x, y, ...]` to WGS-84.
#[wasm_bindgen(js_name = webMercatorToWgs84)]
pub fn web_mercator_to_wgs84(coords: &[f64]) -> Result<Vec<f64>, JsValue> {
    let pts = interleaved_to_coords(coords).map_err(|e| JsValue::from_str(&e))?;
    let out = ComponentTransform::web_mercator_to_wgs84(&pts)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(coords_to_interleaved(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_projection_wgs84_metadata() {
        let p = WasmProjection::wgs84();
        assert_eq!(p.epsg_code(), Some(4326));
        assert!(p.is_geographic());
        assert!(!p.is_projected());
        assert_eq!(p.units(), "degree");
        assert!(p.name().contains("WGS 84"));
        assert!(p.wkt().contains("GEOGCRS"));
        assert!(p.proj_string().is_some());
    }

    #[test]
    fn wasm_projection_web_mercator_metadata() {
        let p = WasmProjection::web_mercator();
        assert_eq!(p.epsg_code(), Some(3857));
        assert!(p.is_projected());
        assert!(!p.is_geographic());
        assert_eq!(p.units(), "metre");
    }

    #[test]
    fn wasm_projection_from_epsg_utm() {
        let p = WasmProjection::from_epsg(32632);
        assert_eq!(p.epsg_code(), Some(32632));
        assert!(p.is_projected());
    }

    #[test]
    fn same_crs_matches() {
        let a = WasmProjection::wgs84();
        let b = WasmProjection::wgs84();
        let c = WasmProjection::web_mercator();
        assert!(a.same_crs(&b));
        assert!(!a.same_crs(&c));
    }

    #[test]
    fn to_json_contains_fields() {
        let json = WasmProjection::wgs84().to_json();
        assert!(json.contains("\"epsgCode\":4326"));
        assert!(json.contains("\"isGeographic\":true"));
    }

    #[test]
    fn interleaved_roundtrip() {
        let input = [13.405, 52.52, 0.0, 0.0];
        let merc = wgs84_to_web_mercator(&input).expect("forward");
        assert_eq!(merc.len(), 4);
        let back = web_mercator_to_wgs84(&merc).expect("inverse");
        assert!((back[0] - input[0]).abs() < 1e-7);
        assert!((back[1] - input[1]).abs() < 1e-7);
        assert!(back[2].abs() < 1e-7);
        assert!(back[3].abs() < 1e-7);
    }

    #[test]
    fn odd_length_slice_errors() {
        // Assert on the native-safe helper — calling the `JsValue`-returning
        // wrapper on the error path would abort off-wasm.
        assert!(interleaved_to_coords(&[1.0, 2.0, 3.0]).is_err());
        assert!(interleaved_to_coords(&[1.0, 2.0]).is_ok());
    }

    #[test]
    fn origin_projects_to_origin() {
        let out = wgs84_to_web_mercator(&[0.0, 0.0]).expect("forward");
        assert!(out[0].abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
    }
}
