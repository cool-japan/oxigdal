//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use wasm_bindgen::prelude::*;

use super::types_4::AdvancedCogViewer;

/// GeoJSON export utilities
#[wasm_bindgen]
pub struct GeoJsonExporter;
#[wasm_bindgen]
impl GeoJsonExporter {
    /// Exports image bounds as GeoJSON
    #[wasm_bindgen(js_name = exportBounds)]
    pub fn export_bounds(
        west: f64,
        south: f64,
        east: f64,
        north: f64,
        epsg: Option<u32>,
    ) -> String {
        serde_json::json!(
            { "type" : "Feature", "geometry" : { "type" : "Polygon", "coordinates" :
            [[[west, south], [east, south], [east, north], [west, north], [west, south]]]
            }, "properties" : { "epsg" : epsg } }
        )
        .to_string()
    }
    /// Exports a point as GeoJSON
    #[wasm_bindgen(js_name = exportPoint)]
    pub fn export_point(x: f64, y: f64, properties: &str) -> String {
        let props: serde_json::Value =
            serde_json::from_str(properties).unwrap_or(serde_json::json!({}));
        serde_json::json!(
            { "type" : "Feature", "geometry" : { "type" : "Point", "coordinates" : [x, y]
            }, "properties" : props }
        )
        .to_string()
    }
}
/// Batch tile loader for efficient multi-tile loading
#[wasm_bindgen]
pub struct BatchTileLoader {
    pub(super) viewer: AdvancedCogViewer,
    pub(super) max_parallel: usize,
}
#[wasm_bindgen]
impl BatchTileLoader {
    /// Creates a new batch tile loader
    #[wasm_bindgen(constructor)]
    pub fn new(max_parallel: usize) -> Self {
        Self {
            viewer: AdvancedCogViewer::new(),
            max_parallel,
        }
    }
    /// Opens a COG
    #[wasm_bindgen]
    pub async fn open(&mut self, url: &str, cache_size_mb: usize) -> Result<(), JsValue> {
        self.viewer.open(url, cache_size_mb).await
    }
    /// Loads multiple tiles in parallel
    #[wasm_bindgen(js_name = loadTilesBatch)]
    pub async fn load_tiles_batch(
        &mut self,
        level: usize,
        tile_coords: Vec<u32>,
    ) -> Result<Vec<JsValue>, JsValue> {
        let mut results = Vec::new();
        for chunk in tile_coords.chunks_exact(2).take(self.max_parallel) {
            let tile_x = chunk[0];
            let tile_y = chunk[1];
            match self
                .viewer
                .read_tile_as_image_data(level, tile_x, tile_y)
                .await
            {
                Ok(image_data) => results.push(image_data.into()),
                Err(e) => results.push(e),
            }
        }
        Ok(results)
    }
}
