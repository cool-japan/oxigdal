//! File inspection utilities
//!
//! This module provides tools for inspecting and analyzing geospatial file formats.

use crate::{DevToolsError, Result};
use colored::Colorize;
use comfy_table::{Cell, Row, Table};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// File inspector
#[derive(Debug, Clone)]
pub struct FileInspector {
    /// File path
    path: PathBuf,
    /// File information
    info: FileInfo,
}

/// File information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    /// File path
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// File extension
    pub extension: Option<String>,
    /// Detected format
    pub format: Option<FileFormat>,
    /// Is readable
    pub readable: bool,
    /// Is writable
    pub writable: bool,
}

/// File format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFormat {
    /// GeoTIFF
    GeoTiff,
    /// GeoJSON
    GeoJson,
    /// Shapefile
    Shapefile,
    /// Zarr
    Zarr,
    /// NetCDF
    NetCdf,
    /// HDF5
    Hdf5,
    /// GeoParquet
    GeoParquet,
    /// FlatGeobuf
    FlatGeobuf,
    /// Unknown format
    Unknown,
}

impl FileFormat {
    /// Get format description
    pub fn description(&self) -> &str {
        match self {
            Self::GeoTiff => "GeoTIFF raster format",
            Self::GeoJson => "GeoJSON vector format",
            Self::Shapefile => "ESRI Shapefile",
            Self::Zarr => "Zarr array storage",
            Self::NetCdf => "NetCDF scientific data",
            Self::Hdf5 => "HDF5 hierarchical data",
            Self::GeoParquet => "GeoParquet columnar format",
            Self::FlatGeobuf => "FlatGeobuf binary format",
            Self::Unknown => "Unknown format",
        }
    }
}

impl FileInspector {
    /// Create a new file inspector
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if !path.exists() {
            return Err(DevToolsError::Inspector(format!(
                "File does not exist: {}",
                path.display()
            )));
        }

        let metadata = std::fs::metadata(&path)?;
        let size = metadata.len();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        let format = Self::detect_format(&path, extension.as_deref())?;

        let info = FileInfo {
            path: path.display().to_string(),
            size,
            extension,
            format: Some(format),
            readable: metadata.permissions().readonly(),
            writable: !metadata.permissions().readonly(),
        };

        Ok(Self { path, info })
    }

    /// Detect file format
    fn detect_format(path: &Path, extension: Option<&str>) -> Result<FileFormat> {
        // First try by extension
        if let Some(ext) = extension {
            match ext.to_lowercase().as_str() {
                "tif" | "tiff" | "gtiff" => return Ok(FileFormat::GeoTiff),
                "json" | "geojson" => return Ok(FileFormat::GeoJson),
                "shp" => return Ok(FileFormat::Shapefile),
                "zarr" => return Ok(FileFormat::Zarr),
                "nc" | "nc4" => return Ok(FileFormat::NetCdf),
                "h5" | "hdf5" => return Ok(FileFormat::Hdf5),
                "parquet" | "geoparquet" => return Ok(FileFormat::GeoParquet),
                "fgb" => return Ok(FileFormat::FlatGeobuf),
                _ => {}
            }
        }

        // Try by magic bytes
        if let Ok(mut file) = std::fs::File::open(path) {
            use std::io::Read;
            let mut magic = [0u8; 8];
            if file.read_exact(&mut magic).is_ok() {
                // GeoTIFF: II or MM (little/big endian TIFF)
                if magic[0..2] == [0x49, 0x49] || magic[0..2] == [0x4D, 0x4D] {
                    return Ok(FileFormat::GeoTiff);
                }
                // GeoJSON: starts with '{'
                if magic[0] == b'{' {
                    return Ok(FileFormat::GeoJson);
                }
                // HDF5: magic number
                if magic[0..4] == [0x89, 0x48, 0x44, 0x46] {
                    return Ok(FileFormat::Hdf5);
                }
            }
        }

        Ok(FileFormat::Unknown)
    }

    /// Get file info
    pub fn info(&self) -> &FileInfo {
        &self.info
    }

    /// Get file path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Generate summary report
    pub fn summary(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("\n{}\n", "File Inspection".bold()));
        report.push_str(&format!("{}\n\n", "=".repeat(60)));

        let mut table = Table::new();
        table.add_row(Row::from(vec![
            Cell::new("Path"),
            Cell::new(&self.info.path),
        ]));
        table.add_row(Row::from(vec![
            Cell::new("Size"),
            Cell::new(format_size(self.info.size)),
        ]));
        if let Some(ref ext) = self.info.extension {
            table.add_row(Row::from(vec![Cell::new("Extension"), Cell::new(ext)]));
        }
        if let Some(format) = self.info.format {
            table.add_row(Row::from(vec![
                Cell::new("Format"),
                Cell::new(format!("{:?}", format)),
            ]));
            table.add_row(Row::from(vec![
                Cell::new("Description"),
                Cell::new(format.description()),
            ]));
        }
        table.add_row(Row::from(vec![
            Cell::new("Readable"),
            Cell::new(if self.info.readable { "Yes" } else { "No" }),
        ]));
        table.add_row(Row::from(vec![
            Cell::new("Writable"),
            Cell::new(if self.info.writable { "Yes" } else { "No" }),
        ]));

        report.push_str(&table.to_string());
        report.push('\n');

        report
    }

    /// Export info as JSON
    pub fn export_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.info)?)
    }
}

/// Format file size
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.00 KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.00 MB");
    }

    #[test]
    fn test_file_inspector_creation() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"test data")?;

        let inspector = FileInspector::new(temp_file.path())?;
        assert_eq!(inspector.info().size, 9);

        Ok(())
    }

    #[test]
    fn test_file_inspector_nonexistent() {
        let result = FileInspector::new("/nonexistent/file.tif");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_detection_by_extension() -> Result<()> {
        let mut temp_file = NamedTempFile::with_suffix(".tif")?;
        temp_file.write_all(b"II\x2a\x00")?; // TIFF magic bytes

        let inspector = FileInspector::new(temp_file.path())?;
        assert_eq!(inspector.info().format, Some(FileFormat::GeoTiff));

        Ok(())
    }

    #[test]
    fn test_file_info_export() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"test")?;

        let inspector = FileInspector::new(temp_file.path())?;
        let json = inspector.export_json()?;
        assert!(json.contains("path"));
        assert!(json.contains("size"));

        Ok(())
    }
}
