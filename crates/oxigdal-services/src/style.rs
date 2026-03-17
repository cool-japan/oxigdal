//! Mapbox GL Style Specification v8 core types.
//!
//! Implements the Mapbox GL Style Spec v8 for dynamic map style rendering,
//! including style documents, sources, layers, filters, expressions, paint
//! properties, layout properties, and style validation/rendering utilities.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// Error types
// ─────────────────────────────────────────────────────────────────────────────

/// Errors that can occur when parsing or validating a Mapbox GL style.
#[derive(Debug, Error)]
pub enum StyleError {
    /// Style version must be 8.
    #[error("invalid style version {0}; must be 8")]
    InvalidVersion(u8),

    /// Encountered an unknown layer type string.
    #[error("unknown layer type: {0}")]
    UnknownLayerType(String),

    /// Failed to parse a CSS color string.
    #[error("color parse error: {0}")]
    ColorParseError(String),

    /// Failed to parse a filter expression.
    #[error("invalid filter: {0}")]
    InvalidFilter(String),

    /// A serde_json serialisation/deserialisation error.
    #[error("serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
}

// ─────────────────────────────────────────────────────────────────────────────
// Root style document
// ─────────────────────────────────────────────────────────────────────────────

/// Root Mapbox GL Style Specification v8 document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleSpec {
    /// Must be 8.
    pub version: u8,
    /// Human-readable name for the style.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Default map center `[longitude, latitude]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 2]>,
    /// Default zoom level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    /// Default bearing (degrees clockwise from north).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<f64>,
    /// Default pitch (degrees toward horizon).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    /// Global light source settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<Light>,
    /// Data sources available to layers.
    pub sources: HashMap<String, Source>,
    /// URL template for sprite images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
    /// URL template for glyph PBF files (`{fontstack}` / `{range}`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glyphs: Option<String>,
    /// Default transition options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// Ordered list of rendering layers.
    pub layers: Vec<Layer>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Sources
// ─────────────────────────────────────────────────────────────────────────────

/// A data source referenced by one or more layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Source {
    /// Vector tile source.
    Vector {
        /// TileJSON URL or direct tile URL.
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// Explicit tile URL templates.
        #[serde(skip_serializing_if = "Option::is_none")]
        tiles: Option<Vec<String>>,
        /// Minimum zoom level.
        #[serde(rename = "minzoom", skip_serializing_if = "Option::is_none")]
        min_zoom: Option<u8>,
        /// Maximum zoom level.
        #[serde(rename = "maxzoom", skip_serializing_if = "Option::is_none")]
        max_zoom: Option<u8>,
        /// HTML attribution string.
        #[serde(skip_serializing_if = "Option::is_none")]
        attribution: Option<String>,
    },
    /// Raster tile source.
    Raster {
        /// TileJSON URL.
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// Explicit tile URL templates.
        #[serde(skip_serializing_if = "Option::is_none")]
        tiles: Option<Vec<String>>,
        /// Tile size in pixels.
        #[serde(rename = "tileSize", skip_serializing_if = "Option::is_none")]
        tile_size: Option<u32>,
        /// Minimum zoom level.
        #[serde(rename = "minzoom", skip_serializing_if = "Option::is_none")]
        min_zoom: Option<u8>,
        /// Maximum zoom level.
        #[serde(rename = "maxzoom", skip_serializing_if = "Option::is_none")]
        max_zoom: Option<u8>,
    },
    /// Raster DEM elevation source.
    #[serde(rename = "raster-dem")]
    RasterDem {
        /// TileJSON URL.
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// Elevation encoding scheme.
        #[serde(default)]
        encoding: DemEncoding,
    },
    /// GeoJSON data source.
    #[serde(rename = "geojson")]
    GeoJson {
        /// Inline GeoJSON or URL.
        data: serde_json::Value,
        /// Maximum zoom for tile index.
        #[serde(rename = "maxzoom", skip_serializing_if = "Option::is_none")]
        max_zoom: Option<u8>,
        /// Enable feature clustering.
        #[serde(skip_serializing_if = "Option::is_none")]
        cluster: Option<bool>,
        /// Cluster radius in pixels.
        #[serde(rename = "clusterRadius", skip_serializing_if = "Option::is_none")]
        cluster_radius: Option<u32>,
    },
    /// Image overlay source.
    Image {
        /// Image URL.
        url: String,
        /// `[[lng,lat]; 4]` corner coordinates (TL, TR, BR, BL).
        coordinates: [[f64; 2]; 4],
    },
    /// Video overlay source.
    Video {
        /// Video URLs (multiple formats for browser compatibility).
        urls: Vec<String>,
        /// Corner coordinates.
        coordinates: [[f64; 2]; 4],
    },
}

/// Elevation encoding for raster-dem sources.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DemEncoding {
    /// Mapbox Terrain-RGB encoding.
    #[default]
    Mapbox,
    /// Terrarium encoding.
    Terrarium,
}

// ─────────────────────────────────────────────────────────────────────────────
// Layers
// ─────────────────────────────────────────────────────────────────────────────

/// A single rendering layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// Unique layer identifier.
    pub id: String,
    /// The rendering type.
    #[serde(rename = "type")]
    pub layer_type: LayerType,
    /// Source name from [`StyleSpec::sources`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Source layer within a vector tile source.
    #[serde(rename = "source-layer", skip_serializing_if = "Option::is_none")]
    pub source_layer: Option<String>,
    /// Minimum zoom level at which the layer is visible.
    #[serde(rename = "minzoom", skip_serializing_if = "Option::is_none")]
    pub min_zoom: Option<f64>,
    /// Maximum zoom level at which the layer is visible.
    #[serde(rename = "maxzoom", skip_serializing_if = "Option::is_none")]
    pub max_zoom: Option<f64>,
    /// Feature filter expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    /// Layout properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    /// Paint properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paint: Option<Paint>,
}

/// Layer rendering type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LayerType {
    /// Solid-color background.
    Background,
    /// Filled polygon layer.
    Fill,
    /// Line/stroke layer.
    Line,
    /// Symbol (icon + text) layer.
    Symbol,
    /// Raster image layer.
    Raster,
    /// Circle layer.
    Circle,
    /// Extruded fill (3-D buildings).
    FillExtrusion,
    /// Heatmap density layer.
    Heatmap,
    /// Hillshade terrain layer.
    Hillshade,
    /// Sky / atmosphere layer.
    Sky,
}

// ─────────────────────────────────────────────────────────────────────────────
// Filters
// ─────────────────────────────────────────────────────────────────────────────

/// Geometry type for geometry-type filters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GeomFilter {
    /// Point geometry.
    Point,
    /// LineString geometry.
    LineString,
    /// Polygon geometry.
    Polygon,
}

/// Mapbox GL filter expression (v2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Filter {
    /// All sub-filters must match.
    All(#[serde(skip)] Vec<Filter>),
    /// At least one sub-filter must match.
    Any(#[serde(skip)] Vec<Filter>),
    /// No sub-filter must match.
    None(#[serde(skip)] Vec<Filter>),
    /// Property equals value.
    Eq {
        /// Feature property name.
        property: String,
        /// Expected value.
        value: serde_json::Value,
    },
    /// Property not equal to value.
    Ne {
        /// Feature property name.
        property: String,
        /// Excluded value.
        value: serde_json::Value,
    },
    /// Property less than value.
    Lt {
        /// Feature property name.
        property: String,
        /// Threshold.
        value: f64,
    },
    /// Property less than or equal to value.
    Lte {
        /// Feature property name.
        property: String,
        /// Threshold.
        value: f64,
    },
    /// Property greater than value.
    Gt {
        /// Feature property name.
        property: String,
        /// Threshold.
        value: f64,
    },
    /// Property greater than or equal to value.
    Gte {
        /// Feature property name.
        property: String,
        /// Threshold.
        value: f64,
    },
    /// Property value is in a set.
    In {
        /// Feature property name.
        property: String,
        /// Allowed values.
        values: Vec<serde_json::Value>,
    },
    /// Property exists.
    Has(String),
    /// Property does not exist.
    NotHas(String),
    /// Geometry type matches.
    GeometryType(GeomFilter),
}

// ─────────────────────────────────────────────────────────────────────────────
// Expressions
// ─────────────────────────────────────────────────────────────────────────────

/// Interpolation type for [`Expression::Interpolate`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interpolation {
    /// Linear interpolation.
    Linear,
    /// Exponential interpolation with base.
    Exponential(f64),
    /// Cubic-bezier interpolation with four control-point components.
    CubicBezier([f64; 4]),
}

/// Mapbox GL expression (core subset).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Expression {
    /// `["get", property]` — read a feature property.
    Get(String),
    /// `["has", property]` — check whether a property exists.
    Has(String),
    /// A JSON literal value.
    Literal(serde_json::Value),
    /// An array of sub-expressions.
    Array(Vec<Expression>),
    /// `["case", cond, val, cond, val, …, fallback]`.
    Case {
        /// `(condition, result)` pairs.
        conditions: Vec<(Expression, Expression)>,
        /// Default result.
        fallback: Box<Expression>,
    },
    /// `["match", input, label, val, …, fallback]`.
    Match {
        /// Input expression.
        input: Box<Expression>,
        /// `(label, result)` pairs.
        cases: Vec<(serde_json::Value, Expression)>,
        /// Default result.
        fallback: Box<Expression>,
    },
    /// `["interpolate", interpolation, input, stop, val, …]`.
    Interpolate {
        /// Interpolation method.
        interpolation: Interpolation,
        /// Input expression (e.g. `Zoom`).
        input: Box<Expression>,
        /// `(stop, value)` pairs.
        stops: Vec<(f64, Expression)>,
    },
    /// `["step", input, default, stop, val, …]`.
    Step {
        /// Input expression.
        input: Box<Expression>,
        /// Value below the first stop.
        default: Box<Expression>,
        /// `(stop, value)` pairs.
        stops: Vec<(f64, Expression)>,
    },
    /// `["zoom"]` — current zoom level.
    Zoom,
    /// Addition.
    Add(Box<Expression>, Box<Expression>),
    /// Subtraction.
    Subtract(Box<Expression>, Box<Expression>),
    /// Multiplication.
    Multiply(Box<Expression>, Box<Expression>),
    /// Division.
    Divide(Box<Expression>, Box<Expression>),
    /// `["coalesce", …]` — first non-null result.
    Coalesce(Vec<Expression>),
}

/// Either a literal value or a data-driven [`Expression`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue<T: Clone> {
    /// A constant value.
    Literal(T),
    /// A data-driven expression.
    Expression(Expression),
}

// ─────────────────────────────────────────────────────────────────────────────
// Color
// ─────────────────────────────────────────────────────────────────────────────

/// An RGBA color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Color {
    /// Red channel (0–255).
    pub r: u8,
    /// Green channel (0–255).
    pub g: u8,
    /// Blue channel (0–255).
    pub b: u8,
    /// Alpha channel (0.0–1.0).
    pub a: f32,
}

impl Color {
    /// Parse a CSS color string: `#rrggbb`, `#rgb`, `rgb(r,g,b)`, `rgba(r,g,b,a)`.
    pub fn parse(s: &str) -> Result<Self, StyleError> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            Self::parse_hex(hex)
        } else if let Some(inner) = s.strip_prefix("rgba(").and_then(|t| t.strip_suffix(')')) {
            Self::parse_rgba(inner)
        } else if let Some(inner) = s.strip_prefix("rgb(").and_then(|t| t.strip_suffix(')')) {
            Self::parse_rgb(inner)
        } else {
            Err(StyleError::ColorParseError(format!(
                "unsupported color format: {s}"
            )))
        }
    }

    fn parse_hex(hex: &str) -> Result<Self, StyleError> {
        let err = || StyleError::ColorParseError(format!("invalid hex color: #{hex}"));
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| err())?;
                let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| err())?;
                let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| err())?;
                Ok(Color { r, g, b, a: 1.0 })
            }
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).map_err(|_| err())?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).map_err(|_| err())?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).map_err(|_| err())?;
                Ok(Color { r, g, b, a: 1.0 })
            }
            _ => Err(err()),
        }
    }

    fn parse_rgb(inner: &str) -> Result<Self, StyleError> {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 3 {
            return Err(StyleError::ColorParseError(format!(
                "rgb() expects 3 components, got {}",
                parts.len()
            )));
        }
        let parse_u8 = |s: &str| -> Result<u8, StyleError> {
            s.trim()
                .parse::<u8>()
                .map_err(|_| StyleError::ColorParseError(format!("invalid channel value: {s}")))
        };
        Ok(Color {
            r: parse_u8(parts[0])?,
            g: parse_u8(parts[1])?,
            b: parse_u8(parts[2])?,
            a: 1.0,
        })
    }

    fn parse_rgba(inner: &str) -> Result<Self, StyleError> {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 4 {
            return Err(StyleError::ColorParseError(format!(
                "rgba() expects 4 components, got {}",
                parts.len()
            )));
        }
        let parse_u8 = |s: &str| -> Result<u8, StyleError> {
            s.trim()
                .parse::<u8>()
                .map_err(|_| StyleError::ColorParseError(format!("invalid channel value: {s}")))
        };
        let a: f32 = parts[3]
            .trim()
            .parse()
            .map_err(|_| StyleError::ColorParseError(format!("invalid alpha: {}", parts[3])))?;
        Ok(Color {
            r: parse_u8(parts[0])?,
            g: parse_u8(parts[1])?,
            b: parse_u8(parts[2])?,
            a,
        })
    }

    /// Serialize to a CSS `rgba(r,g,b,a)` string.
    pub fn to_css(&self) -> String {
        format!("rgba({},{},{},{:.6})", self.r, self.g, self.b, self.a)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Paint
// ─────────────────────────────────────────────────────────────────────────────

/// Paint properties for a layer (flexible map of property name → JSON value).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Paint(pub HashMap<String, serde_json::Value>);

impl Paint {
    /// Helper: attempt to deserialise a key as `PropertyValue<T>`.
    fn get_pv<T>(&self, key: &str) -> Option<PropertyValue<T>>
    where
        T: Clone + for<'de> Deserialize<'de>,
    {
        let v = self.0.get(key)?;
        serde_json::from_value::<PropertyValue<T>>(v.clone()).ok()
    }

    /// `fill-color` paint property.
    pub fn fill_color(&self) -> Option<PropertyValue<Color>> {
        self.get_pv("fill-color")
    }

    /// `fill-opacity` paint property.
    pub fn fill_opacity(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("fill-opacity")
    }

    /// `line-color` paint property.
    pub fn line_color(&self) -> Option<PropertyValue<Color>> {
        self.get_pv("line-color")
    }

    /// `line-width` paint property.
    pub fn line_width(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("line-width")
    }

    /// `line-opacity` paint property.
    pub fn line_opacity(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("line-opacity")
    }

    /// `circle-color` paint property.
    pub fn circle_color(&self) -> Option<PropertyValue<Color>> {
        self.get_pv("circle-color")
    }

    /// `circle-radius` paint property.
    pub fn circle_radius(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("circle-radius")
    }

    /// `raster-opacity` paint property.
    pub fn raster_opacity(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("raster-opacity")
    }

    /// `raster-hue-rotate` paint property.
    pub fn raster_hue_rotate(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("raster-hue-rotate")
    }

    /// `background-color` paint property.
    pub fn background_color(&self) -> Option<PropertyValue<Color>> {
        self.get_pv("background-color")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout
// ─────────────────────────────────────────────────────────────────────────────

/// Layout properties for a layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Layout(pub HashMap<String, serde_json::Value>);

impl Layout {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.as_str()
    }

    fn get_pv<T>(&self, key: &str) -> Option<PropertyValue<T>>
    where
        T: Clone + for<'de> Deserialize<'de>,
    {
        let v = self.0.get(key)?;
        serde_json::from_value::<PropertyValue<T>>(v.clone()).ok()
    }

    /// Layer visibility.
    pub fn visibility(&self) -> Visibility {
        match self.get_str("visibility") {
            Some("none") => Visibility::None,
            _ => Visibility::Visible,
        }
    }

    /// Line cap style.
    pub fn line_cap(&self) -> LineCap {
        match self.get_str("line-cap") {
            Some("round") => LineCap::Round,
            Some("square") => LineCap::Square,
            _ => LineCap::Butt,
        }
    }

    /// Line join style.
    pub fn line_join(&self) -> LineJoin {
        match self.get_str("line-join") {
            Some("round") => LineJoin::Round,
            Some("miter") => LineJoin::Miter,
            _ => LineJoin::Bevel,
        }
    }

    /// Symbol placement.
    pub fn symbol_placement(&self) -> SymbolPlacement {
        match self.get_str("symbol-placement") {
            Some("line") => SymbolPlacement::Line,
            Some("line-center") => SymbolPlacement::LineCenter,
            _ => SymbolPlacement::Point,
        }
    }

    /// `text-field` layout property.
    pub fn text_field(&self) -> Option<PropertyValue<String>> {
        self.get_pv("text-field")
    }

    /// `text-size` layout property.
    pub fn text_size(&self) -> Option<PropertyValue<f64>> {
        self.get_pv("text-size")
    }

    /// `icon-image` layout property.
    pub fn icon_image(&self) -> Option<PropertyValue<String>> {
        self.get_pv("icon-image")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout enums
// ─────────────────────────────────────────────────────────────────────────────

/// Layer visibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Layer is rendered.
    #[default]
    Visible,
    /// Layer is hidden.
    None,
}

/// Line cap style.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LineCap {
    /// Flat square cap at endpoint.
    #[default]
    Butt,
    /// Rounded cap.
    Round,
    /// Square cap extending past endpoint.
    Square,
}

/// Line join style.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LineJoin {
    /// Flat bevel join.
    #[default]
    Bevel,
    /// Rounded join.
    Round,
    /// Sharp miter join.
    Miter,
}

/// Symbol placement along a line or at a point.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolPlacement {
    /// Placed at the feature's point / centroid.
    #[default]
    Point,
    /// Placed along the line.
    Line,
    /// Placed at the centre of the line.
    LineCenter,
}

// ─────────────────────────────────────────────────────────────────────────────
// Transition
// ─────────────────────────────────────────────────────────────────────────────

/// Default transition options for paint property changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    /// Transition duration in milliseconds.
    #[serde(default)]
    pub duration: u32,
    /// Transition delay in milliseconds.
    #[serde(default)]
    pub delay: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Light
// ─────────────────────────────────────────────────────────────────────────────

/// Global light source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Light {
    /// Reference frame for the light position.
    #[serde(default)]
    pub anchor: LightAnchor,
    /// Light color.
    pub color: Color,
    /// Light intensity (0–1).
    #[serde(default = "default_intensity")]
    pub intensity: f64,
    /// `[radial, azimuthal, polar]` position.
    #[serde(default = "default_light_position")]
    pub position: [f64; 3],
}

fn default_intensity() -> f64 {
    0.5
}

fn default_light_position() -> [f64; 3] {
    [1.15, 210.0, 30.0]
}

/// Anchor reference frame for the light source.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LightAnchor {
    /// Light position is in viewport space.
    #[default]
    Viewport,
    /// Light position is in map space.
    Map,
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation
// ─────────────────────────────────────────────────────────────────────────────

/// A single validation diagnostic.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Layer id, if the error is layer-specific.
    pub layer_id: Option<String>,
    /// Human-readable description of the problem.
    pub message: String,
}

/// Validates a [`StyleSpec`] for structural correctness.
pub struct StyleValidator;

impl StyleValidator {
    /// Validate a style specification and return any errors found.
    pub fn validate(spec: &StyleSpec) -> Vec<ValidationError> {
        let mut errors: Vec<ValidationError> = Vec::new();

        // version must be 8
        if spec.version != 8 {
            errors.push(ValidationError {
                layer_id: None,
                message: format!("style version must be 8, got {}", spec.version),
            });
        }

        // no duplicate layer ids
        let mut seen_ids: HashMap<&str, usize> = HashMap::new();
        for (idx, layer) in spec.layers.iter().enumerate() {
            if let Some(prev) = seen_ids.insert(layer.id.as_str(), idx) {
                errors.push(ValidationError {
                    layer_id: Some(layer.id.clone()),
                    message: format!(
                        "duplicate layer id '{}' (first at index {prev}, repeated at index {idx})",
                        layer.id
                    ),
                });
            }
        }

        for layer in &spec.layers {
            // check source references
            if let Some(src) = &layer.source {
                if !spec.sources.contains_key(src.as_str()) {
                    errors.push(ValidationError {
                        layer_id: Some(layer.id.clone()),
                        message: format!("layer references unknown source '{src}'"),
                    });
                }
            }

            // zoom range sanity
            if let (Some(min), Some(max)) = (layer.min_zoom, layer.max_zoom) {
                if min > max {
                    errors.push(ValidationError {
                        layer_id: Some(layer.id.clone()),
                        message: format!("minzoom ({min}) must be <= maxzoom ({max})"),
                    });
                }
            }

            // background layers must have no source
            if layer.layer_type == LayerType::Background && layer.source.is_some() {
                errors.push(ValidationError {
                    layer_id: Some(layer.id.clone()),
                    message: "background layer must not reference a source".to_string(),
                });
            }

            // fill/line/circle/symbol layers must have a source
            let requires_source = matches!(
                layer.layer_type,
                LayerType::Fill | LayerType::Line | LayerType::Circle | LayerType::Symbol
            );
            if requires_source && layer.source.is_none() {
                errors.push(ValidationError {
                    layer_id: Some(layer.id.clone()),
                    message: format!("{:?} layer requires a source", layer.layer_type),
                });
            }
        }

        errors
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendering / evaluation
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluates paint properties and filters at runtime.
pub struct StyleRenderer;

impl StyleRenderer {
    /// Evaluate a `PropertyValue<Color>` at a given zoom level.
    ///
    /// Returns the literal color or the result of evaluating an interpolation
    /// expression; falls back to opaque black for complex expressions not yet
    /// handled by this evaluator.
    pub fn eval_zoom_color(value: &PropertyValue<Color>, zoom: f64) -> Color {
        match value {
            PropertyValue::Literal(c) => c.clone(),
            PropertyValue::Expression(expr) => Self::eval_expr_color(expr, zoom).unwrap_or(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 1.0,
            }),
        }
    }

    fn eval_expr_color(expr: &Expression, zoom: f64) -> Option<Color> {
        match expr {
            Expression::Literal(v) => {
                let s = v.as_str()?;
                Color::parse(s).ok()
            }
            Expression::Interpolate {
                interpolation,
                input,
                stops,
            } => {
                let input_val = Self::eval_expr_f64(input, zoom)?;
                if stops.is_empty() {
                    return None;
                }
                // find surrounding stops
                let (lo_stop, lo_expr, hi_stop, hi_expr) =
                    Self::find_stops_color(stops, input_val)?;
                let lo_c = Self::eval_expr_color(lo_expr, zoom)?;
                let hi_c = Self::eval_expr_color(hi_expr, zoom)?;
                let t = Self::interp_t(interpolation, input_val, lo_stop, hi_stop);
                Some(lerp_color(&lo_c, &hi_c, t))
            }
            _ => None,
        }
    }

    fn find_stops_color(
        stops: &[(f64, Expression)],
        input: f64,
    ) -> Option<(f64, &Expression, f64, &Expression)> {
        if stops.len() == 1 {
            return Some((stops[0].0, &stops[0].1, stops[0].0, &stops[0].1));
        }
        let last = stops.last()?;
        if input >= last.0 {
            let second_last = &stops[stops.len() - 2];
            return Some((second_last.0, &second_last.1, last.0, &last.1));
        }
        let first = stops.first()?;
        if input <= first.0 {
            let second = &stops[1];
            return Some((first.0, &first.1, second.0, &second.1));
        }
        for i in 0..stops.len() - 1 {
            if input >= stops[i].0 && input < stops[i + 1].0 {
                return Some((stops[i].0, &stops[i].1, stops[i + 1].0, &stops[i + 1].1));
            }
        }
        None
    }

    /// Evaluate a `PropertyValue<f64>` at a given zoom level.
    ///
    /// Supports `Literal`, `Expression::Zoom`, and `Expression::Interpolate`
    /// with `Linear` and `Exponential` interpolation. Returns `0.0` for
    /// unrecognised expressions.
    pub fn eval_zoom_f64(value: &PropertyValue<f64>, zoom: f64) -> f64 {
        match value {
            PropertyValue::Literal(v) => *v,
            PropertyValue::Expression(expr) => Self::eval_expr_f64(expr, zoom).unwrap_or(0.0),
        }
    }

    fn eval_expr_f64(expr: &Expression, zoom: f64) -> Option<f64> {
        match expr {
            Expression::Zoom => Some(zoom),
            Expression::Literal(v) => v.as_f64(),
            Expression::Interpolate {
                interpolation,
                input,
                stops,
            } => {
                let input_val = Self::eval_expr_f64(input, zoom)?;
                if stops.is_empty() {
                    return None;
                }
                if stops.len() == 1 {
                    return Self::eval_expr_f64(&stops[0].1, zoom);
                }
                let last = stops.last()?;
                if input_val >= last.0 {
                    return Self::eval_expr_f64(&last.1, zoom);
                }
                let first = stops.first()?;
                if input_val <= first.0 {
                    return Self::eval_expr_f64(&first.1, zoom);
                }
                for i in 0..stops.len() - 1 {
                    if input_val >= stops[i].0 && input_val < stops[i + 1].0 {
                        let lo = Self::eval_expr_f64(&stops[i].1, zoom)?;
                        let hi = Self::eval_expr_f64(&stops[i + 1].1, zoom)?;
                        let t =
                            Self::interp_t(interpolation, input_val, stops[i].0, stops[i + 1].0);
                        return Some(lo + t * (hi - lo));
                    }
                }
                None
            }
            Expression::Step {
                input,
                default,
                stops,
            } => {
                let input_val = Self::eval_expr_f64(input, zoom)?;
                let mut result = Self::eval_expr_f64(default, zoom)?;
                for (stop, val) in stops {
                    if input_val >= *stop {
                        result = Self::eval_expr_f64(val, zoom)?;
                    }
                }
                Some(result)
            }
            Expression::Add(a, b) => {
                Some(Self::eval_expr_f64(a, zoom)? + Self::eval_expr_f64(b, zoom)?)
            }
            Expression::Subtract(a, b) => {
                Some(Self::eval_expr_f64(a, zoom)? - Self::eval_expr_f64(b, zoom)?)
            }
            Expression::Multiply(a, b) => {
                Some(Self::eval_expr_f64(a, zoom)? * Self::eval_expr_f64(b, zoom)?)
            }
            Expression::Divide(a, b) => {
                let divisor = Self::eval_expr_f64(b, zoom)?;
                if divisor == 0.0 {
                    None
                } else {
                    Some(Self::eval_expr_f64(a, zoom)? / divisor)
                }
            }
            _ => None,
        }
    }

    /// Compute the interpolation parameter `t ∈ [0,1]` given an input value and stop range.
    fn interp_t(interp: &Interpolation, input: f64, lo: f64, hi: f64) -> f64 {
        let range = hi - lo;
        if range == 0.0 {
            return 0.0;
        }
        match interp {
            Interpolation::Linear => (input - lo) / range,
            Interpolation::Exponential(base) => {
                if (base - 1.0).abs() < f64::EPSILON {
                    (input - lo) / range
                } else {
                    (base.powf(input - lo) - 1.0) / (base.powf(range) - 1.0)
                }
            }
            Interpolation::CubicBezier(_) => {
                // Approximate with linear for now
                (input - lo) / range
            }
        }
    }

    /// Evaluate whether a set of feature properties matches the given [`Filter`].
    pub fn feature_matches_filter(
        filter: &Filter,
        properties: &HashMap<String, serde_json::Value>,
    ) -> bool {
        match filter {
            Filter::All(filters) => filters
                .iter()
                .all(|f| Self::feature_matches_filter(f, properties)),
            Filter::Any(filters) => filters
                .iter()
                .any(|f| Self::feature_matches_filter(f, properties)),
            Filter::None(filters) => !filters
                .iter()
                .any(|f| Self::feature_matches_filter(f, properties)),
            Filter::Eq { property, value } => properties.get(property.as_str()) == Some(value),
            Filter::Ne { property, value } => properties.get(property.as_str()) != Some(value),
            Filter::Lt { property, value } => properties
                .get(property.as_str())
                .and_then(|v| v.as_f64())
                .is_some_and(|v| v < *value),
            Filter::Lte { property, value } => properties
                .get(property.as_str())
                .and_then(|v| v.as_f64())
                .is_some_and(|v| v <= *value),
            Filter::Gt { property, value } => properties
                .get(property.as_str())
                .and_then(|v| v.as_f64())
                .is_some_and(|v| v > *value),
            Filter::Gte { property, value } => properties
                .get(property.as_str())
                .and_then(|v| v.as_f64())
                .is_some_and(|v| v >= *value),
            Filter::In { property, values } => properties
                .get(property.as_str())
                .is_some_and(|v| values.contains(v)),
            Filter::Has(property) => properties.contains_key(property.as_str()),
            Filter::NotHas(property) => !properties.contains_key(property.as_str()),
            Filter::GeometryType(_) => {
                // Geometry type matching requires the geometry itself, which is
                // not available from property maps alone; default to true to
                // remain non-blocking in property-only evaluation contexts.
                true
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

fn lerp_color(a: &Color, b: &Color, t: f64) -> Color {
    let lerp_u8 = |lo: u8, hi: u8| -> u8 {
        let v = f64::from(lo) + t * (f64::from(hi) - f64::from(lo));
        v.round() as u8
    };
    Color {
        r: lerp_u8(a.r, b.r),
        g: lerp_u8(a.g, b.g),
        b: lerp_u8(a.b, b.b),
        a: a.a + t as f32 * (b.a - a.a),
    }
}
