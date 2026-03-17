//! Shared types for the OxiGDAL WASM Component Model interface.
//!
//! These types are designed to be transferable across the WASM boundary
//! and are compatible with the wasm32-wasip2 Component Model ABI.

/// Geographic bounding box in the source CRS coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentBbox {
    /// Minimum X (longitude or easting)
    pub min_x: f64,
    /// Minimum Y (latitude or northing)
    pub min_y: f64,
    /// Maximum X (longitude or easting)
    pub max_x: f64,
    /// Maximum Y (latitude or northing)
    pub max_y: f64,
}

impl ComponentBbox {
    /// Create a new bounding box.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Width of the bounding box in CRS units.
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Height of the bounding box in CRS units.
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Area of the bounding box in CRS units².
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Center point of the bounding box.
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }

    /// Returns `true` if the point (x, y) is inside or on the boundary.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Returns `true` if `self` intersects `other` (inclusive of touching edges).
    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Expand this bbox to include `other`.
    pub fn union(&self, other: &Self) -> Self {
        Self::new(
            self.min_x.min(other.min_x),
            self.min_y.min(other.min_y),
            self.max_x.max(other.max_x),
            self.max_y.max(other.max_y),
        )
    }
}

/// WASM-compatible raster data type (mirrors Apache Arrow primitive types).
///
/// The `repr(u8)` ensures a stable ABI across the component boundary.
#[derive(Debug, Clone, PartialEq)]
#[repr(u8)]
pub enum ComponentDataType {
    /// Unsigned 8-bit integer
    Uint8 = 0,
    /// Unsigned 16-bit integer
    Uint16 = 1,
    /// Unsigned 32-bit integer
    Uint32 = 2,
    /// Signed 8-bit integer
    Int8 = 3,
    /// Signed 16-bit integer
    Int16 = 4,
    /// Signed 32-bit integer
    Int32 = 5,
    /// 32-bit IEEE 754 float
    Float32 = 6,
    /// 64-bit IEEE 754 float
    Float64 = 7,
}

impl ComponentDataType {
    /// Size of a single value in bytes.
    pub fn byte_size(&self) -> usize {
        match self {
            Self::Uint8 | Self::Int8 => 1,
            Self::Uint16 | Self::Int16 => 2,
            Self::Uint32 | Self::Int32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }

    /// Construct from a raw discriminant. Returns `None` for unknown values.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Uint8),
            1 => Some(Self::Uint16),
            2 => Some(Self::Uint32),
            3 => Some(Self::Int8),
            4 => Some(Self::Int16),
            5 => Some(Self::Int32),
            6 => Some(Self::Float32),
            7 => Some(Self::Float64),
            _ => None,
        }
    }

    /// Returns `true` for floating-point types.
    pub fn is_floating_point(&self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }

    /// Returns `true` for integer types.
    pub fn is_integer(&self) -> bool {
        !self.is_floating_point()
    }

    /// Returns `true` for signed types (signed integers and floats).
    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::Int8 | Self::Int16 | Self::Int32 | Self::Float32 | Self::Float64
        )
    }
}

/// Serialisable error type that can cross the WASM component boundary.
#[derive(Debug, Clone)]
pub struct ComponentError {
    /// Numeric error code (stable across versions).
    pub code: u32,
    /// Human-readable description.
    pub message: String,
    /// High-level error category.
    pub category: ErrorCategory,
}

/// High-level error category enum.
#[derive(Debug, Clone, PartialEq)]
#[repr(u8)]
pub enum ErrorCategory {
    /// I/O or network error
    Io = 0,
    /// Invalid user input or arguments
    InvalidInput = 1,
    /// The requested format is not supported
    UnsupportedFormat = 2,
    /// Allocation failure
    OutOfMemory = 3,
    /// Coordinate reference system or projection error
    Projection = 4,
    /// Internal / unexpected error
    Internal = 255,
}

impl ComponentError {
    /// Generic constructor.
    pub fn new(code: u32, message: impl Into<String>, category: ErrorCategory) -> Self {
        Self {
            code,
            message: message.into(),
            category,
        }
    }

    /// Convenience constructor for [`ErrorCategory::InvalidInput`].
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new(1, msg, ErrorCategory::InvalidInput)
    }

    /// Convenience constructor for [`ErrorCategory::UnsupportedFormat`].
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::new(2, msg, ErrorCategory::UnsupportedFormat)
    }

    /// Convenience constructor for [`ErrorCategory::Io`].
    pub fn io(msg: impl Into<String>) -> Self {
        Self::new(3, msg, ErrorCategory::Io)
    }

    /// Convenience constructor for [`ErrorCategory::OutOfMemory`].
    pub fn out_of_memory(msg: impl Into<String>) -> Self {
        Self::new(4, msg, ErrorCategory::OutOfMemory)
    }

    /// Convenience constructor for [`ErrorCategory::Projection`].
    pub fn projection(msg: impl Into<String>) -> Self {
        Self::new(5, msg, ErrorCategory::Projection)
    }

    /// Convenience constructor for [`ErrorCategory::Internal`].
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(255, msg, ErrorCategory::Internal)
    }
}

impl std::fmt::Display for ComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for ComponentError {}

/// Specialised `Result` type for Component Model operations.
pub type ComponentResult<T> = Result<T, ComponentError>;

/// Integer pixel coordinate (column, row).
#[derive(Debug, Clone, PartialEq)]
pub struct PixelCoord {
    /// Zero-based column index (x).
    pub col: u32,
    /// Zero-based row index (y).
    pub row: u32,
}

impl PixelCoord {
    /// Create a new pixel coordinate.
    pub fn new(col: u32, row: u32) -> Self {
        Self { col, row }
    }
}

/// Image dimensions: width × height with a band count.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageDimensions {
    /// Number of pixels horizontally.
    pub width: u32,
    /// Number of pixels vertically.
    pub height: u32,
    /// Number of spectral bands.
    pub bands: u32,
}

impl ImageDimensions {
    /// Create new image dimensions.
    pub fn new(width: u32, height: u32, bands: u32) -> Self {
        Self {
            width,
            height,
            bands,
        }
    }

    /// Total number of pixels in a single band.
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Byte size of a single band given a data type.
    pub fn band_size_bytes(&self, dtype: &ComponentDataType) -> u64 {
        self.pixel_count() * dtype.byte_size() as u64
    }

    /// Total byte size across all bands given a data type.
    pub fn total_size_bytes(&self, dtype: &ComponentDataType) -> u64 {
        self.band_size_bytes(dtype) * self.bands as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_basic() {
        let b = ComponentBbox::new(0.0, 0.0, 10.0, 5.0);
        assert_eq!(b.width(), 10.0);
        assert_eq!(b.height(), 5.0);
        assert_eq!(b.area(), 50.0);
        assert_eq!(b.center(), (5.0, 2.5));
    }

    #[test]
    fn bbox_contains() {
        let b = ComponentBbox::new(0.0, 0.0, 10.0, 10.0);
        assert!(b.contains(5.0, 5.0));
        assert!(!b.contains(11.0, 5.0));
    }

    #[test]
    fn dtype_byte_sizes() {
        assert_eq!(ComponentDataType::Uint8.byte_size(), 1);
        assert_eq!(ComponentDataType::Float64.byte_size(), 8);
    }

    #[test]
    fn dtype_from_u8_invalid() {
        assert!(ComponentDataType::from_u8(200).is_none());
    }

    #[test]
    fn image_dims_sizes() {
        let d = ImageDimensions::new(100, 100, 3);
        assert_eq!(d.pixel_count(), 10_000);
        assert_eq!(d.total_size_bytes(&ComponentDataType::Float32), 120_000);
    }
}
