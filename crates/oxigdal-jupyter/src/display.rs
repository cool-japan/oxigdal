//! Rich display support for Jupyter
//!
//! This module provides rich display capabilities for various data types
//! including images, maps, tables, and HTML.

use crate::{JupyterError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Display data for Jupyter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayData {
    /// Display data by MIME type
    pub data: HashMap<String, String>,
    /// Metadata
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Transient data
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub transient: HashMap<String, serde_json::Value>,
}

impl DisplayData {
    /// Create new display data
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            metadata: HashMap::new(),
            transient: HashMap::new(),
        }
    }

    /// Add text/plain representation
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.data.insert("text/plain".to_string(), text.into());
        self
    }

    /// Add text/html representation
    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.data.insert("text/html".to_string(), html.into());
        self
    }

    /// Add image/png representation (base64 encoded)
    pub fn with_png(mut self, png_data: &[u8]) -> Self {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_data);
        self.data.insert("image/png".to_string(), encoded);
        self
    }

    /// Add image/jpeg representation (base64 encoded)
    pub fn with_jpeg(mut self, jpeg_data: &[u8]) -> Self {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(jpeg_data);
        self.data.insert("image/jpeg".to_string(), encoded);
        self
    }

    /// Add application/json representation
    pub fn with_json(mut self, json: serde_json::Value) -> Result<Self> {
        let json_str = serde_json::to_string_pretty(&json)?;
        self.data.insert("application/json".to_string(), json_str);
        Ok(self)
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl Default for DisplayData {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for types that can be displayed in Jupyter
pub trait RichDisplay: std::fmt::Debug {
    /// Get display data
    fn display_data(&self) -> Result<DisplayData>;

    /// Get plain text representation
    fn display_text(&self) -> String {
        format!("{:?}", self)
    }

    /// Get HTML representation
    fn display_html(&self) -> Option<String> {
        None
    }

    /// Get image representation
    fn display_image(&self) -> Option<Vec<u8>> {
        None
    }
}

/// Table display
#[derive(Debug, Clone)]
pub struct Table {
    /// Column headers
    headers: Vec<String>,
    /// Row data
    rows: Vec<Vec<String>>,
    /// Table title
    title: Option<String>,
}

impl Table {
    /// Create new table
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
            title: None,
        }
    }

    /// Set table title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Add a row
    pub fn add_row(&mut self, row: Vec<String>) -> Result<()> {
        if row.len() != self.headers.len() {
            return Err(JupyterError::Display(format!(
                "Row length {} doesn't match header length {}",
                row.len(),
                self.headers.len()
            )));
        }
        self.rows.push(row);
        Ok(())
    }

    /// Build table
    pub fn build(self) -> Self {
        self
    }

    /// Convert to HTML
    fn to_html(&self) -> String {
        let mut html = String::from("<div class='oxigdal-table'>");

        if let Some(title) = &self.title {
            html.push_str(&format!("<h3>{}</h3>", title));
        }

        html.push_str("<table style='border-collapse: collapse; margin: 10px 0;'>");

        // Headers
        html.push_str("<thead><tr>");
        for header in &self.headers {
            html.push_str(&format!(
                "<th style='border: 1px solid #ddd; padding: 8px; background-color: #f2f2f2;'>{}</th>",
                header
            ));
        }
        html.push_str("</tr></thead>");

        // Rows
        html.push_str("<tbody>");
        for row in &self.rows {
            html.push_str("<tr>");
            for cell in row {
                html.push_str(&format!(
                    "<td style='border: 1px solid #ddd; padding: 8px;'>{}</td>",
                    cell
                ));
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody>");

        html.push_str("</table></div>");
        html
    }

    /// Convert to plain text
    fn to_text(&self) -> String {
        use comfy_table::Table as ComfyTable;
        use comfy_table::{Cell, CellAlignment, ContentArrangement, Row};

        let mut table = ComfyTable::new();
        table.set_content_arrangement(ContentArrangement::Dynamic);

        // Add header
        let mut header_row = Row::new();
        for h in &self.headers {
            header_row.add_cell(Cell::new(h).set_alignment(CellAlignment::Center));
        }
        table.set_header(header_row);

        // Add rows
        for row in &self.rows {
            let mut table_row = Row::new();
            for cell in row {
                table_row.add_cell(Cell::new(cell));
            }
            table.add_row(table_row);
        }

        if let Some(title) = &self.title {
            format!("{}\n\n{}", title, table)
        } else {
            table.to_string()
        }
    }
}

impl RichDisplay for Table {
    fn display_data(&self) -> Result<DisplayData> {
        let data = DisplayData::new()
            .with_text(self.to_text())
            .with_html(self.to_html());
        Ok(data)
    }

    fn display_text(&self) -> String {
        self.to_text()
    }

    fn display_html(&self) -> Option<String> {
        Some(self.to_html())
    }
}

/// Map display
#[derive(Debug, Clone)]
pub struct MapDisplay {
    /// Center coordinates (lon, lat)
    center: (f64, f64),
    /// Zoom level
    zoom: u8,
    /// Width in pixels
    width: u32,
    /// Height in pixels
    height: u32,
    /// Layers
    layers: Vec<MapLayer>,
}

/// Map layer
#[derive(Debug, Clone)]
pub struct MapLayer {
    /// Layer name
    name: String,
    /// Layer type
    layer_type: LayerType,
    /// Layer data
    data: String,
}

impl MapLayer {
    /// Create a new map layer
    pub fn new(name: impl Into<String>, layer_type: LayerType, data: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            layer_type,
            data: data.into(),
        }
    }

    /// Get the layer name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the layer type
    pub fn layer_type(&self) -> &LayerType {
        &self.layer_type
    }

    /// Get the layer data
    pub fn data(&self) -> &str {
        &self.data
    }
}

/// Layer type
#[derive(Debug, Clone)]
pub enum LayerType {
    /// Raster layer
    Raster,
    /// Vector layer
    Vector,
    /// Tile layer
    Tile,
}

impl MapDisplay {
    /// Create new map display
    pub fn new(center: (f64, f64), zoom: u8) -> Self {
        Self {
            center,
            zoom,
            width: 800,
            height: 600,
            layers: Vec::new(),
        }
    }

    /// Set dimensions
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Add a layer
    pub fn add_layer(&mut self, layer: MapLayer) {
        self.layers.push(layer);
    }

    /// Generate Leaflet HTML
    fn to_html(&self) -> String {
        format!(
            r#"
            <div id='map' style='width: {}px; height: {}px;'></div>
            <script>
            var map = L.map('map').setView([{}, {}], {});
            L.tileLayer('https://{{s}}.tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png', {{
                attribution: '&copy; OpenStreetMap contributors'
            }}).addTo(map);
            </script>
            "#,
            self.width, self.height, self.center.1, self.center.0, self.zoom
        )
    }
}

impl RichDisplay for MapDisplay {
    fn display_data(&self) -> Result<DisplayData> {
        let data = DisplayData::new()
            .with_text(format!(
                "Map centered at {:?}, zoom {}",
                self.center, self.zoom
            ))
            .with_html(self.to_html());
        Ok(data)
    }

    fn display_text(&self) -> String {
        format!("Map centered at {:?}, zoom {}", self.center, self.zoom)
    }

    fn display_html(&self) -> Option<String> {
        Some(self.to_html())
    }
}

/// Image display
#[derive(Debug, Clone)]
pub struct ImageDisplay {
    /// Image data (PNG or JPEG)
    data: Vec<u8>,
    /// Image format
    format: ImageFormat,
    /// Width
    width: u32,
    /// Height
    height: u32,
}

/// Image format
#[derive(Debug, Clone, Copy)]
pub enum ImageFormat {
    /// PNG format
    Png,
    /// JPEG format
    Jpeg,
}

impl ImageDisplay {
    /// Create new image display from PNG data
    pub fn from_png(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            data,
            format: ImageFormat::Png,
            width,
            height,
        }
    }

    /// Create new image display from JPEG data
    pub fn from_jpeg(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            data,
            format: ImageFormat::Jpeg,
            width,
            height,
        }
    }
}

impl RichDisplay for ImageDisplay {
    fn display_data(&self) -> Result<DisplayData> {
        let mut data = DisplayData::new().with_text(format!(
            "Image ({}x{}, {:?})",
            self.width, self.height, self.format
        ));

        match self.format {
            ImageFormat::Png => data = data.with_png(&self.data),
            ImageFormat::Jpeg => data = data.with_jpeg(&self.data),
        }

        Ok(data)
    }

    fn display_text(&self) -> String {
        format!("Image ({}x{}, {:?})", self.width, self.height, self.format)
    }

    fn display_image(&self) -> Option<Vec<u8>> {
        Some(self.data.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_data_creation() {
        let display = DisplayData::new()
            .with_text("Hello, World!")
            .with_html("<h1>Hello, World!</h1>");

        assert_eq!(
            display.data.get("text/plain").map(|s| s.as_str()),
            Some("Hello, World!")
        );
        assert_eq!(
            display.data.get("text/html").map(|s| s.as_str()),
            Some("<h1>Hello, World!</h1>")
        );
    }

    #[test]
    fn test_table_creation() -> Result<()> {
        let mut table =
            Table::new(vec!["Name".to_string(), "Value".to_string()]).with_title("Test Table");
        table.add_row(vec!["x".to_string(), "10".to_string()])?;
        table.add_row(vec!["y".to_string(), "20".to_string()])?;

        let text = table.to_text();
        assert!(text.contains("Name"));
        assert!(text.contains("Value"));
        Ok(())
    }

    #[test]
    fn test_table_row_length_mismatch() {
        let mut table = Table::new(vec!["A".to_string(), "B".to_string()]);
        let result = table.add_row(vec!["1".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_display() -> Result<()> {
        let map = MapDisplay::new((0.0, 0.0), 10).with_dimensions(600, 400);
        let display = map.display_data()?;
        assert!(display.data.contains_key("text/plain"));
        assert!(display.data.contains_key("text/html"));
        Ok(())
    }

    #[test]
    fn test_image_display() -> Result<()> {
        let img = ImageDisplay::from_png(vec![0, 1, 2, 3], 100, 100);
        let display = img.display_data()?;
        assert!(display.data.contains_key("text/plain"));
        assert!(display.data.contains_key("image/png"));
        Ok(())
    }

    #[test]
    fn test_display_data_default() {
        let display = DisplayData::default();
        assert!(display.data.is_empty());
        assert!(display.metadata.is_empty());
        assert!(display.transient.is_empty());
    }

    #[test]
    fn test_display_data_with_json() -> Result<()> {
        let display = DisplayData::new().with_json(serde_json::json!({"key": "value"}))?;
        assert!(display.data.contains_key("application/json"));
        let json_str = display.data.get("application/json").map(|s| s.as_str());
        assert!(json_str.is_some());
        assert!(json_str.unwrap_or("").contains("key"));
        Ok(())
    }

    #[test]
    fn test_display_data_with_metadata() {
        let display = DisplayData::new()
            .with_metadata("width", serde_json::json!(800))
            .with_metadata("height", serde_json::json!(600));
        assert_eq!(display.metadata.get("width"), Some(&serde_json::json!(800)));
        assert_eq!(
            display.metadata.get("height"),
            Some(&serde_json::json!(600))
        );
    }

    #[test]
    fn test_display_data_with_jpeg() -> Result<()> {
        let img = ImageDisplay::from_jpeg(vec![0xff, 0xd8, 0xff], 50, 50);
        let display = img.display_data()?;
        assert!(display.data.contains_key("image/jpeg"));
        let text = img.display_text();
        assert!(text.contains("50x50"));
        Ok(())
    }

    #[test]
    fn test_image_display_image() {
        let data = vec![1, 2, 3, 4, 5];
        let img = ImageDisplay::from_png(data.clone(), 10, 10);
        let raw = img.display_image();
        assert_eq!(raw, Some(data));
    }

    #[test]
    fn test_table_html_contains_headers() -> Result<()> {
        let mut table = Table::new(vec!["Name".to_string(), "Score".to_string()]);
        table.add_row(vec!["Alice".to_string(), "95".to_string()])?;
        let html = table.to_html();
        assert!(html.contains("Name"));
        assert!(html.contains("Score"));
        assert!(html.contains("Alice"));
        assert!(html.contains("95"));
        Ok(())
    }

    #[test]
    fn test_table_title_in_html() -> Result<()> {
        let table = Table::new(vec!["Col".to_string()])
            .with_title("My Table")
            .build();
        let html = table.to_html();
        assert!(html.contains("My Table"));
        Ok(())
    }

    #[test]
    fn test_table_display_data() -> Result<()> {
        let table = Table::new(vec!["A".to_string(), "B".to_string()]);
        let data = table.display_data()?;
        assert!(data.data.contains_key("text/plain"));
        assert!(data.data.contains_key("text/html"));
        Ok(())
    }

    #[test]
    fn test_table_display_html_method() {
        let table = Table::new(vec!["X".to_string()]);
        assert!(table.display_html().is_some());
    }

    #[test]
    fn test_map_display_text() {
        let map = MapDisplay::new((10.5, 20.3), 5);
        let text = map.display_text();
        assert!(text.contains("10.5"));
        assert!(text.contains("20.3"));
        assert!(text.contains("5"));
    }

    #[test]
    fn test_map_display_html_method() {
        let map = MapDisplay::new((0.0, 0.0), 2);
        assert!(map.display_html().is_some());
    }

    #[test]
    fn test_map_display_html_includes_dimensions() {
        let map = MapDisplay::new((0.0, 0.0), 10).with_dimensions(1024, 768);
        let html = map.display_html().unwrap_or_default();
        assert!(html.contains("1024"));
        assert!(html.contains("768"));
    }

    #[test]
    fn test_map_display_add_layer() {
        let mut map = MapDisplay::new((0.0, 0.0), 10);
        map.add_layer(MapLayer::new("layer1", LayerType::Raster, "data://raster"));
        assert_eq!(map.layers.len(), 1);
        assert_eq!(map.layers[0].name(), "layer1");
    }

    #[test]
    fn test_map_layer_types() {
        let raster = MapLayer::new("r", LayerType::Raster, "data");
        let vector = MapLayer::new("v", LayerType::Vector, "data");
        let tile = MapLayer::new("t", LayerType::Tile, "data");
        assert!(matches!(raster.layer_type(), LayerType::Raster));
        assert!(matches!(vector.layer_type(), LayerType::Vector));
        assert!(matches!(tile.layer_type(), LayerType::Tile));
        assert_eq!(tile.data(), "data");
    }

    #[test]
    fn test_png_base64_encoding() {
        let raw = b"\x89PNG\r\n\x1a\n"; // PNG magic bytes
        let display = DisplayData::new().with_png(raw);
        let encoded = display.data.get("image/png");
        assert!(encoded.is_some());
        // Verify it is valid base64
        use base64::Engine;
        let decoded =
            base64::engine::general_purpose::STANDARD.decode(encoded.unwrap_or(&String::new()));
        assert!(decoded.is_ok());
        assert_eq!(decoded.unwrap_or_default(), raw);
    }

    #[test]
    fn test_table_multiple_rows() -> Result<()> {
        let mut table = Table::new(vec!["ID".to_string(), "Value".to_string()]);
        for i in 0..5 {
            table.add_row(vec![i.to_string(), (i * 10).to_string()])?;
        }
        let data = table.display_data()?;
        let text = data
            .data
            .get("text/plain")
            .map(|s| s.as_str())
            .unwrap_or("");
        assert!(text.contains("40")); // 4 * 10
        Ok(())
    }
}
