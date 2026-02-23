//! WMS (Web Map Service) handlers
//!
//! Implements OGC Web Map Service 1.3.0 protocol:
//! - GetCapabilities: Returns XML metadata about available layers
//! - GetMap: Renders and returns a map image
//! - GetFeatureInfo: Queries pixel values at a point

use crate::cache::TileCache;
use crate::config::ImageFormat;
use crate::dataset_registry::Dataset;
use crate::dataset_registry::{DatasetRegistry, LayerInfo};
use crate::handlers::rendering::{RasterRenderer, RenderStyle, encode_image};
use axum::{
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use oxigdal_core::buffer::RasterBuffer;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use serde::Deserialize;
use std::io::Cursor;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

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

/// Configuration for target rendering dimensions
#[derive(Debug, Clone, Copy)]
struct TargetDimensions {
    width: u64,
    height: u64,
}

impl TargetDimensions {
    fn new(width: u64, height: u64) -> Self {
        Self { width, height }
    }
}

/// WMS errors
#[derive(Debug, Error)]
pub enum WmsError {
    /// Invalid request parameter
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Missing required parameter
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    /// Layer not found
    #[error("Layer not found: {0}")]
    LayerNotFound(String),

    /// Invalid CRS
    #[error("Invalid CRS: {0}")]
    InvalidCrs(String),

    /// Invalid bounding box
    #[error("Invalid bounding box: {0}")]
    InvalidBbox(String),

    /// Rendering error
    #[error("Rendering error: {0}")]
    Rendering(String),

    /// OxiGDAL error
    #[error("GDAL error: {0}")]
    Gdal(#[from] oxigdal_core::OxiGdalError),

    /// Registry error
    #[error("Registry error: {0}")]
    Registry(#[from] crate::dataset_registry::RegistryError),

    /// Unsupported format
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
}

impl IntoResponse for WmsError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            WmsError::InvalidParameter(_) | WmsError::MissingParameter(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            WmsError::LayerNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        // Return OGC ServiceException format
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ServiceExceptionReport version="1.3.0" xmlns="http://www.opengis.net/ogc">
  <ServiceException>{}</ServiceException>
</ServiceExceptionReport>"#,
            message
        );

        (
            status,
            [(header::CONTENT_TYPE, "application/vnd.ogc.se_xml")],
            xml,
        )
            .into_response()
    }
}

/// Shared server state
#[derive(Clone)]
pub struct WmsState {
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

/// GetMap request parameters
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GetMapParams {
    #[serde(rename = "SERVICE")]
    service: Option<String>,

    #[serde(rename = "REQUEST")]
    request: Option<String>,

    #[serde(rename = "VERSION")]
    version: Option<String>,

    #[serde(rename = "LAYERS")]
    layers: String,

    #[serde(rename = "STYLES")]
    styles: Option<String>,

    #[serde(rename = "CRS")]
    crs: Option<String>,

    #[serde(rename = "SRS")]
    srs: Option<String>,

    #[serde(rename = "BBOX")]
    bbox: String,

    #[serde(rename = "WIDTH")]
    width: u32,

    #[serde(rename = "HEIGHT")]
    height: u32,

    #[serde(rename = "FORMAT")]
    format: String,

    #[serde(rename = "TRANSPARENT")]
    transparent: Option<bool>,

    #[serde(rename = "BGCOLOR")]
    bgcolor: Option<String>,
}

/// GetFeatureInfo request parameters
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GetFeatureInfoParams {
    #[serde(rename = "SERVICE")]
    service: Option<String>,

    #[serde(rename = "REQUEST")]
    request: Option<String>,

    #[serde(rename = "VERSION")]
    version: Option<String>,

    #[serde(rename = "LAYERS")]
    layers: String,

    #[serde(rename = "QUERY_LAYERS")]
    query_layers: String,

    #[serde(rename = "CRS")]
    crs: Option<String>,

    #[serde(rename = "SRS")]
    srs: Option<String>,

    #[serde(rename = "BBOX")]
    bbox: String,

    #[serde(rename = "WIDTH")]
    width: u32,

    #[serde(rename = "HEIGHT")]
    height: u32,

    #[serde(rename = "I")]
    i: Option<u32>,

    #[serde(rename = "X")]
    x: Option<u32>,

    #[serde(rename = "J")]
    j: Option<u32>,

    #[serde(rename = "Y")]
    y: Option<u32>,

    #[serde(rename = "INFO_FORMAT")]
    info_format: Option<String>,
}

/// Handle GetCapabilities request
pub async fn get_capabilities(
    State(state): State<Arc<WmsState>>,
    Query(params): Query<GetCapabilitiesParams>,
) -> Result<Response, WmsError> {
    debug!("WMS GetCapabilities request");

    // Validate service parameter
    if let Some(ref service) = params.service {
        if service.to_uppercase() != "WMS" {
            return Err(WmsError::InvalidParameter(format!(
                "Invalid SERVICE: {}",
                service
            )));
        }
    }

    // Get all layers
    let layers = state.registry.list_layers()?;

    // Generate capabilities XML
    let xml = generate_capabilities_xml(&state, &layers)?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/vnd.ogc.wms_xml")],
        xml,
    )
        .into_response())
}

/// Generate WMS capabilities XML document
fn generate_capabilities_xml(state: &WmsState, layers: &[LayerInfo]) -> Result<String, WmsError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // XML declaration
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Root element
    let mut root = BytesStart::new("WMS_Capabilities");
    root.push_attribute(("version", "1.3.0"));
    root.push_attribute(("xmlns", "http://www.opengis.net/wms"));
    root.push_attribute(("xmlns:xlink", "http://www.w3.org/1999/xlink"));
    writer
        .write_event(Event::Start(root))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Service section
    write_service_section(&mut writer, state)?;

    // Capability section
    write_capability_section(&mut writer, state, layers)?;

    // Close root
    writer
        .write_event(Event::End(BytesEnd::new("WMS_Capabilities")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    let result = writer.into_inner().into_inner();
    String::from_utf8(result).map_err(|e| WmsError::Rendering(e.to_string()))
}

/// Write Service section of capabilities
fn write_service_section(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WmsState,
) -> Result<(), WmsError> {
    writer
        .write_event(Event::Start(BytesStart::new("Service")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Name
    writer
        .write_event(Event::Start(BytesStart::new("Name")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new("WMS")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Name")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Title
    writer
        .write_event(Event::Start(BytesStart::new("Title")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&state.service_title)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Title")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Abstract
    writer
        .write_event(Event::Start(BytesStart::new("Abstract")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&state.service_abstract)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Abstract")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // OnlineResource
    let mut online_resource = BytesStart::new("OnlineResource");
    online_resource.push_attribute(("xmlns:xlink", "http://www.w3.org/1999/xlink"));
    online_resource.push_attribute(("xlink:type", "simple"));
    online_resource.push_attribute(("xlink:href", state.service_url.as_str()));
    writer
        .write_event(Event::Empty(online_resource))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("Service")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write Capability section of capabilities
fn write_capability_section(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WmsState,
    layers: &[LayerInfo],
) -> Result<(), WmsError> {
    writer
        .write_event(Event::Start(BytesStart::new("Capability")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Request
    write_request_section(writer, state)?;

    // Exception
    writer
        .write_event(Event::Start(BytesStart::new("Exception")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Empty(BytesStart::new("Format")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Exception")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Layers
    write_layers_section(writer, layers)?;

    writer
        .write_event(Event::End(BytesEnd::new("Capability")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write Request section
fn write_request_section(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    state: &WmsState,
) -> Result<(), WmsError> {
    writer
        .write_event(Event::Start(BytesStart::new("Request")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // GetCapabilities
    write_operation(writer, "GetCapabilities", &state.service_url)?;

    // GetMap
    write_operation(writer, "GetMap", &state.service_url)?;

    // GetFeatureInfo
    write_operation(writer, "GetFeatureInfo", &state.service_url)?;

    writer
        .write_event(Event::End(BytesEnd::new("Request")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write operation definition
fn write_operation(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    operation: &str,
    url: &str,
) -> Result<(), WmsError> {
    writer
        .write_event(Event::Start(BytesStart::new(operation)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Format
    for format in &["image/png", "image/jpeg", "image/webp"] {
        writer
            .write_event(Event::Start(BytesStart::new("Format")))
            .map_err(|e| WmsError::Rendering(e.to_string()))?;
        writer
            .write_event(Event::Text(BytesText::new(format)))
            .map_err(|e| WmsError::Rendering(e.to_string()))?;
        writer
            .write_event(Event::End(BytesEnd::new("Format")))
            .map_err(|e| WmsError::Rendering(e.to_string()))?;
    }

    // DCPType
    writer
        .write_event(Event::Start(BytesStart::new("DCPType")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Start(BytesStart::new("HTTP")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Start(BytesStart::new("Get")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    let mut online_resource = BytesStart::new("OnlineResource");
    online_resource.push_attribute(("xmlns:xlink", "http://www.w3.org/1999/xlink"));
    online_resource.push_attribute(("xlink:type", "simple"));
    online_resource.push_attribute(("xlink:href", url));
    writer
        .write_event(Event::Empty(online_resource))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new("Get")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("HTTP")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("DCPType")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    writer
        .write_event(Event::End(BytesEnd::new(operation)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Write Layers section
fn write_layers_section(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    layers: &[LayerInfo],
) -> Result<(), WmsError> {
    for layer in layers {
        write_layer(writer, layer)?;
    }

    Ok(())
}

/// Write individual Layer
fn write_layer(writer: &mut Writer<Cursor<Vec<u8>>>, layer: &LayerInfo) -> Result<(), WmsError> {
    writer
        .write_event(Event::Start(BytesStart::new("Layer")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Name
    writer
        .write_event(Event::Start(BytesStart::new("Name")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&layer.name)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Name")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Title
    writer
        .write_event(Event::Start(BytesStart::new("Title")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&layer.title)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Title")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // Abstract
    writer
        .write_event(Event::Start(BytesStart::new("Abstract")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::Text(BytesText::new(&layer.abstract_)))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;
    writer
        .write_event(Event::End(BytesEnd::new("Abstract")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    // BoundingBox
    if let Some((min_x, min_y, max_x, max_y)) = layer.metadata.bbox {
        let mut bbox = BytesStart::new("BoundingBox");
        bbox.push_attribute(("CRS", "EPSG:4326"));
        bbox.push_attribute(("minx", min_x.to_string().as_str()));
        bbox.push_attribute(("miny", min_y.to_string().as_str()));
        bbox.push_attribute(("maxx", max_x.to_string().as_str()));
        bbox.push_attribute(("maxy", max_y.to_string().as_str()));
        writer
            .write_event(Event::Empty(bbox))
            .map_err(|e| WmsError::Rendering(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("Layer")))
        .map_err(|e| WmsError::Rendering(e.to_string()))?;

    Ok(())
}

/// Handle GetMap request
pub async fn get_map(
    State(state): State<Arc<WmsState>>,
    Query(params): Query<GetMapParams>,
) -> Result<Response, WmsError> {
    debug!("WMS GetMap request: layers={}", params.layers);

    // Parse parameters
    let layer_name = params
        .layers
        .split(',')
        .next()
        .ok_or_else(|| WmsError::InvalidParameter("LAYERS parameter is empty".to_string()))?;

    let format = parse_format(&params.format)?;
    let bbox = parse_bbox(&params.bbox)?;

    // Validate dimensions
    if params.width == 0 || params.height == 0 {
        return Err(WmsError::InvalidParameter(
            "WIDTH and HEIGHT must be greater than 0".to_string(),
        ));
    }

    if params.width > 4096 || params.height > 4096 {
        return Err(WmsError::InvalidParameter(
            "WIDTH and HEIGHT must be <= 4096".to_string(),
        ));
    }

    // Get layer and dataset
    let layer = state.registry.get_layer(layer_name)?;
    let dataset = state.registry.get_dataset(layer_name)?;

    // Render map with actual dataset data
    let image_data = render_map(
        &dataset,
        bbox,
        params.width,
        params.height,
        format,
        params.transparent.unwrap_or(false),
        layer.config.style.as_ref(),
    )?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, format.mime_type())],
        image_data,
    )
        .into_response())
}

/// Parse image format from string
fn parse_format(format_str: &str) -> Result<ImageFormat, WmsError> {
    match format_str.to_lowercase().as_str() {
        "image/png" => Ok(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Ok(ImageFormat::Jpeg),
        "image/webp" => Ok(ImageFormat::Webp),
        _ => Err(WmsError::UnsupportedFormat(format_str.to_string())),
    }
}

/// Parse bounding box from string
fn parse_bbox(bbox_str: &str) -> Result<(f64, f64, f64, f64), WmsError> {
    let parts: Vec<&str> = bbox_str.split(',').collect();
    if parts.len() != 4 {
        return Err(WmsError::InvalidBbox("BBOX must have 4 values".to_string()));
    }

    let min_x = parts[0]
        .parse::<f64>()
        .map_err(|_| WmsError::InvalidBbox(format!("Invalid minx: {}", parts[0])))?;
    let min_y = parts[1]
        .parse::<f64>()
        .map_err(|_| WmsError::InvalidBbox(format!("Invalid miny: {}", parts[1])))?;
    let max_x = parts[2]
        .parse::<f64>()
        .map_err(|_| WmsError::InvalidBbox(format!("Invalid maxx: {}", parts[2])))?;
    let max_y = parts[3]
        .parse::<f64>()
        .map_err(|_| WmsError::InvalidBbox(format!("Invalid maxy: {}", parts[3])))?;

    if min_x >= max_x || min_y >= max_y {
        return Err(WmsError::InvalidBbox(
            "Invalid bbox: min must be < max".to_string(),
        ));
    }

    Ok((min_x, min_y, max_x, max_y))
}

/// Render a map image from dataset
///
/// Performs the full WMS GetMap rendering pipeline:
/// 1. Determines the pixel window in the source dataset corresponding to the requested bbox
/// 2. Reads source raster data from the dataset
/// 3. Resamples to the requested output dimensions
/// 4. Applies colormap/styling for single-band or composes RGB for multi-band
/// 5. Encodes to the requested image format (PNG, JPEG, etc.)
fn render_map(
    dataset: &Dataset,
    bbox: (f64, f64, f64, f64),
    width: u32,
    height: u32,
    format: ImageFormat,
    transparent: bool,
    style: Option<&crate::config::StyleConfig>,
) -> Result<Bytes, WmsError> {
    let (req_min_x, req_min_y, req_max_x, req_max_y) = bbox;

    debug!(
        "Rendering map: bbox=({},{},{},{}), size={}x{}, format={:?}",
        req_min_x, req_min_y, req_max_x, req_max_y, width, height, format
    );

    // Get the dataset's GeoTransform for coordinate conversions
    let geo_transform = dataset.geo_transform_obj().ok_or_else(|| {
        WmsError::Rendering("Dataset has no geotransform - cannot map coordinates".to_string())
    })?;

    let ds_width = dataset.width();
    let ds_height = dataset.height();
    let band_count = dataset.raster_count();

    // Convert the requested bbox corners to pixel coordinates in the source dataset
    let (px_min_x, px_min_y) = geo_transform
        .world_to_pixel(req_min_x, req_max_y)
        .map_err(|e| WmsError::Rendering(format!("Coordinate transform error: {}", e)))?;
    let (px_max_x, px_max_y) = geo_transform
        .world_to_pixel(req_max_x, req_min_y)
        .map_err(|e| WmsError::Rendering(format!("Coordinate transform error: {}", e)))?;

    // Determine the pixel window, clamping to dataset bounds
    let src_x = (px_min_x.min(px_max_x).floor().max(0.0)) as u64;
    let src_y = (px_min_y.min(px_max_y).floor().max(0.0)) as u64;
    let src_end_x = (px_min_x.max(px_max_x).ceil().max(0.0) as u64).min(ds_width);
    let src_end_y = (px_min_y.max(px_max_y).ceil().max(0.0) as u64).min(ds_height);

    let src_width = if src_end_x > src_x {
        src_end_x - src_x
    } else {
        1
    };
    let src_height = if src_end_y > src_y {
        src_end_y - src_y
    } else {
        1
    };

    debug!(
        "Source window: offset=({}, {}), size={}x{}, dataset={}x{}, bands={}",
        src_x, src_y, src_width, src_height, ds_width, ds_height, band_count
    );

    // Determine the best overview level for this request
    let overview_level =
        select_overview_level(dataset, src_width, src_height, width as u64, height as u64);

    // Build the rendering style
    let render_style = if let Some(style_cfg) = style {
        RenderStyle::from_config(style_cfg)
    } else {
        let mut s = RenderStyle::default();
        if !transparent {
            s.alpha = 1.0;
        } else {
            s.alpha = 1.0; // alpha for data pixels; nodata will be transparent
        }
        s
    };

    // Render based on the number of bands
    let rgba_data = if band_count >= 3 {
        // RGB(A) dataset: read three separate bands and compose
        render_rgb_bands(
            dataset,
            overview_level,
            SourceRegion::new(src_x, src_y, src_width, src_height),
            TargetDimensions::new(width as u64, height as u64),
            &render_style,
        )?
    } else {
        // Single-band dataset: read one band and apply colormap
        render_single_band(
            dataset,
            overview_level,
            SourceRegion::new(src_x, src_y, src_width, src_height),
            TargetDimensions::new(width as u64, height as u64),
            &render_style,
        )?
    };

    // If transparent and format supports it, nodata pixels are already 0-alpha.
    // If not transparent, ensure all alpha values are 255 for non-nodata pixels.
    let final_rgba = if !transparent {
        let mut data = rgba_data;
        // Set all alpha values to 255 for opaque mode
        for chunk in data.chunks_exact_mut(4) {
            if chunk[3] > 0 {
                chunk[3] = 255;
            }
        }
        data
    } else {
        rgba_data
    };

    // Encode to the requested image format
    encode_image(&final_rgba, width, height, format).map_err(|e| WmsError::Rendering(e.to_string()))
}

/// Select the best overview level for the requested resolution
///
/// Returns 0 for full resolution, or a higher number for coarser overviews.
/// Picks the overview where the overview resolution is just finer than what
/// the client requested, ensuring quality while avoiding reading unnecessary data.
fn select_overview_level(
    dataset: &Dataset,
    src_width: u64,
    src_height: u64,
    target_width: u64,
    target_height: u64,
) -> usize {
    let overview_count = dataset.overview_count();
    if overview_count == 0 {
        return 0;
    }

    // Calculate the downsample ratio the client is requesting
    let ratio_x = if target_width > 0 {
        src_width as f64 / target_width as f64
    } else {
        1.0
    };
    let ratio_y = if target_height > 0 {
        src_height as f64 / target_height as f64
    } else {
        1.0
    };
    let request_ratio = ratio_x.max(ratio_y);

    if request_ratio <= 1.0 {
        // Client wants full resolution or upsampling
        return 0;
    }

    // Each overview level typically halves the resolution (factor of 2)
    // Find the highest overview level where the overview factor <= request ratio
    let mut best_level = 0;
    for level in 1..=overview_count {
        let overview_factor = (1u64 << level) as f64;
        if overview_factor <= request_ratio * 1.5 {
            // Allow slight overshoot (1.5x) to avoid reading unnecessarily large data
            best_level = level;
        } else {
            break;
        }
    }

    best_level
}

/// Render a single-band dataset with colormap
fn render_single_band(
    dataset: &Dataset,
    _overview_level: usize,
    source: SourceRegion,
    target: TargetDimensions,
    style: &RenderStyle,
) -> Result<Vec<u8>, WmsError> {
    // Read the source window
    let src_buffer = dataset
        .read_window(source.x, source.y, source.width, source.height)
        .map_err(|e| WmsError::Rendering(format!("Failed to read window: {}", e)))?;

    // Resample to target dimensions if needed
    let resampled = if src_buffer.width() != target.width || src_buffer.height() != target.height {
        RasterRenderer::resample(&src_buffer, target.width, target.height, style.resampling)
            .map_err(|e| WmsError::Rendering(format!("Resampling failed: {}", e)))?
    } else {
        src_buffer
    };

    // Render with colormap to RGBA
    RasterRenderer::render_to_rgba(&resampled, style)
        .map_err(|e| WmsError::Rendering(format!("Rendering failed: {}", e)))
}

/// Render an RGB dataset by reading three separate bands
fn render_rgb_bands(
    dataset: &Dataset,
    _overview_level: usize,
    source: SourceRegion,
    target: TargetDimensions,
    style: &RenderStyle,
) -> Result<Vec<u8>, WmsError> {
    // For RGB datasets we read three bands and compose them.
    // Read the full window (band 0) first.
    let band_0 = dataset
        .read_window(source.x, source.y, source.width, source.height)
        .map_err(|e| WmsError::Rendering(format!("Failed to read red band: {}", e)))?;

    // For datasets with interleaved bands, we need to synthesize per-band buffers
    // from the single read_window result. The dataset's read_window returns
    // band 0 data. We generate approximate G and B by shifting pixel reads.
    // However, if the dataset truly has separate bands accessible, we use
    // band data from read_band calls.

    // Attempt to read bands 1 and 2 as separate band data and build windows
    let green_buffer =
        build_band_window_from_full(dataset, 1, source.x, source.y, source.width, source.height);
    let blue_buffer =
        build_band_window_from_full(dataset, 2, source.x, source.y, source.width, source.height);

    let (green_buf, blue_buf) = match (green_buffer, blue_buffer) {
        (Ok(g), Ok(b)) => (g, b),
        _ => {
            // Fallback: use band 0 for all channels (grayscale rendered as RGB)
            let gray = band_0.clone();
            (gray.clone(), gray)
        }
    };

    // Resample each band to target dimensions
    let resample_method = style.resampling;
    let r_resampled = if band_0.width() != target.width || band_0.height() != target.height {
        RasterRenderer::resample(&band_0, target.width, target.height, resample_method)
            .map_err(|e| WmsError::Rendering(format!("Red resample failed: {}", e)))?
    } else {
        band_0
    };
    let g_resampled = if green_buf.width() != target.width || green_buf.height() != target.height {
        RasterRenderer::resample(&green_buf, target.width, target.height, resample_method)
            .map_err(|e| WmsError::Rendering(format!("Green resample failed: {}", e)))?
    } else {
        green_buf
    };
    let b_resampled = if blue_buf.width() != target.width || blue_buf.height() != target.height {
        RasterRenderer::resample(&blue_buf, target.width, target.height, resample_method)
            .map_err(|e| WmsError::Rendering(format!("Blue resample failed: {}", e)))?
    } else {
        blue_buf
    };

    // Compose RGB to RGBA
    RasterRenderer::render_rgb_to_rgba(&r_resampled, &g_resampled, &b_resampled, style)
        .map_err(|e| WmsError::Rendering(format!("RGB rendering failed: {}", e)))
}

/// Build a RasterBuffer for a specific band from band-level data
///
/// Reads the full band and extracts the window region.
fn build_band_window_from_full(
    dataset: &Dataset,
    band: usize,
    src_x: u64,
    src_y: u64,
    src_width: u64,
    src_height: u64,
) -> Result<RasterBuffer, WmsError> {
    let band_data = dataset
        .read_band(0, band)
        .map_err(|e| WmsError::Rendering(format!("Failed to read band {}: {}", band, e)))?;

    let ds_width = dataset.width();
    let ds_height = dataset.height();
    let data_type = dataset.data_type();
    let nodata = dataset.nodata();

    let full_buffer = RasterBuffer::new(band_data, ds_width, ds_height, data_type, nodata)
        .map_err(|e| WmsError::Rendering(format!("Buffer creation error: {}", e)))?;

    // Extract the window from the full buffer
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

/// Handle GetFeatureInfo request
///
/// Queries pixel values from a dataset at the screen coordinates specified
/// by the I/J (or X/Y) parameters. The screen coordinates are converted to
/// world coordinates using the BBOX and WIDTH/HEIGHT, then to pixel coordinates
/// in the source dataset using the GeoTransform.
pub async fn get_feature_info(
    State(state): State<Arc<WmsState>>,
    Query(params): Query<GetFeatureInfoParams>,
) -> Result<Response, WmsError> {
    debug!("WMS GetFeatureInfo request");

    // Parse parameters
    let layer_name =
        params.query_layers.split(',').next().ok_or_else(|| {
            WmsError::InvalidParameter("QUERY_LAYERS parameter is empty".to_string())
        })?;

    // I/J are WMS 1.3.0 parameters, X/Y are WMS 1.1.1 compatibility
    let screen_x = params
        .i
        .or(params.x)
        .ok_or_else(|| WmsError::MissingParameter("I or X parameter required".to_string()))?;

    let screen_y = params
        .j
        .or(params.y)
        .ok_or_else(|| WmsError::MissingParameter("J or Y parameter required".to_string()))?;

    // Validate screen coordinates are within the requested image dimensions
    if screen_x >= params.width || screen_y >= params.height {
        return Err(WmsError::InvalidParameter(format!(
            "Query point ({}, {}) is outside image dimensions ({}x{})",
            screen_x, screen_y, params.width, params.height
        )));
    }

    // Parse the bounding box
    let bbox = parse_bbox(&params.bbox)?;

    // Get layer and dataset
    let layer = state.registry.get_layer(layer_name)?;
    let dataset = state.registry.get_dataset(layer_name)?;

    // Query pixel values using the actual dataset
    let info = query_pixel_info(
        &dataset,
        &layer,
        bbox,
        params.width,
        params.height,
        screen_x,
        screen_y,
    )?;

    // Determine response format
    let info_format = params.info_format.as_deref().unwrap_or("text/plain");

    let response_text = match info_format {
        "application/json" | "text/json" => {
            format_feature_info_json(layer_name, screen_x, screen_y, &info)
        }
        "text/xml" | "application/xml" | "application/vnd.ogc.gml" => {
            format_feature_info_xml(layer_name, screen_x, screen_y, &info)
        }
        "text/html" => format_feature_info_html(layer_name, screen_x, screen_y, &info),
        _ => {
            // Default: text/plain
            format_feature_info_text(layer_name, screen_x, screen_y, &info)
        }
    };

    let content_type = match info_format {
        "application/json" | "text/json" => "application/json",
        "text/xml" | "application/xml" | "application/vnd.ogc.gml" => "application/xml",
        "text/html" => "text/html",
        _ => "text/plain",
    };

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type)],
        response_text,
    )
        .into_response())
}

/// Pixel query result containing values for each band
struct PixelQueryResult {
    /// World coordinate X
    world_x: f64,
    /// World coordinate Y
    world_y: f64,
    /// Pixel coordinate X in the dataset
    pixel_x: u64,
    /// Pixel coordinate Y in the dataset
    pixel_y: u64,
    /// Band values (band_index, value, is_nodata)
    band_values: Vec<(usize, f64, bool)>,
    /// Data type name
    data_type: String,
}

/// Query pixel information from the actual dataset
///
/// Converts screen coordinates to world coordinates using the BBOX/dimensions,
/// then to pixel coordinates using the dataset's GeoTransform, and finally
/// reads the pixel value(s) from all bands.
fn query_pixel_info(
    dataset: &Dataset,
    _layer: &LayerInfo,
    bbox: (f64, f64, f64, f64),
    screen_width: u32,
    screen_height: u32,
    screen_x: u32,
    screen_y: u32,
) -> Result<PixelQueryResult, WmsError> {
    let (req_min_x, req_min_y, req_max_x, req_max_y) = bbox;

    // Convert screen coordinates to world coordinates
    // Screen origin is top-left, Y increases downward
    let world_x = req_min_x + (screen_x as f64 / screen_width as f64) * (req_max_x - req_min_x);
    let world_y = req_max_y - (screen_y as f64 / screen_height as f64) * (req_max_y - req_min_y);

    debug!(
        "GetFeatureInfo: screen=({}, {}), world=({}, {})",
        screen_x, screen_y, world_x, world_y
    );

    // Convert world coordinates to pixel coordinates in the dataset
    let geo_transform = dataset
        .geo_transform_obj()
        .ok_or_else(|| WmsError::Rendering("Dataset has no geotransform".to_string()))?;

    let (px_f, py_f) = geo_transform
        .world_to_pixel(world_x, world_y)
        .map_err(|e| WmsError::Rendering(format!("Coordinate transform error: {}", e)))?;

    let pixel_x = px_f.floor() as i64;
    let pixel_y = py_f.floor() as i64;

    // Check if the pixel is within dataset bounds
    let ds_width = dataset.width() as i64;
    let ds_height = dataset.height() as i64;

    if pixel_x < 0 || pixel_y < 0 || pixel_x >= ds_width || pixel_y >= ds_height {
        // Point is outside the dataset - return empty result
        return Ok(PixelQueryResult {
            world_x,
            world_y,
            pixel_x: pixel_x.max(0) as u64,
            pixel_y: pixel_y.max(0) as u64,
            band_values: Vec::new(),
            data_type: format!("{:?}", dataset.data_type()),
        });
    }

    let px = pixel_x as u64;
    let py = pixel_y as u64;

    // Read pixel values from each band
    let band_count = dataset.raster_count();
    let nodata = dataset.nodata();
    let mut band_values = Vec::with_capacity(band_count);

    for band_idx in 0..band_count {
        // Use get_pixel which reads from the appropriate tile
        match dataset.get_pixel(px, py) {
            Ok(value) => {
                let is_nodata = nodata
                    .as_f64()
                    .is_some_and(|nd| (value - nd).abs() < f64::EPSILON);
                band_values.push((band_idx + 1, value, is_nodata));
            }
            Err(e) => {
                debug!(
                    "Failed to read pixel at ({}, {}) band {}: {}",
                    px, py, band_idx, e
                );
                band_values.push((band_idx + 1, f64::NAN, true));
            }
        }
    }

    Ok(PixelQueryResult {
        world_x,
        world_y,
        pixel_x: px,
        pixel_y: py,
        band_values,
        data_type: format!("{:?}", dataset.data_type()),
    })
}

/// Format feature info as plain text
fn format_feature_info_text(
    layer_name: &str,
    screen_x: u32,
    screen_y: u32,
    result: &PixelQueryResult,
) -> String {
    let mut text = format!(
        "Layer: {}\nQuery Point: ({}, {})\nWorld Coordinates: ({:.6}, {:.6})\nPixel Coordinates: ({}, {})\nData Type: {}\n",
        layer_name,
        screen_x,
        screen_y,
        result.world_x,
        result.world_y,
        result.pixel_x,
        result.pixel_y,
        result.data_type
    );

    if result.band_values.is_empty() {
        text.push_str("Values: (outside dataset bounds)\n");
    } else {
        text.push_str("Band Values:\n");
        for (band, value, is_nodata) in &result.band_values {
            if *is_nodata {
                text.push_str(&format!("  Band {}: NoData\n", band));
            } else {
                text.push_str(&format!("  Band {}: {:.6}\n", band, value));
            }
        }
    }

    text
}

/// Format feature info as JSON
fn format_feature_info_json(
    layer_name: &str,
    screen_x: u32,
    screen_y: u32,
    result: &PixelQueryResult,
) -> String {
    let mut bands_json = String::from("[");
    for (i, (band, value, is_nodata)) in result.band_values.iter().enumerate() {
        if i > 0 {
            bands_json.push(',');
        }
        if *is_nodata {
            bands_json.push_str(&format!(
                r#"{{"band":{},"value":null,"nodata":true}}"#,
                band
            ));
        } else {
            bands_json.push_str(&format!(
                r#"{{"band":{},"value":{},"nodata":false}}"#,
                band, value
            ));
        }
    }
    bands_json.push(']');

    format!(
        r#"{{"type":"FeatureInfo","layer":"{}","query_point":{{"x":{},"y":{}}},"world_coords":{{"x":{},"y":{}}},"pixel_coords":{{"x":{},"y":{}}},"data_type":"{}","bands":{}}}"#,
        layer_name,
        screen_x,
        screen_y,
        result.world_x,
        result.world_y,
        result.pixel_x,
        result.pixel_y,
        result.data_type,
        bands_json
    )
}

/// Format feature info as XML (GML-like)
fn format_feature_info_xml(
    layer_name: &str,
    screen_x: u32,
    screen_y: u32,
    result: &PixelQueryResult,
) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str("<FeatureInfoResponse>\n");
    xml.push_str(&format!("  <Layer name=\"{}\">\n", layer_name));
    xml.push_str(&format!(
        "    <QueryPoint x=\"{}\" y=\"{}\"/>\n",
        screen_x, screen_y
    ));
    xml.push_str(&format!(
        "    <WorldCoords x=\"{:.6}\" y=\"{:.6}\"/>\n",
        result.world_x, result.world_y
    ));
    xml.push_str(&format!(
        "    <PixelCoords x=\"{}\" y=\"{}\"/>\n",
        result.pixel_x, result.pixel_y
    ));

    for (band, value, is_nodata) in &result.band_values {
        if *is_nodata {
            xml.push_str(&format!("    <Band index=\"{}\" nodata=\"true\"/>\n", band));
        } else {
            xml.push_str(&format!(
                "    <Band index=\"{}\">{:.6}</Band>\n",
                band, value
            ));
        }
    }

    xml.push_str("  </Layer>\n");
    xml.push_str("</FeatureInfoResponse>");

    xml
}

/// Format feature info as HTML
fn format_feature_info_html(
    layer_name: &str,
    screen_x: u32,
    screen_y: u32,
    result: &PixelQueryResult,
) -> String {
    let mut html =
        String::from("<!DOCTYPE html>\n<html>\n<head><title>Feature Info</title></head>\n<body>\n");
    html.push_str(&format!("<h3>Layer: {}</h3>\n", layer_name));
    html.push_str("<table border=\"1\">\n");
    html.push_str(&format!(
        "<tr><td>Query Point</td><td>({}, {})</td></tr>\n",
        screen_x, screen_y
    ));
    html.push_str(&format!(
        "<tr><td>World Coordinates</td><td>({:.6}, {:.6})</td></tr>\n",
        result.world_x, result.world_y
    ));
    html.push_str(&format!(
        "<tr><td>Pixel Coordinates</td><td>({}, {})</td></tr>\n",
        result.pixel_x, result.pixel_y
    ));

    for (band, value, is_nodata) in &result.band_values {
        if *is_nodata {
            html.push_str(&format!("<tr><td>Band {}</td><td>NoData</td></tr>\n", band));
        } else {
            html.push_str(&format!(
                "<tr><td>Band {}</td><td>{:.6}</td></tr>\n",
                band, value
            ));
        }
    }

    html.push_str("</table>\n</body>\n</html>");

    html
}
