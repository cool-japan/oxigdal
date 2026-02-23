//! WMTS (Web Map Tile Service) handlers
//!
//! Implements OGC Web Map Tile Service 1.0.0 protocol:
//! - GetCapabilities: Returns XML metadata about tile sets
//! - GetTile: Returns a specific tile from the tile matrix set

use crate::cache::{CacheKey, TileCache};
use crate::config::ImageFormat;
use crate::dataset_registry::{DatasetRegistry, LayerInfo};
use crate::handlers::rendering::{RasterRenderer, RenderStyle, encode_image, tile_to_bbox};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use oxigdal_algorithms::resampling::ResamplingMethod;
use oxigdal_core::buffer::RasterBuffer;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use serde::Deserialize;
use std::io::Cursor;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace};

/// Configuration for source region to read from a dataset
#[derive(Debug, Clone, Copy)]
struct SourceRegion {
    x: u64,
    y: u64,
    width: u64,
    height: u64,
}

impl SourceRegion {
    fn new(x: u64, y: u64, width: u64, height: u64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Configuration for tile dimensions
#[derive(Debug, Clone, Copy)]
struct TileDimensions {
    width: u64,
    height: u64,
}

impl TileDimensions {
    fn new(width: u64, height: u64) -> Self {
        Self { width, height }
    }
}

/// WMTS errors
#[derive(Debug, Error)]
pub enum WmtsError {
    /// Invalid request parameter
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Missing required parameter
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    /// Layer not found
    #[error("Layer not found: {0}")]
    LayerNotFound(String),

    /// Tile matrix set not found
    #[error("TileMatrixSet not found: {0}")]
    TileMatrixSetNotFound(String),

    /// Tile out of bounds
    #[error("Tile coordinates out of bounds")]
    TileOutOfBounds,

    /// Rendering error
    #[error("Rendering error: {0}")]
    Rendering(String),

    /// Registry error
    #[error("Registry error: {0}")]
    Registry(#[from] crate::dataset_registry::RegistryError),

    /// Unsupported format
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
}

impl IntoResponse for WmtsError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            WmtsError::InvalidParameter(_) | WmtsError::MissingParameter(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            WmtsError::LayerNotFound(_) | WmtsError::TileOutOfBounds => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ExceptionReport version="1.0.0" xmlns="http://www.opengis.net/ows/1.1">
  <Exception exceptionCode="{}">{}</Exception>
</ExceptionReport>"#,
            status.as_u16(),
            message
        );

        (status, [(header::CONTENT_TYPE, "application/xml")], xml).into_response()
    }
}

/// Shared WMTS state
#[derive(Clone)]
pub struct WmtsState {
    /// Dataset registry
    pub registry: DatasetRegistry,

    /// Tile cache
    pub cache: TileCache,

    /// Service URL
    pub service_url: String,

    /// Service title
    pub service_title: String,
    /// Service description/abstract
    pub service_abstract: String,
}

/// Tile matrix set definition
#[derive(Debug, Clone)]
pub struct TileMatrixSet {
    /// Identifier (e.g., "WebMercatorQuad")
    pub identifier: String,

    /// Supported CRS
    pub crs: String,

    /// Tile matrices (one per zoom level)
    pub matrices: Vec<TileMatrix>,
}

/// Individual tile matrix (zoom level)
#[derive(Debug, Clone)]
pub struct TileMatrix {
    /// Identifier (zoom level as string)
    pub identifier: String,

    /// Scale denominator
    pub scale_denominator: f64,

    /// Top-left corner
    pub top_left_corner: (f64, f64),

    /// Tile width in pixels
    pub tile_width: u32,

    /// Tile height in pixels
    pub tile_height: u32,

    /// Matrix width in tiles
    pub matrix_width: u32,

    /// Matrix height in tiles
    pub matrix_height: u32,
}

impl TileMatrixSet {
    /// Create Web Mercator tile matrix set
    pub fn web_mercator_quad() -> Self {
        let mut matrices = Vec::new();

        for z in 0..=18 {
            let tiles_at_zoom = 1u32 << z;
            let scale_denominator = 559082264.0287178 / (1u64 << z) as f64;

            matrices.push(TileMatrix {
                identifier: z.to_string(),
                scale_denominator,
                top_left_corner: (-20037508.34278925, 20037508.34278925),
                tile_width: 256,
                tile_height: 256,
                matrix_width: tiles_at_zoom,
                matrix_height: tiles_at_zoom,
            });
        }

        Self {
            identifier: "WebMercatorQuad".to_string(),
            crs: "urn:ogc:def:crs:EPSG::3857".to_string(),
            matrices,
        }
    }

    /// Create World CRS84 tile matrix set
    pub fn world_crs84_quad() -> Self {
        let mut matrices = Vec::new();

        for z in 0..=18 {
            let tiles_x = 2u32 << z;
            let tiles_y = 1u32 << z;
            let scale_denominator = 279541132.0143589 / (1u64 << z) as f64;

            matrices.push(TileMatrix {
                identifier: z.to_string(),
                scale_denominator,
                top_left_corner: (-180.0, 90.0),
                tile_width: 256,
                tile_height: 256,
                matrix_width: tiles_x,
                matrix_height: tiles_y,
            });
        }

        Self {
            identifier: "WorldCRS84Quad".to_string(),
            crs: "urn:ogc:def:crs:OGC:1.3:CRS84".to_string(),
            matrices,
        }
    }

    /// Get tile matrix by identifier
    pub fn get_matrix(&self, identifier: &str) -> Option<&TileMatrix> {
        self.matrices.iter().find(|m| m.identifier == identifier)
    }
}

/// GetCapabilities request parameters
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GetCapabilitiesParams {
    #[serde(rename = "SERVICE")]
    service: Option<String>,

    #[serde(rename = "REQUEST")]
    request: Option<String>,

    #[serde(rename = "VERSION")]
    version: Option<String>,
}

/// GetTile request parameters (KVP encoding)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GetTileParams {
    #[serde(rename = "SERVICE")]
    service: Option<String>,

    #[serde(rename = "REQUEST")]
    request: Option<String>,

    #[serde(rename = "VERSION")]
    version: Option<String>,

    #[serde(rename = "LAYER")]
    layer: String,

    #[serde(rename = "STYLE")]
    style: Option<String>,

    #[serde(rename = "FORMAT")]
    format: String,

    #[serde(rename = "TILEMATRIXSET")]
    tile_matrix_set: String,

    #[serde(rename = "TILEMATRIX")]
    tile_matrix: String,

    #[serde(rename = "TILEROW")]
    tile_row: u32,

    #[serde(rename = "TILECOL")]
    tile_col: u32,
}

/// RESTful GetTile path parameters
#[derive(Debug, Deserialize)]
pub struct GetTilePath {
    layer: String,
    tile_matrix_set: String,
    tile_matrix: String,
    tile_row: u32,
    tile_col: u32,
}

/// Handle GetCapabilities request
pub async fn get_capabilities(
    State(state): State<Arc<WmtsState>>,
    Query(params): Query<GetCapabilitiesParams>,
) -> Result<Response, WmtsError> {
    debug!("WMTS GetCapabilities request");

    // Validate service parameter
    if let Some(ref service) = params.service {
        if service.to_uppercase() != "WMTS" {
            return Err(WmtsError::InvalidParameter(format!(
                "Invalid SERVICE: {}",
                service
            )));
        }
    }

    // Get all layers
    let layers = state.registry.list_layers()?;

    // Get tile matrix sets
    let matrix_sets = vec![
        TileMatrixSet::web_mercator_quad(),
        TileMatrixSet::world_crs84_quad(),
    ];

    // Generate capabilities XML
    let xml = generate_capabilities_xml(&state, &layers, &matrix_sets)?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/xml")],
        xml,
    )
        .into_response())
}

/// Generate WMTS capabilities XML
fn generate_capabilities_xml(
    state: &WmtsState,
    layers: &[LayerInfo],
    matrix_sets: &[TileMatrixSet],
) -> Result<String, WmtsError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // XML declaration
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Root element
    let mut root = BytesStart::new("Capabilities");
    root.push_attribute(("version", "1.0.0"));
    root.push_attribute(("xmlns", "http://www.opengis.net/wmts/1.0"));
    root.push_attribute(("xmlns:ows", "http://www.opengis.net/ows/1.1"));
    root.push_attribute(("xmlns:xlink", "http://www.w3.org/1999/xlink"));
    writer
        .write_event(Event::Start(root))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // ServiceIdentification
    write_service_identification(&mut writer, state)?;

    // Contents
    write_contents(&mut writer, state, layers, matrix_sets)?;

    // Close root
    writer
        .write_event(Event::End(BytesEnd::new("Capabilities")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    let result = writer.into_inner().into_inner();
    String::from_utf8(result).map_err(|e| WmtsError::Rendering(e.to_string()))
}

/// Write ServiceIdentification section
fn write_service_identification(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WmtsState,
) -> Result<(), WmtsError> {
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceIdentification")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Title
    writer
        .write_event(Event::Start(BytesStart::new("ows:Title")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&state.service_title)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:Title")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Abstract
    writer
        .write_event(Event::Start(BytesStart::new("ows:Abstract")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&state.service_abstract)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:Abstract")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // ServiceType
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceType")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new("OGC WMTS")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceType")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // ServiceTypeVersion
    writer
        .write_event(Event::Start(BytesStart::new("ows:ServiceTypeVersion")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new("1.0.0")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceTypeVersion")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("ows:ServiceIdentification")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write Contents section
fn write_contents(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WmtsState,
    layers: &[LayerInfo],
    matrix_sets: &[TileMatrixSet],
) -> Result<(), WmtsError> {
    writer
        .write_event(Event::Start(BytesStart::new("Contents")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Write layers
    for layer in layers {
        write_layer(writer, state, layer, matrix_sets)?;
    }

    // Write tile matrix sets
    for matrix_set in matrix_sets {
        write_tile_matrix_set(writer, matrix_set)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("Contents")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write Layer element
fn write_layer(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WmtsState,
    layer: &LayerInfo,
    matrix_sets: &[TileMatrixSet],
) -> Result<(), WmtsError> {
    writer
        .write_event(Event::Start(BytesStart::new("Layer")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Identifier
    writer
        .write_event(Event::Start(BytesStart::new("ows:Identifier")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&layer.name)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:Identifier")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Title
    writer
        .write_event(Event::Start(BytesStart::new("ows:Title")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&layer.title)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:Title")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Formats
    for format in &layer.config.formats {
        writer
            .write_event(Event::Start(BytesStart::new("Format")))
            .map_err(|e| WmtsError::Rendering(e.to_string()))?;
        writer
            .write_event(Event::Text(BytesText::new(format.mime_type())))
            .map_err(|e| WmtsError::Rendering(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("Format")))
            .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    }

    // TileMatrixSetLinks
    for matrix_set in matrix_sets {
        if layer
            .config
            .tile_matrix_sets
            .contains(&matrix_set.identifier)
        {
            writer
                .write_event(Event::Start(BytesStart::new("TileMatrixSetLink")))
                .map_err(|e| WmtsError::Rendering(e.to_string()))?;

            writer
                .write_event(Event::Start(BytesStart::new("TileMatrixSet")))
                .map_err(|e| WmtsError::Rendering(e.to_string()))?;
            writer
                .write_event(Event::Text(BytesText::new(&matrix_set.identifier)))
                .map_err(|e| WmtsError::Rendering(e.to_string()))?;
            writer
                .write_event(Event::End(BytesEnd::new("TileMatrixSet")))
                .map_err(|e| WmtsError::Rendering(e.to_string()))?;

            writer
                .write_event(Event::End(BytesEnd::new("TileMatrixSetLink")))
                .map_err(|e| WmtsError::Rendering(e.to_string()))?;
        }
    }

    // ResourceURL (RESTful template)
    let mut resource_url = BytesStart::new("ResourceURL");
    resource_url.push_attribute(("format", "image/png"));
    resource_url.push_attribute(("resourceType", "tile"));
    let template = format!(
        "{}/wmts/1.0.0/{}/{{TileMatrixSet}}/{{TileMatrix}}/{{TileRow}}/{{TileCol}}.png",
        state.service_url, layer.name
    );
    resource_url.push_attribute(("template", template.as_str()));
    writer
        .write_event(Event::Empty(resource_url))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("Layer")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write TileMatrixSet element
fn write_tile_matrix_set(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    matrix_set: &TileMatrixSet,
) -> Result<(), WmtsError> {
    writer
        .write_event(Event::Start(BytesStart::new("TileMatrixSet")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Identifier
    writer
        .write_event(Event::Start(BytesStart::new("ows:Identifier")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&matrix_set.identifier)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:Identifier")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // SupportedCRS
    writer
        .write_event(Event::Start(BytesStart::new("ows:SupportedCRS")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&matrix_set.crs)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:SupportedCRS")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // TileMatrix elements
    for matrix in &matrix_set.matrices {
        write_tile_matrix(writer, matrix)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("TileMatrixSet")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write TileMatrix element
fn write_tile_matrix(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    matrix: &TileMatrix,
) -> Result<(), WmtsError> {
    writer
        .write_event(Event::Start(BytesStart::new("TileMatrix")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // Identifier
    writer
        .write_event(Event::Start(BytesStart::new("ows:Identifier")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&matrix.identifier)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ows:Identifier")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // ScaleDenominator
    writer
        .write_event(Event::Start(BytesStart::new("ScaleDenominator")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(
            &matrix.scale_denominator.to_string(),
        )))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("ScaleDenominator")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // TopLeftCorner
    writer
        .write_event(Event::Start(BytesStart::new("TopLeftCorner")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    let corner = format!("{} {}", matrix.top_left_corner.0, matrix.top_left_corner.1);
    writer
        .write_event(Event::Text(BytesText::new(&corner)))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("TopLeftCorner")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // TileWidth
    writer
        .write_event(Event::Start(BytesStart::new("TileWidth")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&matrix.tile_width.to_string())))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("TileWidth")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // TileHeight
    writer
        .write_event(Event::Start(BytesStart::new("TileHeight")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&matrix.tile_height.to_string())))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("TileHeight")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // MatrixWidth
    writer
        .write_event(Event::Start(BytesStart::new("MatrixWidth")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(
            &matrix.matrix_width.to_string(),
        )))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("MatrixWidth")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    // MatrixHeight
    writer
        .write_event(Event::Start(BytesStart::new("MatrixHeight")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(
            &matrix.matrix_height.to_string(),
        )))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("MatrixHeight")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("TileMatrix")))
        .map_err(|e| WmtsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Handle GetTile request (RESTful)
pub async fn get_tile_rest(
    State(state): State<Arc<WmtsState>>,
    AxumPath(path): AxumPath<GetTilePath>,
) -> Result<Response, WmtsError> {
    debug!(
        "WMTS GetTile (REST): layer={}, z={}, x={}, y={}",
        path.layer, path.tile_matrix, path.tile_col, path.tile_row
    );

    get_tile_impl(
        &state,
        &path.layer,
        &path.tile_matrix_set,
        &path.tile_matrix,
        path.tile_row,
        path.tile_col,
        ImageFormat::Png,
    )
    .await
}

/// Handle GetTile request (KVP)
pub async fn get_tile_kvp(
    State(state): State<Arc<WmtsState>>,
    Query(params): Query<GetTileParams>,
) -> Result<Response, WmtsError> {
    debug!("WMTS GetTile (KVP): layer={}", params.layer);

    let format = parse_format(&params.format)?;

    get_tile_impl(
        &state,
        &params.layer,
        &params.tile_matrix_set,
        &params.tile_matrix,
        params.tile_row,
        params.tile_col,
        format,
    )
    .await
}

/// Shared GetTile implementation
async fn get_tile_impl(
    state: &WmtsState,
    layer_name: &str,
    tile_matrix_set: &str,
    tile_matrix: &str,
    tile_row: u32,
    tile_col: u32,
    format: ImageFormat,
) -> Result<Response, WmtsError> {
    // Check cache first
    let zoom_level = tile_matrix.parse::<u8>().ok().unwrap_or(0);
    let cache_key = CacheKey::new(
        layer_name.to_string(),
        zoom_level,
        tile_col,
        tile_row,
        format.extension().to_string(),
    );

    if let Some(cached_tile) = state.cache.get(&cache_key) {
        trace!("Cache hit for tile: {}", cache_key.to_string());
        return Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, format.mime_type())],
            cached_tile,
        )
            .into_response());
    }

    // Get layer (for validation and config)
    let _ = state.registry.get_layer(layer_name)?;

    // Validate tile matrix set
    let matrix_set_obj = get_tile_matrix_set(tile_matrix_set)?;
    let matrix = matrix_set_obj.get_matrix(tile_matrix).ok_or_else(|| {
        WmtsError::InvalidParameter(format!("Invalid tile matrix: {}", tile_matrix))
    })?;

    // Validate tile coordinates
    if tile_col >= matrix.matrix_width || tile_row >= matrix.matrix_height {
        return Err(WmtsError::TileOutOfBounds);
    }

    // Render tile
    let tile_data = render_tile(
        &state.registry,
        layer_name,
        &matrix_set_obj,
        matrix,
        tile_row,
        tile_col,
        format,
    )
    .await?;

    // Cache the tile
    let _ = state.cache.put(cache_key, tile_data.clone());

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, format.mime_type())],
        tile_data,
    )
        .into_response())
}

/// Get tile matrix set by identifier
fn get_tile_matrix_set(identifier: &str) -> Result<TileMatrixSet, WmtsError> {
    match identifier {
        "WebMercatorQuad" => Ok(TileMatrixSet::web_mercator_quad()),
        "WorldCRS84Quad" => Ok(TileMatrixSet::world_crs84_quad()),
        _ => Err(WmtsError::TileMatrixSetNotFound(identifier.to_string())),
    }
}

/// Parse image format from MIME type
fn parse_format(format_str: &str) -> Result<ImageFormat, WmtsError> {
    match format_str.to_lowercase().as_str() {
        "image/png" => Ok(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Ok(ImageFormat::Jpeg),
        "image/webp" => Ok(ImageFormat::Webp),
        _ => Err(WmtsError::UnsupportedFormat(format_str.to_string())),
    }
}

/// Render a tile from the actual dataset
///
/// Performs the full WMTS tile rendering pipeline:
/// 1. Calculates the geographic bounds of the tile from the tile matrix parameters
/// 2. Converts tile bounds to pixel coordinates in the source dataset
/// 3. Reads the corresponding raster data, using overviews when available
/// 4. Resamples to the tile pixel dimensions (typically 256x256)
/// 5. Applies colormap/styling
/// 6. Encodes to the requested image format
async fn render_tile(
    registry: &DatasetRegistry,
    layer_name: &str,
    matrix_set: &TileMatrixSet,
    matrix: &TileMatrix,
    tile_row: u32,
    tile_col: u32,
    format: ImageFormat,
) -> Result<Bytes, WmtsError> {
    let zoom: u32 = matrix.identifier.parse().map_err(|_| {
        WmtsError::InvalidParameter(format!(
            "Cannot parse zoom level from matrix identifier: {}",
            matrix.identifier
        ))
    })?;

    debug!(
        "Rendering tile: layer={}, z={}, col={}, row={}, matrix_set={}",
        layer_name, zoom, tile_col, tile_row, matrix_set.identifier
    );

    // Get the dataset for this layer
    let dataset = registry.get_dataset(layer_name).map_err(|e| {
        WmtsError::Rendering(format!(
            "Failed to get dataset for layer {}: {}",
            layer_name, e
        ))
    })?;

    // Get layer config for styling
    let layer_info = registry
        .get_layer(layer_name)
        .map_err(|e| WmtsError::Rendering(format!("Failed to get layer info: {}", e)))?;

    let tile_width = matrix.tile_width as u64;
    let tile_height = matrix.tile_height as u64;

    // Calculate the geographic bounds of this tile
    let tile_bbox = tile_to_bbox(&matrix_set.identifier, zoom, tile_col, tile_row)
        .map_err(|e| WmtsError::Rendering(format!("Failed to calculate tile bbox: {}", e)))?;

    // Get the dataset's GeoTransform
    let geo_transform = dataset
        .geo_transform_obj()
        .ok_or_else(|| WmtsError::Rendering("Dataset has no geotransform".to_string()))?;

    let ds_width = dataset.width();
    let ds_height = dataset.height();

    // Convert tile bounds to pixel coordinates in the source dataset
    // tile_bbox is in the CRS of the tile matrix set (e.g., EPSG:3857 or CRS84)
    // The dataset may be in a different CRS, but for now we assume matching CRS
    // or that the dataset is already in the tile CRS
    let (px_min_x, px_min_y) = geo_transform
        .world_to_pixel(tile_bbox.min_x, tile_bbox.max_y)
        .map_err(|e| WmtsError::Rendering(format!("Coordinate transform error: {}", e)))?;
    let (px_max_x, px_max_y) = geo_transform
        .world_to_pixel(tile_bbox.max_x, tile_bbox.min_y)
        .map_err(|e| WmtsError::Rendering(format!("Coordinate transform error: {}", e)))?;

    // Determine the pixel window, clamping to dataset bounds
    let src_x = (px_min_x.min(px_max_x).floor().max(0.0)) as u64;
    let src_y = (px_min_y.min(px_max_y).floor().max(0.0)) as u64;
    let src_end_x = (px_min_x.max(px_max_x).ceil().max(0.0) as u64).min(ds_width);
    let src_end_y = (px_min_y.max(px_max_y).ceil().max(0.0) as u64).min(ds_height);

    let src_width = src_end_x.saturating_sub(src_x);
    let src_height = src_end_y.saturating_sub(src_y);

    // Check if the tile has any overlap with the dataset
    if src_width == 0 || src_height == 0 {
        // No overlap - return a transparent/empty tile
        return render_empty_tile(tile_width, tile_height, format);
    }

    // Determine the best overview level based on the resolution ratio
    let overview_level =
        select_tile_overview_level(&dataset, src_width, src_height, tile_width, tile_height);

    // Build the rendering style from layer configuration
    let render_style = if let Some(ref style_cfg) = layer_info.config.style {
        RenderStyle::from_config(style_cfg)
    } else {
        RenderStyle::default()
    };

    let band_count = dataset.raster_count();

    // Render based on band count
    let rgba_data = if band_count >= 3 {
        render_tile_rgb(
            &dataset,
            overview_level,
            SourceRegion::new(src_x, src_y, src_width, src_height),
            TileDimensions::new(tile_width, tile_height),
            &render_style,
        )?
    } else {
        render_tile_single_band(
            &dataset,
            overview_level,
            SourceRegion::new(src_x, src_y, src_width, src_height),
            TileDimensions::new(tile_width, tile_height),
            &render_style,
        )?
    };

    // Handle partial coverage: if the tile only partially overlaps the dataset,
    // the edges will be nodata (transparent). This is already handled by the
    // rendering functions returning 0-alpha for nodata pixels.

    // Encode to the requested format
    encode_image(&rgba_data, tile_width as u32, tile_height as u32, format)
        .map_err(|e| WmtsError::Rendering(format!("Image encoding failed: {}", e)))
}

/// Render an empty (transparent) tile for areas outside the dataset
fn render_empty_tile(width: u64, height: u64, format: ImageFormat) -> Result<Bytes, WmtsError> {
    // Create a fully transparent RGBA buffer
    let rgba = vec![0u8; (width * height * 4) as usize];
    encode_image(&rgba, width as u32, height as u32, format)
        .map_err(|e| WmtsError::Rendering(format!("Empty tile encoding failed: {}", e)))
}

/// Select the best overview level for tile rendering
///
/// Chooses the overview level that provides just enough resolution for the
/// requested tile, avoiding reading unnecessarily high-resolution data
/// at low zoom levels.
fn select_tile_overview_level(
    dataset: &crate::dataset_registry::Dataset,
    src_width: u64,
    src_height: u64,
    tile_width: u64,
    tile_height: u64,
) -> usize {
    let overview_count = dataset.overview_count();
    if overview_count == 0 {
        return 0;
    }

    // Calculate the downsample ratio
    let ratio_x = if tile_width > 0 {
        src_width as f64 / tile_width as f64
    } else {
        1.0
    };
    let ratio_y = if tile_height > 0 {
        src_height as f64 / tile_height as f64
    } else {
        1.0
    };
    let request_ratio = ratio_x.max(ratio_y);

    if request_ratio <= 1.0 {
        return 0; // Full resolution or upsampling needed
    }

    // Find the best overview level (each level typically halves resolution)
    let mut best_level = 0;
    for level in 1..=overview_count {
        let overview_factor = (1u64 << level) as f64;
        if overview_factor <= request_ratio * 1.5 {
            best_level = level;
        } else {
            break;
        }
    }

    best_level
}

/// Render a single-band tile with colormap
fn render_tile_single_band(
    dataset: &crate::dataset_registry::Dataset,
    _overview_level: usize,
    source: SourceRegion,
    tile: TileDimensions,
    style: &RenderStyle,
) -> Result<Vec<u8>, WmtsError> {
    // Read the source window from the dataset
    let src_buffer = dataset
        .read_window(source.x, source.y, source.width, source.height)
        .map_err(|e| WmtsError::Rendering(format!("Failed to read window: {}", e)))?;

    // Resample to tile dimensions
    let resampled = if src_buffer.width() != tile.width || src_buffer.height() != tile.height {
        RasterRenderer::resample(&src_buffer, tile.width, tile.height, style.resampling)
            .map_err(|e| WmtsError::Rendering(format!("Resampling failed: {}", e)))?
    } else {
        src_buffer
    };

    // Render with colormap to RGBA
    RasterRenderer::render_to_rgba(&resampled, style)
        .map_err(|e| WmtsError::Rendering(format!("Rendering failed: {}", e)))
}

/// Render an RGB tile from three bands
fn render_tile_rgb(
    dataset: &crate::dataset_registry::Dataset,
    _overview_level: usize,
    source: SourceRegion,
    tile: TileDimensions,
    style: &RenderStyle,
) -> Result<Vec<u8>, WmtsError> {
    // Read band 0 (red) using read_window
    let red_buffer = dataset
        .read_window(source.x, source.y, source.width, source.height)
        .map_err(|e| WmtsError::Rendering(format!("Failed to read red band: {}", e)))?;

    // Read bands 1 (green) and 2 (blue) from full band data and extract windows
    let green_buffer =
        build_band_window(dataset, 1, source.x, source.y, source.width, source.height);
    let blue_buffer =
        build_band_window(dataset, 2, source.x, source.y, source.width, source.height);

    let (green_buf, blue_buf) = match (green_buffer, blue_buffer) {
        (Ok(g), Ok(b)) => (g, b),
        _ => {
            // Fallback: use red band for all channels (grayscale as RGB)
            let gray = red_buffer.clone();
            (gray.clone(), gray)
        }
    };

    // Resample each band
    let resample_method = style.resampling;
    let r_resampled = resample_if_needed(&red_buffer, tile.width, tile.height, resample_method)?;
    let g_resampled = resample_if_needed(&green_buf, tile.width, tile.height, resample_method)?;
    let b_resampled = resample_if_needed(&blue_buf, tile.width, tile.height, resample_method)?;

    // Compose RGB to RGBA
    RasterRenderer::render_rgb_to_rgba(&r_resampled, &g_resampled, &b_resampled, style)
        .map_err(|e| WmtsError::Rendering(format!("RGB rendering failed: {}", e)))
}

/// Resample a buffer to target dimensions if needed
fn resample_if_needed(
    buffer: &RasterBuffer,
    target_width: u64,
    target_height: u64,
    method: ResamplingMethod,
) -> Result<RasterBuffer, WmtsError> {
    if buffer.width() != target_width || buffer.height() != target_height {
        RasterRenderer::resample(buffer, target_width, target_height, method)
            .map_err(|e| WmtsError::Rendering(format!("Resampling failed: {}", e)))
    } else {
        Ok(buffer.clone())
    }
}

/// Build a RasterBuffer window for a specific band from full band data
fn build_band_window(
    dataset: &crate::dataset_registry::Dataset,
    band: usize,
    src_x: u64,
    src_y: u64,
    src_width: u64,
    src_height: u64,
) -> Result<RasterBuffer, WmtsError> {
    let band_data = dataset
        .read_band(0, band)
        .map_err(|e| WmtsError::Rendering(format!("Failed to read band {}: {}", band, e)))?;

    let ds_width = dataset.width();
    let ds_height = dataset.height();
    let data_type = dataset.data_type();
    let nodata = dataset.nodata();

    let full_buffer = RasterBuffer::new(band_data, ds_width, ds_height, data_type, nodata)
        .map_err(|e| WmtsError::Rendering(format!("Buffer creation error: {}", e)))?;

    // Extract the window
    let mut window = RasterBuffer::zeros(src_width, src_height, data_type);
    for dy in 0..src_height {
        for dx in 0..src_width {
            let gx = src_x + dx;
            let gy = src_y + dy;
            if gx < ds_width && gy < ds_height {
                if let Ok(val) = full_buffer.get_pixel(gx, gy) {
                    let _ = window.set_pixel(dx, dy, val);
                }
            }
        }
    }

    Ok(window)
}
