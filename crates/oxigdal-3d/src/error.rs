//! Error types for OxiGDAL 3D operations

use std::io;
use thiserror::Error;

/// Result type for 3D operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for 3D visualization and point cloud operations
#[derive(Error, Debug)]
pub enum Error {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// LAS/LAZ format error
    #[error("LAS/LAZ error: {0}")]
    Las(String),

    /// LAZ compression error
    #[error("LAZ compression error: {0}")]
    LazCompression(String),

    /// Point format error
    #[error("Unsupported point format: {0}")]
    UnsupportedPointFormat(u8),

    /// COPC (Cloud Optimized Point Cloud) error
    #[error("COPC error: {0}")]
    Copc(String),

    /// EPT (Entwine Point Tiles) error
    #[error("EPT error: {0}")]
    Ept(String),

    /// Mesh format error
    #[error("Mesh format error: {0}")]
    MeshFormat(String),

    /// OBJ export error
    #[error("OBJ export error: {0}")]
    ObjExport(String),

    /// glTF/GLB error
    #[error("glTF/GLB error: {0}")]
    Gltf(String),

    /// glTF JSON error
    #[error("glTF JSON error: {0}")]
    GltfJson(String),

    /// TIN (Triangulated Irregular Network) error
    #[error("TIN error: {0}")]
    Tin(String),

    /// Triangulation error
    #[error("Triangulation error: {0}")]
    Triangulation(String),

    /// DEM to mesh conversion error
    #[error("DEM to mesh conversion error: {0}")]
    DemToMesh(String),

    /// 3D Tiles error
    #[error("3D Tiles error: {0}")]
    Tiles3d(String),

    /// Tileset JSON error
    #[error("Tileset JSON error: {0}")]
    TilesetJson(String),

    /// B3DM (Batched 3D Model) error
    #[error("B3DM error: {0}")]
    B3dm(String),

    /// PNTS (Point Cloud) tile error
    #[error("PNTS error: {0}")]
    Pnts(String),

    /// Classification error
    #[error("Classification error: {0}")]
    Classification(String),

    /// Ground classification error
    #[error("Ground classification error: {0}")]
    GroundClassification(String),

    /// Spatial indexing error
    #[error("Spatial index error: {0}")]
    SpatialIndex(String),

    /// Invalid bounds
    #[error("Invalid bounds: {0}")]
    InvalidBounds(String),

    /// Invalid geometry
    #[error("Invalid geometry: {0}")]
    InvalidGeometry(String),

    /// Empty dataset
    #[error("Empty dataset: {0}")]
    EmptyDataset(String),

    /// Invalid point count
    #[error("Invalid point count: expected {expected}, got {actual}")]
    InvalidPointCount {
        /// Expected number of points
        expected: usize,
        /// Actual number of points
        actual: usize,
    },

    /// Invalid triangle count
    #[error("Invalid triangle count: {0}")]
    InvalidTriangleCount(usize),

    /// Invalid mesh
    #[error("Invalid mesh: {0}")]
    InvalidMesh(String),

    /// Missing texture
    #[error("Missing texture: {0}")]
    MissingTexture(String),

    /// Invalid texture coordinates
    #[error("Invalid texture coordinates: {0}")]
    InvalidTextureCoords(String),

    /// HTTP request error (for COPC, EPT)
    #[error("HTTP error: {0}")]
    Http(String),

    /// Range request error
    #[error("Range request error: {0}")]
    RangeRequest(String),

    /// JSON parsing error
    #[error("JSON error: {0}")]
    Json(String),

    /// UTF-8 encoding error
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// Base64 decode error
    #[error("Base64 decode error: {0}")]
    Base64Decode(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Memory allocation error
    #[error("Memory allocation error: {0}")]
    MemoryAllocation(String),

    /// Octree error
    #[error("Octree error: {0}")]
    Octree(String),

    /// Hierarchical LOD error
    #[error("Hierarchical LOD error: {0}")]
    HierarchicalLod(String),

    /// Tile loading error
    #[error("Tile loading error: {0}")]
    TileLoading(String),

    /// Metadata error
    #[error("Metadata error: {0}")]
    Metadata(String),

    /// Invalid header
    #[error("Invalid header: {0}")]
    InvalidHeader(String),

    /// Version mismatch
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch {
        /// Expected version
        expected: String,
        /// Actual version
        actual: String,
    },

    /// Feature not supported
    #[error("Feature not supported: {0}")]
    Unsupported(String),

    /// OxiGDAL core error
    #[error("OxiGDAL core error: {0}")]
    Core(String),
}

// Conversion from las crate errors
impl From<las::Error> for Error {
    fn from(err: las::Error) -> Self {
        Error::Las(err.to_string())
    }
}

// Conversion from gltf errors
impl From<gltf::Error> for Error {
    fn from(err: gltf::Error) -> Self {
        Error::Gltf(err.to_string())
    }
}

// Note: gltf_json::Error is an alias for serde_json::Error
// so we only need the serde_json conversion

// Conversion from base64 decode errors
impl From<base64::DecodeError> for Error {
    fn from(err: base64::DecodeError) -> Self {
        Error::Base64Decode(err.to_string())
    }
}

// Conversion from serde_json errors
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err.to_string())
    }
}

#[cfg(feature = "async")]
impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Http(err.to_string())
    }
}

impl From<oxigdal_core::error::OxiGdalError> for Error {
    fn from(err: oxigdal_core::error::OxiGdalError) -> Self {
        Error::Core(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::Las("test error".to_string());
        assert_eq!(err.to_string(), "LAS/LAZ error: test error");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid}")
            .expect_err("Should fail to parse invalid JSON");
        let err: Error = json_err.into();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn test_invalid_point_count() {
        let err = Error::InvalidPointCount {
            expected: 100,
            actual: 50,
        };
        assert_eq!(err.to_string(), "Invalid point count: expected 100, got 50");
    }

    #[test]
    fn test_version_mismatch() {
        let err = Error::VersionMismatch {
            expected: "1.4".to_string(),
            actual: "1.2".to_string(),
        };
        assert_eq!(err.to_string(), "Version mismatch: expected 1.4, got 1.2");
    }
}
