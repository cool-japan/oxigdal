//! Dataset registry for managing available layers
//!
//! This module provides a thread-safe registry for managing GDAL datasets
//! that can be served via WMS/WMTS protocols.
//!
//! # CRS Transformation
//!
//! The registry supports bounding box transformation between coordinate reference systems.
//! When requesting a layer's bounding box in a target CRS different from the native CRS,
//! the registry performs:
//!
//! 1. Edge densification - Adding intermediate points along bbox edges for accurate transformation
//! 2. Coordinate transformation - Using proj4rs for coordinate conversion
//! 3. Axis order handling - Correctly handling lat/lon vs lon/lat conventions
//!
//! ## Supported CRS
//!
//! - EPSG:4326 (WGS84 - Geographic)
//! - EPSG:3857 (Web Mercator - Projected)
//! - All WGS84 UTM zones (EPSG:32601-32660 North, EPSG:32701-32760 South)
//! - Common national datums (NAD83, ETRS89, GDA94, JGD2000, etc.)

use crate::config::{ConfigError, LayerConfig};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_proj::{BoundingBox, Coordinate, Crs, Transformer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Dataset wrapper for GeoTIFF files
pub struct Dataset {
    /// Path to the dataset file
    path: PathBuf,
    /// GeoTIFF reader (wrapped for thread-safety)
    reader: RwLock<Option<GeoTiffReader<FileDataSource>>>,
    /// Cached metadata
    width: u64,
    height: u64,
    band_count: u32,
    data_type: RasterDataType,
    geo_transform: Option<GeoTransform>,
    nodata: NoDataValue,
    tile_size: Option<(u32, u32)>,
    overview_count: usize,
}

impl Dataset {
    /// Open a dataset from a file path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, oxigdal_core::OxiGdalError> {
        let path_buf = path.as_ref().to_path_buf();

        // Open the file source
        let source = FileDataSource::open(&path_buf)?;

        // Create the GeoTIFF reader
        let reader = GeoTiffReader::open(source)?;

        // Extract metadata
        let width = reader.width();
        let height = reader.height();
        let band_count = reader.band_count();
        let data_type = reader.data_type().unwrap_or(RasterDataType::UInt8);
        let geo_transform = reader.geo_transform().cloned();
        let nodata = reader.nodata();
        let tile_size = reader.tile_size();
        let overview_count = reader.overview_count();

        Ok(Self {
            path: path_buf,
            reader: RwLock::new(Some(reader)),
            width,
            height,
            band_count,
            data_type,
            geo_transform,
            nodata,
            tile_size,
            overview_count,
        })
    }

    /// Get the file path
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get raster size (width, height)
    #[must_use]
    pub fn raster_size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    /// Get raster width
    #[must_use]
    pub fn width(&self) -> u64 {
        self.width
    }

    /// Get raster height
    #[must_use]
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Get raster band count
    #[must_use]
    pub fn raster_count(&self) -> usize {
        self.band_count as usize
    }

    /// Get the data type
    #[must_use]
    pub fn data_type(&self) -> RasterDataType {
        self.data_type
    }

    /// Get projection (WKT string)
    pub fn projection(&self) -> Result<String, oxigdal_core::OxiGdalError> {
        // Return EPSG code if available
        if let Ok(guard) = self.reader.read() {
            if let Some(ref reader) = *guard {
                if let Some(epsg) = reader.epsg_code() {
                    return Ok(format!("EPSG:{}", epsg));
                }
            }
        }
        Ok("EPSG:4326".to_string())
    }

    /// Get geotransform as array
    pub fn geotransform(&self) -> Result<[f64; 6], oxigdal_core::OxiGdalError> {
        if let Some(ref gt) = self.geo_transform {
            Ok(gt.to_gdal_array())
        } else {
            // Default identity transform
            Ok([0.0, 1.0, 0.0, 0.0, 0.0, -1.0])
        }
    }

    /// Get the GeoTransform
    #[must_use]
    pub fn geo_transform_obj(&self) -> Option<&GeoTransform> {
        self.geo_transform.as_ref()
    }

    /// Get NoData value
    #[must_use]
    pub fn nodata(&self) -> NoDataValue {
        self.nodata
    }

    /// Get tile size if tiled
    #[must_use]
    pub fn tile_size(&self) -> Option<(u32, u32)> {
        self.tile_size
    }

    /// Get number of overview levels
    #[must_use]
    pub fn overview_count(&self) -> usize {
        self.overview_count
    }

    /// Get bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<oxigdal_core::types::BoundingBox> {
        self.geo_transform
            .as_ref()
            .map(|gt| gt.compute_bounds(self.width, self.height))
    }

    /// Read a tile from the dataset
    ///
    /// # Arguments
    /// * `level` - Overview level (0 = full resolution)
    /// * `tile_x` - Tile X coordinate
    /// * `tile_y` - Tile Y coordinate
    pub fn read_tile(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<Vec<u8>, oxigdal_core::OxiGdalError> {
        let guard = self
            .reader
            .read()
            .map_err(|_| oxigdal_core::OxiGdalError::Internal {
                message: "Failed to acquire read lock".to_string(),
            })?;

        if let Some(ref reader) = *guard {
            reader.read_tile(level, tile_x, tile_y)
        } else {
            Err(oxigdal_core::OxiGdalError::Internal {
                message: "Reader not initialized".to_string(),
            })
        }
    }

    /// Read a tile as RasterBuffer
    pub fn read_tile_buffer(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<RasterBuffer, oxigdal_core::OxiGdalError> {
        let guard = self
            .reader
            .read()
            .map_err(|_| oxigdal_core::OxiGdalError::Internal {
                message: "Failed to acquire read lock".to_string(),
            })?;

        if let Some(ref reader) = *guard {
            reader.read_tile_buffer(level, tile_x, tile_y)
        } else {
            Err(oxigdal_core::OxiGdalError::Internal {
                message: "Reader not initialized".to_string(),
            })
        }
    }

    /// Read full band data
    pub fn read_band(
        &self,
        level: usize,
        band: usize,
    ) -> Result<Vec<u8>, oxigdal_core::OxiGdalError> {
        let guard = self
            .reader
            .read()
            .map_err(|_| oxigdal_core::OxiGdalError::Internal {
                message: "Failed to acquire read lock".to_string(),
            })?;

        if let Some(ref reader) = *guard {
            reader.read_band(level, band)
        } else {
            Err(oxigdal_core::OxiGdalError::Internal {
                message: "Reader not initialized".to_string(),
            })
        }
    }

    /// Read a window of data from the dataset as RasterBuffer
    ///
    /// # Arguments
    /// * `x_offset` - X offset in pixels
    /// * `y_offset` - Y offset in pixels
    /// * `x_size` - Width to read
    /// * `y_size` - Height to read
    pub fn read_window(
        &self,
        x_offset: u64,
        y_offset: u64,
        x_size: u64,
        y_size: u64,
    ) -> Result<RasterBuffer, oxigdal_core::OxiGdalError> {
        // Validate window bounds
        if x_offset >= self.width || y_offset >= self.height {
            return Err(oxigdal_core::OxiGdalError::OutOfBounds {
                message: format!(
                    "Window offset ({}, {}) out of bounds ({}x{})",
                    x_offset, y_offset, self.width, self.height
                ),
            });
        }

        // Clamp window size to dataset bounds
        let actual_x_size = x_size.min(self.width - x_offset);
        let actual_y_size = y_size.min(self.height - y_offset);

        // Create output buffer with requested size
        let mut window_buffer = RasterBuffer::zeros(x_size, y_size, self.data_type);

        // Get tile dimensions (default to 256x256 if not tiled)
        let (tile_w, tile_h) = self.tile_size.unwrap_or((256, 256));
        let tile_w = tile_w as u64;
        let tile_h = tile_h as u64;

        // Calculate tile range that intersects with the requested window
        let start_tile_x = x_offset / tile_w;
        let start_tile_y = y_offset / tile_h;
        let end_tile_x = (x_offset + actual_x_size).div_ceil(tile_w);
        let end_tile_y = (y_offset + actual_y_size).div_ceil(tile_h);

        // Read only the tiles that intersect with the window
        for tile_y in start_tile_y..end_tile_y {
            for tile_x in start_tile_x..end_tile_x {
                // Calculate tile boundaries in dataset coordinates
                let tile_pixel_x = tile_x * tile_w;
                let tile_pixel_y = tile_y * tile_h;

                // Read the tile
                let tile_buffer = match self.read_tile_buffer(0, tile_x as u32, tile_y as u32) {
                    Ok(buf) => buf,
                    Err(e) => {
                        // If tile read fails, skip it (may be outside bounds or missing)
                        debug!("Failed to read tile ({}, {}): {}", tile_x, tile_y, e);
                        continue;
                    }
                };

                let tile_width = tile_buffer.width();
                let tile_height = tile_buffer.height();

                // Calculate the intersection between tile and requested window
                let win_min_x = x_offset;
                let win_min_y = y_offset;
                let win_max_x = x_offset + actual_x_size;
                let win_max_y = y_offset + actual_y_size;

                let tile_max_x = (tile_pixel_x + tile_width).min(self.width);
                let tile_max_y = (tile_pixel_y + tile_height).min(self.height);

                let intersect_min_x = win_min_x.max(tile_pixel_x);
                let intersect_min_y = win_min_y.max(tile_pixel_y);
                let intersect_max_x = win_max_x.min(tile_max_x);
                let intersect_max_y = win_max_y.min(tile_max_y);

                // Copy pixels from tile to window buffer
                for src_y in intersect_min_y..intersect_max_y {
                    for src_x in intersect_min_x..intersect_max_x {
                        // Calculate position in tile coordinates
                        let tile_local_x = src_x - tile_pixel_x;
                        let tile_local_y = src_y - tile_pixel_y;

                        // Calculate position in window coordinates
                        let win_local_x = src_x - x_offset;
                        let win_local_y = src_y - y_offset;

                        // Read pixel from tile and write to window buffer
                        if let Ok(value) = tile_buffer.get_pixel(tile_local_x, tile_local_y) {
                            let _ = window_buffer.set_pixel(win_local_x, win_local_y, value);
                        }
                    }
                }
            }
        }

        Ok(window_buffer)
    }

    /// Get pixel value at coordinates
    pub fn get_pixel(&self, x: u64, y: u64) -> Result<f64, oxigdal_core::OxiGdalError> {
        if x >= self.width || y >= self.height {
            return Err(oxigdal_core::OxiGdalError::OutOfBounds {
                message: format!(
                    "Pixel ({}, {}) out of bounds ({}x{})",
                    x, y, self.width, self.height
                ),
            });
        }

        // Determine which tile contains this pixel
        let (tile_w, tile_h) = self.tile_size.unwrap_or((256, 256));
        let tile_x = x / tile_w as u64;
        let tile_y = y / tile_h as u64;
        let local_x = x % tile_w as u64;
        let local_y = y % tile_h as u64;

        // Read the tile
        let tile_buffer = self.read_tile_buffer(0, tile_x as u32, tile_y as u32)?;

        // Get the pixel value
        tile_buffer.get_pixel(local_x, local_y)
    }

    /// Get raster band info (for compatibility with old code)
    pub fn rasterband(&self, _band: usize) -> Result<RasterBandInfo, oxigdal_core::OxiGdalError> {
        Ok(RasterBandInfo {
            nodata: self.nodata,
            data_type: self.data_type,
        })
    }
}

/// Raster band information (compatible with old interface)
pub struct RasterBandInfo {
    nodata: NoDataValue,
    data_type: RasterDataType,
}

impl RasterBandInfo {
    /// Get nodata value
    pub fn nodata(&self) -> Option<f64> {
        self.nodata.as_f64()
    }

    /// Get datatype string
    pub fn datatype(&self) -> &str {
        match self.data_type {
            RasterDataType::UInt8 => "UInt8",
            RasterDataType::Int8 => "Int8",
            RasterDataType::UInt16 => "UInt16",
            RasterDataType::Int16 => "Int16",
            RasterDataType::UInt32 => "UInt32",
            RasterDataType::Int32 => "Int32",
            RasterDataType::Float32 => "Float32",
            RasterDataType::Float64 => "Float64",
            RasterDataType::UInt64 => "UInt64",
            RasterDataType::Int64 => "Int64",
            RasterDataType::CFloat32 => "CFloat32",
            RasterDataType::CFloat64 => "CFloat64",
        }
    }
}

/// Registry errors
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Layer not found
    #[error("Layer not found: {0}")]
    LayerNotFound(String),

    /// Dataset open error
    #[error("Failed to open dataset: {0}")]
    DatasetOpen(#[from] oxigdal_core::OxiGdalError),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// CRS transformation error
    #[error("CRS transformation failed: {0}")]
    CrsTransformation(String),

    /// Invalid CRS specification
    #[error("Invalid CRS: {0}")]
    InvalidCrs(String),

    /// Lock poisoned
    #[error("Lock poisoned")]
    LockPoisoned,
}

impl From<oxigdal_proj::Error> for RegistryError {
    fn from(err: oxigdal_proj::Error) -> Self {
        RegistryError::CrsTransformation(err.to_string())
    }
}

/// Result type for registry operations
pub type RegistryResult<T> = Result<T, RegistryError>;

/// Information about a registered layer
#[derive(Debug, Clone)]
pub struct LayerInfo {
    /// Layer name
    pub name: String,

    /// Display title
    pub title: String,

    /// Layer description
    pub abstract_: String,

    /// Configuration
    pub config: LayerConfig,

    /// Dataset metadata
    pub metadata: DatasetMetadata,
}

/// Dataset metadata extracted from GDAL
#[derive(Debug, Clone)]
pub struct DatasetMetadata {
    /// Dataset width in pixels
    pub width: usize,

    /// Dataset height in pixels
    pub height: usize,

    /// Number of bands
    pub band_count: usize,

    /// Data type name
    pub data_type: String,

    /// Spatial reference system (WKT)
    pub srs: Option<String>,

    /// Bounding box (min_x, min_y, max_x, max_y)
    pub bbox: Option<(f64, f64, f64, f64)>,

    /// Geotransform coefficients
    pub geotransform: Option<[f64; 6]>,

    /// NoData value
    pub nodata: Option<f64>,
}

/// Thread-safe dataset registry
pub struct DatasetRegistry {
    /// Registered layers
    layers: Arc<RwLock<HashMap<String, LayerInfo>>>,

    /// Dataset cache (opened datasets)
    datasets: Arc<RwLock<HashMap<String, Arc<Dataset>>>>,
}

impl DatasetRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            layers: Arc::new(RwLock::new(HashMap::new())),
            datasets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a layer from configuration
    pub fn register_layer(&self, config: LayerConfig) -> RegistryResult<()> {
        info!("Registering layer: {}", config.name);

        // Save the name before moving config
        let layer_name = config.name.clone();

        // Open dataset to extract metadata
        let dataset = Self::open_dataset(&config.path)?;
        let metadata = Self::extract_metadata(&dataset)?;

        debug!(
            "Layer {} metadata: {}x{}, {} bands",
            layer_name, metadata.width, metadata.height, metadata.band_count
        );

        let layer_info = LayerInfo {
            name: config.name.clone(),
            title: config.title.clone().unwrap_or_else(|| config.name.clone()),
            abstract_: config
                .abstract_
                .clone()
                .unwrap_or_else(|| format!("Layer {}", config.name)),
            config,
            metadata,
        };

        // Store layer info
        let mut layers = self
            .layers
            .write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        layers.insert(layer_info.name.clone(), layer_info);

        // Cache the dataset
        let mut datasets = self
            .datasets
            .write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        datasets.insert(layer_name, Arc::new(dataset));

        Ok(())
    }

    /// Register multiple layers from configurations
    pub fn register_layers(&self, configs: Vec<LayerConfig>) -> RegistryResult<()> {
        for config in configs {
            if !config.enabled {
                debug!("Skipping disabled layer: {}", config.name);
                continue;
            }

            if let Err(e) = self.register_layer(config) {
                warn!("Failed to register layer: {}", e);
                // Continue with other layers
            }
        }
        Ok(())
    }

    /// Get layer information
    pub fn get_layer(&self, name: &str) -> RegistryResult<LayerInfo> {
        let layers = self
            .layers
            .read()
            .map_err(|_| RegistryError::LockPoisoned)?;

        layers
            .get(name)
            .cloned()
            .ok_or_else(|| RegistryError::LayerNotFound(name.to_string()))
    }

    /// Get a dataset for a layer
    pub fn get_dataset(&self, name: &str) -> RegistryResult<Arc<Dataset>> {
        let datasets = self
            .datasets
            .read()
            .map_err(|_| RegistryError::LockPoisoned)?;

        datasets
            .get(name)
            .cloned()
            .ok_or_else(|| RegistryError::LayerNotFound(name.to_string()))
    }

    /// List all registered layers
    pub fn list_layers(&self) -> RegistryResult<Vec<LayerInfo>> {
        let layers = self
            .layers
            .read()
            .map_err(|_| RegistryError::LockPoisoned)?;

        Ok(layers.values().cloned().collect())
    }

    /// Check if a layer exists
    pub fn has_layer(&self, name: &str) -> bool {
        self.layers
            .read()
            .map(|layers| layers.contains_key(name))
            .unwrap_or(false)
    }

    /// Remove a layer from the registry
    pub fn unregister_layer(&self, name: &str) -> RegistryResult<()> {
        let mut layers = self
            .layers
            .write()
            .map_err(|_| RegistryError::LockPoisoned)?;

        let mut datasets = self
            .datasets
            .write()
            .map_err(|_| RegistryError::LockPoisoned)?;

        layers.remove(name);
        datasets.remove(name);

        info!("Unregistered layer: {}", name);
        Ok(())
    }

    /// Get the number of registered layers
    pub fn layer_count(&self) -> usize {
        self.layers.read().map(|l| l.len()).unwrap_or(0)
    }

    /// Open a dataset from a path
    fn open_dataset<P: AsRef<Path>>(path: P) -> RegistryResult<Dataset> {
        let dataset = Dataset::open(path.as_ref())?;
        Ok(dataset)
    }

    /// Extract metadata from a dataset
    fn extract_metadata(dataset: &Dataset) -> RegistryResult<DatasetMetadata> {
        let raster_size = dataset.raster_size();
        let band_count = dataset.raster_count();

        // Get spatial reference
        let srs = dataset.projection().ok();

        // Get bounding box from geotransform
        let geotransform = dataset.geotransform().ok();
        let bbox = geotransform.map(|gt| {
            let width = raster_size.0 as f64;
            let height = raster_size.1 as f64;

            let min_x = gt[0];
            let max_x = gt[0] + gt[1] * width + gt[2] * height;
            let max_y = gt[3];
            let min_y = gt[3] + gt[4] * width + gt[5] * height;

            // Ensure proper order
            let (min_x, max_x) = if min_x < max_x {
                (min_x, max_x)
            } else {
                (max_x, min_x)
            };
            let (min_y, max_y) = if min_y < max_y {
                (min_y, max_y)
            } else {
                (max_y, min_y)
            };

            (min_x, min_y, max_x, max_y)
        });

        // Get NoData value from first band
        let nodata = if band_count > 0 {
            dataset.rasterband(1).ok().and_then(|band| band.nodata())
        } else {
            None
        };

        // Get data type from first band
        let data_type = if band_count > 0 {
            dataset
                .rasterband(1)
                .ok()
                .map(|band| format!("{:?}", band.datatype()))
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            "Unknown".to_string()
        };

        Ok(DatasetMetadata {
            width: raster_size.0,
            height: raster_size.1,
            band_count,
            data_type,
            srs,
            bbox,
            geotransform,
            nodata,
        })
    }

    /// Reload a layer (useful after dataset updates)
    pub fn reload_layer(&self, name: &str) -> RegistryResult<()> {
        let config = {
            let layers = self
                .layers
                .read()
                .map_err(|_| RegistryError::LockPoisoned)?;

            layers
                .get(name)
                .ok_or_else(|| RegistryError::LayerNotFound(name.to_string()))?
                .config
                .clone()
        };

        self.unregister_layer(name)?;
        self.register_layer(config)?;

        info!("Reloaded layer: {}", name);
        Ok(())
    }

    /// Get layer bounding box in a specific CRS
    ///
    /// This method transforms the native bounding box of a layer to a target CRS.
    /// The transformation uses edge densification for accurate results, especially
    /// when transforming between geographic and projected coordinate systems.
    ///
    /// # Arguments
    ///
    /// * `name` - The layer name
    /// * `target_crs` - Optional target CRS specification (e.g., "EPSG:4326", "EPSG:3857")
    ///
    /// # Returns
    ///
    /// The bounding box in the target CRS as (min_x, min_y, max_x, max_y), or None
    /// if the layer has no bounding box defined.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The layer is not found
    /// - The source or target CRS is invalid
    /// - The transformation fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let registry = DatasetRegistry::new();
    /// // Get bounding box in Web Mercator
    /// let bbox = registry.get_layer_bbox("my_layer", Some("EPSG:3857"))?;
    /// ```
    pub fn get_layer_bbox(
        &self,
        name: &str,
        target_crs: Option<&str>,
    ) -> RegistryResult<Option<(f64, f64, f64, f64)>> {
        let layer = self.get_layer(name)?;

        // Return native bbox if no target CRS specified
        let target_crs_str = match target_crs {
            Some(crs) => crs,
            None => return Ok(layer.metadata.bbox),
        };

        // Get the native bbox
        let native_bbox = match layer.metadata.bbox {
            Some(bbox) => bbox,
            None => return Ok(None),
        };

        // Get the source CRS from layer metadata
        let source_crs_str = layer.metadata.srs.as_deref().unwrap_or("EPSG:4326");

        // Check if source and target CRS are the same
        if Self::crs_strings_equivalent(source_crs_str, target_crs_str) {
            return Ok(Some(native_bbox));
        }

        // Parse source and target CRS
        let source_crs = Self::parse_crs(source_crs_str)?;
        let target_crs = Self::parse_crs(target_crs_str)?;

        // Transform the bounding box
        let transformed =
            Self::transform_bbox_with_densification(native_bbox, &source_crs, &target_crs)?;

        Ok(Some(transformed))
    }

    /// Parse a CRS string into a Crs object
    ///
    /// Supports formats:
    /// - "EPSG:4326" or "epsg:4326"
    /// - PROJ strings ("+proj=longlat +datum=WGS84 +no_defs")
    /// - WKT strings
    fn parse_crs(crs_str: &str) -> RegistryResult<Crs> {
        let trimmed = crs_str.trim();

        // Try EPSG code format
        if let Some(code_str) = trimmed.to_uppercase().strip_prefix("EPSG:") {
            let code: u32 = code_str.parse().map_err(|_| {
                RegistryError::InvalidCrs(format!("Invalid EPSG code: {}", code_str))
            })?;
            return Ok(Crs::from_epsg(code)?);
        }

        // Try PROJ string
        if trimmed.starts_with("+proj=") || trimmed.contains("+proj=") {
            return Ok(Crs::from_proj(trimmed)?);
        }

        // Try WKT
        if trimmed.starts_with("GEOGCS[")
            || trimmed.starts_with("PROJCS[")
            || trimmed.starts_with("GEOCCS[")
        {
            return Ok(Crs::from_wkt(trimmed)?);
        }

        Err(RegistryError::InvalidCrs(format!(
            "Unrecognized CRS format: {}",
            crs_str
        )))
    }

    /// Check if two CRS strings refer to the same CRS
    fn crs_strings_equivalent(crs1: &str, crs2: &str) -> bool {
        let norm1 = crs1.trim().to_uppercase();
        let norm2 = crs2.trim().to_uppercase();
        norm1 == norm2
    }

    /// Transform a bounding box with edge densification for accurate results
    ///
    /// For accurate transformation between coordinate systems (especially between
    /// geographic and projected CRS), we densify the edges of the bounding box
    /// by adding intermediate points. This accounts for the curvature introduced
    /// by the projection.
    ///
    /// # Arguments
    ///
    /// * `bbox` - The bounding box as (min_x, min_y, max_x, max_y)
    /// * `source_crs` - Source coordinate reference system
    /// * `target_crs` - Target coordinate reference system
    ///
    /// # Returns
    ///
    /// The transformed bounding box as (min_x, min_y, max_x, max_y)
    fn transform_bbox_with_densification(
        bbox: (f64, f64, f64, f64),
        source_crs: &Crs,
        target_crs: &Crs,
    ) -> RegistryResult<(f64, f64, f64, f64)> {
        let (min_x, min_y, max_x, max_y) = bbox;

        // Create transformer
        let transformer = Transformer::new(source_crs.clone(), target_crs.clone())?;

        // Number of points to sample along each edge for accurate transformation
        // More points = more accurate but slower
        // 21 points per edge is a good balance (20 segments)
        const DENSIFY_POINTS: usize = 21;

        // Generate densified edge points
        let edge_points = Self::densify_bbox_edges(min_x, min_y, max_x, max_y, DENSIFY_POINTS);

        // Transform all edge points
        let transformed_points: Vec<Coordinate> = edge_points
            .iter()
            .filter_map(|coord| transformer.transform(coord).ok())
            .collect();

        // Check if we got valid transformed points
        if transformed_points.is_empty() {
            return Err(RegistryError::CrsTransformation(
                "All points failed to transform".to_string(),
            ));
        }

        // Find the bounding box of transformed points
        let mut result_min_x = f64::INFINITY;
        let mut result_min_y = f64::INFINITY;
        let mut result_max_x = f64::NEG_INFINITY;
        let mut result_max_y = f64::NEG_INFINITY;

        for point in &transformed_points {
            if point.x.is_finite() && point.y.is_finite() {
                result_min_x = result_min_x.min(point.x);
                result_min_y = result_min_y.min(point.y);
                result_max_x = result_max_x.max(point.x);
                result_max_y = result_max_y.max(point.y);
            }
        }

        // Verify we have valid results
        if !result_min_x.is_finite()
            || !result_min_y.is_finite()
            || !result_max_x.is_finite()
            || !result_max_y.is_finite()
        {
            return Err(RegistryError::CrsTransformation(
                "Transformation resulted in non-finite values".to_string(),
            ));
        }

        Ok((result_min_x, result_min_y, result_max_x, result_max_y))
    }

    /// Generate densified points along the edges of a bounding box
    ///
    /// Creates evenly spaced points along all four edges of the bbox.
    /// The points are ordered: bottom edge, right edge, top edge, left edge.
    fn densify_bbox_edges(
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        points_per_edge: usize,
    ) -> Vec<Coordinate> {
        let mut points = Vec::with_capacity(points_per_edge * 4);

        // Ensure at least 2 points per edge (corners)
        let n = points_per_edge.max(2);

        // Bottom edge (min_y, from min_x to max_x)
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            let x = min_x + t * (max_x - min_x);
            points.push(Coordinate::new(x, min_y));
        }

        // Right edge (max_x, from min_y to max_y)
        // Skip first point to avoid duplicate corner
        for i in 1..n {
            let t = i as f64 / (n - 1) as f64;
            let y = min_y + t * (max_y - min_y);
            points.push(Coordinate::new(max_x, y));
        }

        // Top edge (max_y, from max_x to min_x)
        // Skip first point to avoid duplicate corner
        for i in 1..n {
            let t = i as f64 / (n - 1) as f64;
            let x = max_x - t * (max_x - min_x);
            points.push(Coordinate::new(x, max_y));
        }

        // Left edge (min_x, from max_y to min_y)
        // Skip first and last points to avoid duplicate corners
        for i in 1..(n - 1) {
            let t = i as f64 / (n - 1) as f64;
            let y = max_y - t * (max_y - min_y);
            points.push(Coordinate::new(min_x, y));
        }

        points
    }

    /// Transform a single point from source to target CRS
    ///
    /// Convenience method for transforming individual coordinates.
    #[allow(dead_code)]
    fn transform_point(
        x: f64,
        y: f64,
        source_crs: &Crs,
        target_crs: &Crs,
    ) -> RegistryResult<(f64, f64)> {
        let transformer = Transformer::new(source_crs.clone(), target_crs.clone())?;
        let coord = Coordinate::new(x, y);
        let transformed = transformer.transform(&coord)?;
        Ok((transformed.x, transformed.y))
    }

    /// Get layer bounding box in Web Mercator (EPSG:3857)
    ///
    /// Convenience method for getting bbox in the most common web mapping CRS.
    pub fn get_layer_bbox_web_mercator(
        &self,
        name: &str,
    ) -> RegistryResult<Option<(f64, f64, f64, f64)>> {
        self.get_layer_bbox(name, Some("EPSG:3857"))
    }

    /// Get layer bounding box in WGS84 (EPSG:4326)
    ///
    /// Convenience method for getting bbox in geographic coordinates.
    pub fn get_layer_bbox_wgs84(&self, name: &str) -> RegistryResult<Option<(f64, f64, f64, f64)>> {
        self.get_layer_bbox(name, Some("EPSG:4326"))
    }

    /// Transform bounding box using the simple 4-corner method
    ///
    /// This is faster but less accurate than densification for projections
    /// with significant curvature. Use for quick approximations.
    #[allow(dead_code)]
    fn transform_bbox_simple(
        bbox: (f64, f64, f64, f64),
        source_crs: &Crs,
        target_crs: &Crs,
    ) -> RegistryResult<(f64, f64, f64, f64)> {
        let (min_x, min_y, max_x, max_y) = bbox;

        // Create bounding box using oxigdal_proj
        let source_bbox = BoundingBox::new(min_x, min_y, max_x, max_y)
            .map_err(|e| RegistryError::CrsTransformation(e.to_string()))?;

        // Create transformer and transform bbox
        let transformer = Transformer::new(source_crs.clone(), target_crs.clone())?;
        let transformed = transformer.transform_bbox(&source_bbox)?;

        Ok((
            transformed.min_x,
            transformed.min_y,
            transformed.max_x,
            transformed.max_y,
        ))
    }
}

impl Default for DatasetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DatasetRegistry {
    fn clone(&self) -> Self {
        Self {
            layers: Arc::clone(&self.layers),
            datasets: Arc::clone(&self.datasets),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = DatasetRegistry::new();
        assert_eq!(registry.layer_count(), 0);
        assert!(!registry.has_layer("test"));
    }

    #[test]
    fn test_layer_not_found() {
        let registry = DatasetRegistry::new();
        let result = registry.get_layer("nonexistent");
        assert!(matches!(result, Err(RegistryError::LayerNotFound(_))));
    }

    #[test]
    fn test_registry_clone() {
        let registry1 = DatasetRegistry::new();
        let registry2 = registry1.clone();

        assert_eq!(registry1.layer_count(), registry2.layer_count());
    }

    // CRS parsing tests
    #[test]
    fn test_parse_crs_epsg_uppercase() {
        let crs = DatasetRegistry::parse_crs("EPSG:4326");
        assert!(crs.is_ok());
        let crs = crs.expect("should parse");
        assert_eq!(crs.epsg_code(), Some(4326));
    }

    #[test]
    fn test_parse_crs_epsg_lowercase() {
        let crs = DatasetRegistry::parse_crs("epsg:3857");
        assert!(crs.is_ok());
        let crs = crs.expect("should parse");
        assert_eq!(crs.epsg_code(), Some(3857));
    }

    #[test]
    fn test_parse_crs_proj_string() {
        let crs = DatasetRegistry::parse_crs("+proj=longlat +datum=WGS84 +no_defs");
        assert!(crs.is_ok());
    }

    #[test]
    fn test_parse_crs_wkt() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]]"#;
        let crs = DatasetRegistry::parse_crs(wkt);
        assert!(crs.is_ok());
    }

    #[test]
    fn test_parse_crs_invalid() {
        let crs = DatasetRegistry::parse_crs("invalid crs");
        assert!(matches!(crs, Err(RegistryError::InvalidCrs(_))));
    }

    #[test]
    fn test_parse_crs_invalid_epsg() {
        let crs = DatasetRegistry::parse_crs("EPSG:abc");
        assert!(matches!(crs, Err(RegistryError::InvalidCrs(_))));
    }

    // CRS equivalence tests
    #[test]
    fn test_crs_strings_equivalent_same() {
        assert!(DatasetRegistry::crs_strings_equivalent(
            "EPSG:4326",
            "EPSG:4326"
        ));
    }

    #[test]
    fn test_crs_strings_equivalent_case_insensitive() {
        assert!(DatasetRegistry::crs_strings_equivalent(
            "EPSG:4326",
            "epsg:4326"
        ));
        assert!(DatasetRegistry::crs_strings_equivalent(
            "epsg:3857",
            "EPSG:3857"
        ));
    }

    #[test]
    fn test_crs_strings_equivalent_with_whitespace() {
        assert!(DatasetRegistry::crs_strings_equivalent(
            "  EPSG:4326  ",
            "EPSG:4326"
        ));
    }

    #[test]
    fn test_crs_strings_not_equivalent() {
        assert!(!DatasetRegistry::crs_strings_equivalent(
            "EPSG:4326",
            "EPSG:3857"
        ));
    }

    // Densification tests
    #[test]
    fn test_densify_bbox_edges_minimum() {
        let points = DatasetRegistry::densify_bbox_edges(0.0, 0.0, 10.0, 10.0, 2);
        // With 2 points per edge, we get corners only
        // 2 (bottom) + 1 (right, skip corner) + 1 (top, skip corner) + 0 (left, skip both corners)
        assert_eq!(points.len(), 4);

        // Check corners exist
        assert!(
            points
                .iter()
                .any(|p| (p.x - 0.0).abs() < 1e-10 && (p.y - 0.0).abs() < 1e-10)
        );
        assert!(
            points
                .iter()
                .any(|p| (p.x - 10.0).abs() < 1e-10 && (p.y - 0.0).abs() < 1e-10)
        );
        assert!(
            points
                .iter()
                .any(|p| (p.x - 10.0).abs() < 1e-10 && (p.y - 10.0).abs() < 1e-10)
        );
        assert!(
            points
                .iter()
                .any(|p| (p.x - 0.0).abs() < 1e-10 && (p.y - 10.0).abs() < 1e-10)
        );
    }

    #[test]
    fn test_densify_bbox_edges_5_points() {
        let points = DatasetRegistry::densify_bbox_edges(0.0, 0.0, 10.0, 10.0, 5);
        // 5 (bottom) + 4 (right) + 4 (top) + 3 (left) = 16
        assert_eq!(points.len(), 16);
    }

    #[test]
    fn test_densify_bbox_edges_21_points() {
        let points = DatasetRegistry::densify_bbox_edges(-10.0, -10.0, 10.0, 10.0, 21);
        // 21 (bottom) + 20 (right) + 20 (top) + 19 (left) = 80
        assert_eq!(points.len(), 80);

        // Check that corners are included
        let has_bottom_left = points
            .iter()
            .any(|p| (p.x - (-10.0)).abs() < 1e-10 && (p.y - (-10.0)).abs() < 1e-10);
        let has_bottom_right = points
            .iter()
            .any(|p| (p.x - 10.0).abs() < 1e-10 && (p.y - (-10.0)).abs() < 1e-10);
        let has_top_right = points
            .iter()
            .any(|p| (p.x - 10.0).abs() < 1e-10 && (p.y - 10.0).abs() < 1e-10);
        let has_top_left = points
            .iter()
            .any(|p| (p.x - (-10.0)).abs() < 1e-10 && (p.y - 10.0).abs() < 1e-10);

        assert!(has_bottom_left, "Should have bottom-left corner");
        assert!(has_bottom_right, "Should have bottom-right corner");
        assert!(has_top_right, "Should have top-right corner");
        assert!(has_top_left, "Should have top-left corner");
    }

    // Bbox transformation tests
    #[test]
    fn test_transform_bbox_same_crs() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::wgs84();
        let bbox = (0.0, 0.0, 10.0, 10.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (min_x, min_y, max_x, max_y) = result.expect("should transform");
        assert!((min_x - 0.0).abs() < 1e-6);
        assert!((min_y - 0.0).abs() < 1e-6);
        assert!((max_x - 10.0).abs() < 1e-6);
        assert!((max_y - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_bbox_wgs84_to_web_mercator() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::web_mercator();

        // Small bbox around null island
        let bbox = (-1.0, -1.0, 1.0, 1.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (min_x, min_y, max_x, max_y) = result.expect("should transform");

        // In Web Mercator, 1 degree at equator is approximately 111,320 meters
        // So bbox should be roughly centered at 0,0 and extend about 111km in each direction
        assert!(min_x < 0.0, "min_x should be negative");
        assert!(min_y < 0.0, "min_y should be negative");
        assert!(max_x > 0.0, "max_x should be positive");
        assert!(max_y > 0.0, "max_y should be positive");

        // Rough check for Web Mercator coordinates
        assert!(min_x > -200_000.0, "min_x should be > -200000");
        assert!(max_x < 200_000.0, "max_x should be < 200000");
        assert!(min_y > -200_000.0, "min_y should be > -200000");
        assert!(max_y < 200_000.0, "max_y should be < 200000");
    }

    #[test]
    fn test_transform_bbox_web_mercator_to_wgs84() {
        let source_crs = Crs::web_mercator();
        let target_crs = Crs::wgs84();

        // 1 million meters from origin (roughly 9 degrees)
        let bbox = (-1_000_000.0, -1_000_000.0, 1_000_000.0, 1_000_000.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (min_x, min_y, max_x, max_y) = result.expect("should transform");

        // Should be roughly +-9 degrees
        assert!(
            min_x > -15.0 && min_x < -5.0,
            "min_x should be around -9 degrees"
        );
        assert!(
            max_x > 5.0 && max_x < 15.0,
            "max_x should be around 9 degrees"
        );
        assert!(
            min_y > -15.0 && min_y < -5.0,
            "min_y should be around -9 degrees"
        );
        assert!(
            max_y > 5.0 && max_y < 15.0,
            "max_y should be around 9 degrees"
        );
    }

    #[test]
    fn test_transform_bbox_high_latitude() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::web_mercator();

        // Northern Europe bbox (demonstrates importance of densification)
        let bbox = (0.0, 50.0, 10.0, 60.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (_min_x, min_y, _max_x, max_y) = result.expect("should transform");

        // Web Mercator Y values should be large positive numbers for 50-60 degrees N
        assert!(min_y > 6_000_000.0, "min_y should be > 6M for 50 degrees N");
        assert!(max_y > 8_000_000.0, "max_y should be > 8M for 60 degrees N");
    }

    #[test]
    fn test_transform_bbox_simple_vs_densified() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::web_mercator();

        // Large bbox where densification matters
        let bbox = (-20.0, 40.0, 20.0, 70.0);

        let simple = DatasetRegistry::transform_bbox_simple(bbox, &source_crs, &target_crs);
        let densified =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);

        assert!(simple.is_ok());
        assert!(densified.is_ok());

        // Both should produce valid results
        let simple = simple.expect("simple should work");
        let densified = densified.expect("densified should work");

        // For Mercator projection, the results should be similar
        // The densified version may have slightly larger bounds due to curvature
        assert!(
            (simple.0 - densified.0).abs() < 100.0,
            "min_x should be similar"
        );
        assert!(
            (simple.2 - densified.2).abs() < 100.0,
            "max_x should be similar"
        );
    }

    // Transform point test
    #[test]
    fn test_transform_point() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::web_mercator();

        let result = DatasetRegistry::transform_point(0.0, 0.0, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (x, y) = result.expect("should transform");
        assert!((x - 0.0).abs() < 1.0, "x should be close to 0");
        assert!((y - 0.0).abs() < 1.0, "y should be close to 0");
    }

    #[test]
    fn test_transform_point_london() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::web_mercator();

        // London: -0.1276, 51.5074
        let result = DatasetRegistry::transform_point(-0.1276, 51.5074, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (x, y) = result.expect("should transform");
        // London in Web Mercator is approximately (-14200, 6711000)
        assert!(x > -20_000.0 && x < 0.0, "x should be slightly negative");
        assert!(
            y > 6_500_000.0 && y < 7_000_000.0,
            "y should be around 6.7M"
        );
    }

    // UTM zone transformation tests
    #[test]
    fn test_transform_bbox_to_utm() {
        let source_crs = Crs::wgs84();
        // UTM Zone 32N (Central Europe, 6-12 degrees E)
        let target_crs = Crs::from_epsg(32632).expect("UTM 32N should exist");

        // Bbox in Germany (within UTM zone 32)
        let bbox = (8.0, 48.0, 10.0, 50.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (min_x, min_y, _max_x, _max_y) = result.expect("should transform");

        // UTM coordinates should be in typical range
        // X (easting): 166,000 to 833,000 meters from false easting of 500,000
        // Y (northing): meters from equator
        assert!(
            min_x > 300_000.0 && min_x < 700_000.0,
            "easting should be in valid range"
        );
        assert!(
            min_y > 5_000_000.0 && min_y < 6_000_000.0,
            "northing should be in valid range"
        );
    }

    // Edge case tests
    #[test]
    fn test_transform_bbox_antimeridian() {
        let source_crs = Crs::wgs84();
        let target_crs = Crs::web_mercator();

        // Bbox near antimeridian (but not crossing)
        let bbox = (170.0, -10.0, 179.0, 10.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());

        let (min_x, _min_y, max_x, _max_y) = result.expect("should transform");
        assert!(min_x > 0.0 && max_x > min_x, "should have valid x range");
    }

    #[test]
    fn test_transform_bbox_polar_region() {
        let source_crs = Crs::wgs84();
        // Polar Stereographic North
        let target_crs = Crs::from_epsg(3413).expect("NSIDC Polar Stereographic should exist");

        // Arctic region bbox
        let bbox = (-10.0, 70.0, 10.0, 80.0);

        let result =
            DatasetRegistry::transform_bbox_with_densification(bbox, &source_crs, &target_crs);
        assert!(result.is_ok());
    }

    // Error handling tests
    #[test]
    fn test_transform_bbox_invalid_crs() {
        // Try to parse a non-existent EPSG code
        let crs = DatasetRegistry::parse_crs("EPSG:99999");
        assert!(crs.is_err());
    }
}
