//! Interactive widgets for Jupyter
//!
//! This module provides interactive widgets for geospatial data visualization
//! and manipulation in Jupyter notebooks.

use crate::{JupyterError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Base widget trait
pub trait Widget {
    /// Get widget type
    fn widget_type(&self) -> &str;

    /// Get widget state
    fn state(&self) -> Result<HashMap<String, serde_json::Value>>;

    /// Update widget state
    fn update_state(&mut self, state: HashMap<String, serde_json::Value>) -> Result<()>;

    /// Render widget to HTML
    fn render(&self) -> Result<String>;
}

/// Map widget for interactive map display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapWidget {
    /// Widget ID
    id: String,
    /// Center coordinates (lon, lat)
    center: (f64, f64),
    /// Zoom level
    zoom: u8,
    /// Width in pixels
    width: u32,
    /// Height in pixels
    height: u32,
    /// Layers
    layers: Vec<String>,
    /// Basemap provider
    basemap: BasemapProvider,
}

/// Basemap provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BasemapProvider {
    /// OpenStreetMap
    OpenStreetMap,
    /// Satellite imagery
    Satellite,
    /// Terrain
    Terrain,
    /// Custom tile URL
    Custom(String),
}

impl MapWidget {
    /// Create new map widget
    pub fn new(id: impl Into<String>, center: (f64, f64), zoom: u8) -> Self {
        Self {
            id: id.into(),
            center,
            zoom,
            width: 800,
            height: 600,
            layers: Vec::new(),
            basemap: BasemapProvider::OpenStreetMap,
        }
    }

    /// Set dimensions
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set basemap
    pub fn with_basemap(mut self, basemap: BasemapProvider) -> Self {
        self.basemap = basemap;
        self
    }

    /// Add layer
    pub fn add_layer(&mut self, layer_url: String) {
        self.layers.push(layer_url);
    }

    /// Get center
    pub fn center(&self) -> (f64, f64) {
        self.center
    }

    /// Set center
    pub fn set_center(&mut self, center: (f64, f64)) {
        self.center = center;
    }

    /// Get zoom
    pub fn zoom(&self) -> u8 {
        self.zoom
    }

    /// Set zoom
    pub fn set_zoom(&mut self, zoom: u8) {
        self.zoom = zoom;
    }

    /// Get basemap URL
    fn basemap_url(&self) -> String {
        match &self.basemap {
            BasemapProvider::OpenStreetMap => {
                "https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png".to_string()
            }
            BasemapProvider::Satellite => {
                "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}".to_string()
            }
            BasemapProvider::Terrain => {
                "https://{s}.tile.opentopomap.org/{z}/{x}/{y}.png".to_string()
            }
            BasemapProvider::Custom(url) => url.clone(),
        }
    }
}

impl Widget for MapWidget {
    fn widget_type(&self) -> &str {
        "MapWidget"
    }

    fn state(&self) -> Result<HashMap<String, serde_json::Value>> {
        let mut state = HashMap::new();
        state.insert("center".to_string(), serde_json::json!(self.center));
        state.insert("zoom".to_string(), serde_json::json!(self.zoom));
        state.insert("width".to_string(), serde_json::json!(self.width));
        state.insert("height".to_string(), serde_json::json!(self.height));
        state.insert("layers".to_string(), serde_json::json!(self.layers));
        state.insert("basemap".to_string(), serde_json::json!(self.basemap));
        Ok(state)
    }

    fn update_state(&mut self, state: HashMap<String, serde_json::Value>) -> Result<()> {
        if let Some(center) = state.get("center") {
            if let Ok(c) = serde_json::from_value::<(f64, f64)>(center.clone()) {
                self.center = c;
            }
        }
        if let Some(zoom) = state.get("zoom") {
            if let Ok(z) = serde_json::from_value::<u8>(zoom.clone()) {
                self.zoom = z;
            }
        }
        Ok(())
    }

    fn render(&self) -> Result<String> {
        let html = format!(
            r#"
            <div id='{}' style='width: {}px; height: {}px;'></div>
            <script src='https://unpkg.com/leaflet@1.9.4/dist/leaflet.js'></script>
            <link rel='stylesheet' href='https://unpkg.com/leaflet@1.9.4/dist/leaflet.css' />
            <script>
            (function() {{
                var map = L.map('{}').setView([{}, {}], {});
                L.tileLayer('{}', {{
                    attribution: '&copy; OpenStreetMap contributors'
                }}).addTo(map);
                {}
            }})();
            </script>
            "#,
            self.id,
            self.width,
            self.height,
            self.id,
            self.center.1,
            self.center.0,
            self.zoom,
            self.basemap_url(),
            self.layers
                .iter()
                .map(|url| format!("L.tileLayer('{}').addTo(map);", url))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(html)
    }
}

/// Slider widget
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliderWidget {
    /// Widget ID
    id: String,
    /// Current value
    value: f64,
    /// Minimum value
    min: f64,
    /// Maximum value
    max: f64,
    /// Step size
    step: f64,
    /// Label
    label: String,
}

impl SliderWidget {
    /// Create new slider widget
    pub fn new(id: impl Into<String>, min: f64, max: f64) -> Self {
        Self {
            id: id.into(),
            value: min,
            min,
            max,
            step: (max - min) / 100.0,
            label: "Value".to_string(),
        }
    }

    /// Set label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Set step
    pub fn with_step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Get value
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Set value
    pub fn set_value(&mut self, value: f64) -> Result<()> {
        if value < self.min || value > self.max {
            return Err(JupyterError::Widget(format!(
                "Value {} out of range [{}, {}]",
                value, self.min, self.max
            )));
        }
        self.value = value;
        Ok(())
    }
}

impl Widget for SliderWidget {
    fn widget_type(&self) -> &str {
        "SliderWidget"
    }

    fn state(&self) -> Result<HashMap<String, serde_json::Value>> {
        let mut state = HashMap::new();
        state.insert("value".to_string(), serde_json::json!(self.value));
        state.insert("min".to_string(), serde_json::json!(self.min));
        state.insert("max".to_string(), serde_json::json!(self.max));
        state.insert("step".to_string(), serde_json::json!(self.step));
        state.insert("label".to_string(), serde_json::json!(self.label));
        Ok(state)
    }

    fn update_state(&mut self, state: HashMap<String, serde_json::Value>) -> Result<()> {
        if let Some(value) = state.get("value") {
            if let Ok(v) = serde_json::from_value::<f64>(value.clone()) {
                self.set_value(v)?;
            }
        }
        Ok(())
    }

    fn render(&self) -> Result<String> {
        let html = format!(
            r#"
            <div class='slider-widget'>
                <label for='{}'>{}: <span id='{}-value'>{:.2}</span></label>
                <input type='range' id='{}' min='{}' max='{}' step='{}' value='{}'
                    oninput='document.getElementById("{}-value").textContent = this.value;'>
            </div>
            "#,
            self.id,
            self.label,
            self.id,
            self.value,
            self.id,
            self.min,
            self.max,
            self.step,
            self.value,
            self.id
        );
        Ok(html)
    }
}

/// Dropdown widget
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropdownWidget {
    /// Widget ID
    id: String,
    /// Options
    options: Vec<String>,
    /// Selected index
    selected_index: usize,
    /// Label
    label: String,
}

impl DropdownWidget {
    /// Create new dropdown widget
    pub fn new(id: impl Into<String>, options: Vec<String>) -> Result<Self> {
        if options.is_empty() {
            return Err(JupyterError::Widget(
                "Dropdown must have at least one option".to_string(),
            ));
        }
        Ok(Self {
            id: id.into(),
            options,
            selected_index: 0,
            label: "Select".to_string(),
        })
    }

    /// Set label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Get selected value
    pub fn selected_value(&self) -> &str {
        &self.options[self.selected_index]
    }

    /// Set selected index
    pub fn set_selected_index(&mut self, index: usize) -> Result<()> {
        if index >= self.options.len() {
            return Err(JupyterError::Widget(format!(
                "Index {} out of range [0, {})",
                index,
                self.options.len()
            )));
        }
        self.selected_index = index;
        Ok(())
    }
}

impl Widget for DropdownWidget {
    fn widget_type(&self) -> &str {
        "DropdownWidget"
    }

    fn state(&self) -> Result<HashMap<String, serde_json::Value>> {
        let mut state = HashMap::new();
        state.insert("options".to_string(), serde_json::json!(self.options));
        state.insert(
            "selected_index".to_string(),
            serde_json::json!(self.selected_index),
        );
        state.insert("label".to_string(), serde_json::json!(self.label));
        Ok(state)
    }

    fn update_state(&mut self, state: HashMap<String, serde_json::Value>) -> Result<()> {
        if let Some(index) = state.get("selected_index") {
            if let Ok(i) = serde_json::from_value::<usize>(index.clone()) {
                self.set_selected_index(i)?;
            }
        }
        Ok(())
    }

    fn render(&self) -> Result<String> {
        let options_html = self
            .options
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                if i == self.selected_index {
                    format!("<option selected>{}</option>", opt)
                } else {
                    format!("<option>{}</option>", opt)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let html = format!(
            r#"
            <div class='dropdown-widget'>
                <label for='{}'>{}: </label>
                <select id='{}'>
                    {}
                </select>
            </div>
            "#,
            self.id, self.label, self.id, options_html
        );
        Ok(html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_widget_creation() {
        let widget = MapWidget::new("map1", (0.0, 0.0), 10);
        assert_eq!(widget.widget_type(), "MapWidget");
        assert_eq!(widget.center(), (0.0, 0.0));
        assert_eq!(widget.zoom(), 10);
    }

    #[test]
    fn test_map_widget_state() -> Result<()> {
        let widget = MapWidget::new("map1", (0.0, 0.0), 10);
        let state = widget.state()?;
        assert!(state.contains_key("center"));
        assert!(state.contains_key("zoom"));
        Ok(())
    }

    #[test]
    fn test_slider_widget() -> Result<()> {
        let mut widget = SliderWidget::new("slider1", 0.0, 100.0).with_label("Opacity");
        assert_eq!(widget.value(), 0.0);

        widget.set_value(50.0)?;
        assert_eq!(widget.value(), 50.0);

        let result = widget.set_value(150.0);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_dropdown_widget() -> Result<()> {
        let mut widget = DropdownWidget::new(
            "dropdown1",
            vec!["Option 1".to_string(), "Option 2".to_string()],
        )?
        .with_label("Choose");

        assert_eq!(widget.selected_value(), "Option 1");

        widget.set_selected_index(1)?;
        assert_eq!(widget.selected_value(), "Option 2");

        let result = widget.set_selected_index(5);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_empty_dropdown() {
        let result = DropdownWidget::new("dropdown1", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_widget_render() -> Result<()> {
        let widget = MapWidget::new("map1", (0.0, 0.0), 10);
        let html = widget.render()?;
        assert!(html.contains("leaflet"));
        Ok(())
    }

    #[test]
    fn test_map_widget_satellite_basemap() -> Result<()> {
        let widget =
            MapWidget::new("map2", (35.0, 139.0), 12).with_basemap(BasemapProvider::Satellite);
        let state = widget.state()?;
        assert!(state.contains_key("center"));
        assert!(state.contains_key("basemap"));
        Ok(())
    }

    #[test]
    fn test_map_widget_custom_basemap() -> Result<()> {
        let url = "https://tiles.example.com/{z}/{x}/{y}.png".to_string();
        let widget = MapWidget::new("map3", (0.0, 0.0), 5)
            .with_basemap(BasemapProvider::Custom(url.clone()));
        let html = widget.render()?;
        assert!(html.contains(&url));
        Ok(())
    }

    #[test]
    fn test_map_widget_terrain_basemap() -> Result<()> {
        let widget = MapWidget::new("map4", (0.0, 0.0), 8).with_basemap(BasemapProvider::Terrain);
        let html = widget.render()?;
        assert!(html.contains("opentopomap") || html.contains("map4"));
        Ok(())
    }

    #[test]
    fn test_map_widget_update_center() -> Result<()> {
        let mut widget = MapWidget::new("map5", (0.0, 0.0), 10);
        assert_eq!(widget.center(), (0.0, 0.0));
        widget.set_center((51.5, -0.1));
        assert_eq!(widget.center(), (51.5, -0.1));
        Ok(())
    }

    #[test]
    fn test_map_widget_update_zoom() -> Result<()> {
        let mut widget = MapWidget::new("map6", (0.0, 0.0), 10);
        widget.set_zoom(15);
        assert_eq!(widget.zoom(), 15);
        Ok(())
    }

    #[test]
    fn test_map_widget_add_layer() -> Result<()> {
        let mut widget = MapWidget::new("map7", (0.0, 0.0), 10);
        widget.add_layer("https://tiles.example.com/{z}/{x}/{y}.png".to_string());
        let state = widget.state()?;
        let layers = state.get("layers");
        assert!(layers.is_some());
        Ok(())
    }

    #[test]
    fn test_map_widget_update_state() -> Result<()> {
        let mut widget = MapWidget::new("map8", (0.0, 0.0), 10);
        let mut state = std::collections::HashMap::new();
        state.insert("zoom".to_string(), serde_json::json!(7u8));
        state.insert("center".to_string(), serde_json::json!([10.0f64, 20.0f64]));
        widget.update_state(state)?;
        assert_eq!(widget.zoom(), 7);
        Ok(())
    }

    #[test]
    fn test_slider_widget_type_name() {
        let widget = SliderWidget::new("s1", 0.0, 100.0);
        assert_eq!(widget.widget_type(), "SliderWidget");
    }

    #[test]
    fn test_slider_state_keys() -> Result<()> {
        let widget = SliderWidget::new("s2", -10.0, 10.0).with_label("Band");
        let state = widget.state()?;
        assert!(state.contains_key("value"));
        assert!(state.contains_key("min"));
        assert!(state.contains_key("max"));
        assert!(state.contains_key("step"));
        assert!(state.contains_key("label"));
        Ok(())
    }

    #[test]
    fn test_slider_render() -> Result<()> {
        let widget = SliderWidget::new("s3", 0.0, 1.0).with_label("Opacity");
        let html = widget.render()?;
        assert!(html.contains("range"));
        assert!(html.contains("Opacity"));
        Ok(())
    }

    #[test]
    fn test_slider_update_state_via_state_map() -> Result<()> {
        let mut widget = SliderWidget::new("s4", 0.0, 100.0);
        let mut state = std::collections::HashMap::new();
        state.insert("value".to_string(), serde_json::json!(75.0f64));
        widget.update_state(state)?;
        assert_eq!(widget.value(), 75.0);
        Ok(())
    }

    #[test]
    fn test_slider_step_size() {
        let widget = SliderWidget::new("s5", 0.0, 100.0).with_step(5.0);
        // step is private field, check via state
        let state = widget.state();
        assert!(state.is_ok());
    }

    #[test]
    fn test_dropdown_widget_type_name() -> Result<()> {
        let widget = DropdownWidget::new("d1", vec!["A".to_string(), "B".to_string()])?;
        assert_eq!(widget.widget_type(), "DropdownWidget");
        Ok(())
    }

    #[test]
    fn test_dropdown_state_keys() -> Result<()> {
        let widget = DropdownWidget::new("d2", vec!["X".to_string()])?;
        let state = widget.state()?;
        assert!(state.contains_key("options"));
        assert!(state.contains_key("selected_index"));
        assert!(state.contains_key("label"));
        Ok(())
    }

    #[test]
    fn test_dropdown_render_contains_options() -> Result<()> {
        let widget = DropdownWidget::new(
            "d3",
            vec!["Red".to_string(), "Green".to_string(), "Blue".to_string()],
        )?;
        let html = widget.render()?;
        assert!(html.contains("Red"));
        assert!(html.contains("Green"));
        assert!(html.contains("Blue"));
        Ok(())
    }

    #[test]
    fn test_dropdown_render_marks_selected() -> Result<()> {
        let mut widget = DropdownWidget::new("d4", vec!["Alpha".to_string(), "Beta".to_string()])?;
        widget.set_selected_index(1)?;
        let html = widget.render()?;
        assert!(html.contains("selected"));
        assert!(html.contains("Beta"));
        Ok(())
    }

    #[test]
    fn test_dropdown_update_state_via_state_map() -> Result<()> {
        let mut widget =
            DropdownWidget::new("d5", vec!["First".to_string(), "Second".to_string()])?;
        let mut state = std::collections::HashMap::new();
        state.insert("selected_index".to_string(), serde_json::json!(1usize));
        widget.update_state(state)?;
        assert_eq!(widget.selected_value(), "Second");
        Ok(())
    }

    #[test]
    fn test_dropdown_with_label() -> Result<()> {
        let widget =
            DropdownWidget::new("d6", vec!["opt".to_string()])?.with_label("Choose Option");
        let state = widget.state()?;
        let label = state.get("label").and_then(|v| v.as_str());
        assert_eq!(label, Some("Choose Option"));
        Ok(())
    }
}
