//! Server configuration management
//!
//! This module handles configuration from multiple sources:
//! - TOML configuration files
//! - Environment variables
//! - Command-line arguments
//!
//! # Example Configuration
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 8080
//! workers = 4
//!
//! [cache]
//! memory_size_mb = 256
//! disk_cache = "/tmp/oxigdal-cache"
//! ttl_seconds = 3600
//!
//! [[layers]]
//! name = "landsat"
//! path = "/data/landsat.tif"
//! formats = ["png", "jpeg", "webp"]
//! tile_size = 256
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

/// Configuration errors
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Invalid configuration value
    #[error("Invalid configuration: {0}")]
    Invalid(String),

    /// Configuration file I/O error
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error
    #[error("Failed to parse TOML: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Layer not found
    #[error("Layer not found: {0}")]
    LayerNotFound(String),
}

/// Result type for configuration operations
pub type ConfigResult<T> = Result<T, ConfigError>;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host address to bind to
    #[serde(default = "default_host")]
    pub host: IpAddr,

    /// Port to bind to
    #[serde(default = "default_port")]
    pub port: u16,

    /// Number of worker threads (0 = number of CPUs)
    #[serde(default = "default_workers")]
    pub workers: usize,

    /// Maximum request size in bytes
    #[serde(default = "default_max_request_size")]
    pub max_request_size: usize,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,

    /// Enable CORS
    #[serde(default = "default_cors")]
    pub enable_cors: bool,

    /// Allowed CORS origins (empty = all)
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// In-memory cache size in megabytes
    #[serde(default = "default_memory_cache_mb")]
    pub memory_size_mb: usize,

    /// Optional disk cache directory
    #[serde(default)]
    pub disk_cache: Option<PathBuf>,

    /// Time-to-live for cached tiles in seconds
    #[serde(default = "default_ttl_seconds")]
    pub ttl_seconds: u64,

    /// Enable cache statistics
    #[serde(default = "default_enable_stats")]
    pub enable_stats: bool,

    /// Cache compression (gzip tiles in memory)
    #[serde(default = "default_compression")]
    pub compression: bool,
}

/// Layer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerConfig {
    /// Layer name (used in URLs)
    pub name: String,

    /// Display title
    #[serde(default)]
    pub title: Option<String>,

    /// Layer description
    #[serde(default)]
    pub abstract_: Option<String>,

    /// Path to dataset file
    pub path: PathBuf,

    /// Supported output formats
    #[serde(default = "default_formats")]
    pub formats: Vec<ImageFormat>,

    /// Tile size in pixels
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,

    /// Minimum zoom level
    #[serde(default)]
    pub min_zoom: u8,

    /// Maximum zoom level
    #[serde(default = "default_max_zoom")]
    pub max_zoom: u8,

    /// Supported tile matrix sets
    #[serde(default = "default_tile_matrix_sets")]
    pub tile_matrix_sets: Vec<String>,

    /// Optional style configuration
    #[serde(default)]
    pub style: Option<StyleConfig>,

    /// Layer-specific metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// Enable this layer
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Style configuration for rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleConfig {
    /// Style name
    pub name: String,

    /// Colormap name (e.g., "viridis", "terrain")
    #[serde(default)]
    pub colormap: Option<String>,

    /// Value range for colormap
    #[serde(default)]
    pub value_range: Option<(f64, f64)>,

    /// Alpha/transparency value (0.0-1.0)
    #[serde(default = "default_alpha")]
    pub alpha: f32,

    /// Gamma correction
    #[serde(default = "default_gamma")]
    pub gamma: f32,

    /// Brightness adjustment (-1.0 to 1.0)
    #[serde(default)]
    pub brightness: f32,

    /// Contrast adjustment (0.0 to 2.0)
    #[serde(default = "default_contrast")]
    pub contrast: f32,
}

/// Image format enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// PNG format
    Png,
    /// JPEG format
    Jpeg,
    /// WebP format
    Webp,
    /// GeoTIFF format
    Geotiff,
}

impl ImageFormat {
    /// Get MIME type for this format
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Webp => "image/webp",
            ImageFormat::Geotiff => "image/tiff",
        }
    }

    /// Get file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
            ImageFormat::Geotiff => "tif",
        }
    }
}

/// Error type for parsing image format
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseImageFormatError(String);

impl std::fmt::Display for ParseImageFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown image format: {}", self.0)
    }
}

impl std::error::Error for ParseImageFormatError {}

impl FromStr for ImageFormat {
    type Err = ParseImageFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "png" => Ok(ImageFormat::Png),
            "jpeg" | "jpg" => Ok(ImageFormat::Jpeg),
            "webp" => Ok(ImageFormat::Webp),
            "geotiff" | "tif" | "tiff" => Ok(ImageFormat::Geotiff),
            _ => Err(ParseImageFormatError(s.to_string())),
        }
    }
}

/// Complete server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Server settings
    #[serde(default)]
    pub server: ServerConfig,

    /// Cache settings
    #[serde(default)]
    pub cache: CacheConfig,

    /// Layer definitions
    #[serde(default)]
    pub layers: Vec<LayerConfig>,

    /// Global metadata
    #[serde(default)]
    pub metadata: MetadataConfig,
}

/// Service metadata configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataConfig {
    /// Service title
    #[serde(default = "default_service_title")]
    pub title: String,

    /// Service abstract/description
    #[serde(default = "default_service_abstract")]
    pub abstract_: String,

    /// Contact information
    #[serde(default)]
    pub contact: Option<ContactInfo>,

    /// Keywords
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Online resource URL
    #[serde(default)]
    pub online_resource: Option<String>,
}

/// Contact information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    /// Organization name
    pub organization: String,

    /// Contact person
    #[serde(default)]
    pub person: Option<String>,

    /// Email address
    #[serde(default)]
    pub email: Option<String>,

    /// Phone number
    #[serde(default)]
    pub phone: Option<String>,
}

// Default value functions
fn default_host() -> IpAddr {
    IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0))
}

fn default_port() -> u16 {
    8080
}

fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .ok()
        .unwrap_or(4)
}

fn default_max_request_size() -> usize {
    10 * 1024 * 1024 // 10 MB
}

fn default_timeout() -> u64 {
    30
}

fn default_cors() -> bool {
    true
}

fn default_memory_cache_mb() -> usize {
    256
}

fn default_ttl_seconds() -> u64 {
    3600
}

fn default_enable_stats() -> bool {
    true
}

fn default_compression() -> bool {
    false
}

fn default_formats() -> Vec<ImageFormat> {
    vec![ImageFormat::Png, ImageFormat::Jpeg]
}

fn default_tile_size() -> u32 {
    256
}

fn default_max_zoom() -> u8 {
    18
}

fn default_tile_matrix_sets() -> Vec<String> {
    vec!["WebMercatorQuad".to_string(), "WorldCRS84Quad".to_string()]
}

fn default_enabled() -> bool {
    true
}

fn default_alpha() -> f32 {
    1.0
}

fn default_gamma() -> f32 {
    1.0
}

fn default_contrast() -> f32 {
    1.0
}

fn default_service_title() -> String {
    "OxiGDAL Tile Server".to_string()
}

fn default_service_abstract() -> String {
    "WMS/WMTS tile server powered by OxiGDAL".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            workers: default_workers(),
            max_request_size: default_max_request_size(),
            timeout_seconds: default_timeout(),
            enable_cors: default_cors(),
            cors_origins: Vec::new(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            memory_size_mb: default_memory_cache_mb(),
            disk_cache: None,
            ttl_seconds: default_ttl_seconds(),
            enable_stats: default_enable_stats(),
            compression: default_compression(),
        }
    }
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            title: default_service_title(),
            abstract_: default_service_abstract(),
            contact: None,
            keywords: Vec::new(),
            online_resource: None,
        }
    }
}

impl Config {
    /// Load configuration from TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_toml(&contents)
    }

    /// Parse configuration from TOML string
    pub fn from_toml(toml: &str) -> ConfigResult<Self> {
        let config: Config = toml::from_str(toml)?;
        config.validate()?;
        Ok(config)
    }

    /// Create a default configuration
    pub fn default_config() -> Self {
        Self {
            server: ServerConfig::default(),
            cache: CacheConfig::default(),
            layers: Vec::new(),
            metadata: MetadataConfig::default(),
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> ConfigResult<()> {
        // Check for duplicate layer names
        let mut names = std::collections::HashSet::new();
        for layer in &self.layers {
            if !names.insert(&layer.name) {
                return Err(ConfigError::Invalid(format!(
                    "Duplicate layer name: {}",
                    layer.name
                )));
            }

            // Validate layer path exists
            if !layer.path.exists() {
                return Err(ConfigError::Invalid(format!(
                    "Layer path does not exist: {}",
                    layer.path.display()
                )));
            }

            // Validate tile size is power of 2
            if !layer.tile_size.is_power_of_two() {
                return Err(ConfigError::Invalid(format!(
                    "Tile size must be power of 2, got {}",
                    layer.tile_size
                )));
            }

            // Validate zoom levels
            if layer.min_zoom > layer.max_zoom {
                return Err(ConfigError::Invalid(format!(
                    "min_zoom ({}) cannot be greater than max_zoom ({})",
                    layer.min_zoom, layer.max_zoom
                )));
            }
        }

        // Validate cache settings
        if self.cache.memory_size_mb == 0 {
            return Err(ConfigError::Invalid(
                "Cache memory size must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Get a layer by name
    pub fn get_layer(&self, name: &str) -> ConfigResult<&LayerConfig> {
        self.layers
            .iter()
            .find(|l| l.name == name && l.enabled)
            .ok_or_else(|| ConfigError::LayerNotFound(name.to_string()))
    }

    /// Get all enabled layers
    pub fn enabled_layers(&self) -> impl Iterator<Item = &LayerConfig> {
        self.layers.iter().filter(|l| l.enabled)
    }

    /// Get server bind address
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default_config();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.cache.memory_size_mb, 256);
        assert!(config.layers.is_empty());
    }

    #[test]
    fn test_image_format_mime_types() {
        assert_eq!(ImageFormat::Png.mime_type(), "image/png");
        assert_eq!(ImageFormat::Jpeg.mime_type(), "image/jpeg");
        assert_eq!(ImageFormat::Webp.mime_type(), "image/webp");
        assert_eq!(ImageFormat::Geotiff.mime_type(), "image/tiff");
    }

    #[test]
    fn test_image_format_from_str() {
        assert_eq!("png".parse::<ImageFormat>().ok(), Some(ImageFormat::Png));
        assert_eq!("PNG".parse::<ImageFormat>().ok(), Some(ImageFormat::Png));
        assert_eq!("jpeg".parse::<ImageFormat>().ok(), Some(ImageFormat::Jpeg));
        assert_eq!("jpg".parse::<ImageFormat>().ok(), Some(ImageFormat::Jpeg));
        assert_eq!("webp".parse::<ImageFormat>().ok(), Some(ImageFormat::Webp));
        assert_eq!(
            "geotiff".parse::<ImageFormat>().ok(),
            Some(ImageFormat::Geotiff)
        );
        assert!("invalid".parse::<ImageFormat>().is_err());
    }

    #[test]
    fn test_config_from_toml() {
        let toml = r#"
            [server]
            host = "127.0.0.1"
            port = 9000
            workers = 8

            [cache]
            memory_size_mb = 512
            ttl_seconds = 7200

            [metadata]
            title = "Test Server"
        "#;

        let config = Config::from_toml(toml).expect("valid config");
        assert_eq!(config.server.host.to_string(), "127.0.0.1");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.workers, 8);
        assert_eq!(config.cache.memory_size_mb, 512);
        assert_eq!(config.metadata.title, "Test Server");
    }

    #[test]
    fn test_bind_address() {
        let config = Config::default_config();
        assert_eq!(config.bind_address(), "0.0.0.0:8080");
    }
}
