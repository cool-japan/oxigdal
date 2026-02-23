//! Utility modules for CLI operations

pub mod parallel;
pub mod progress;
pub mod raster;

use std::path::Path;

/// Detect file format from extension
pub fn detect_format(path: &Path) -> Option<&'static str> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| match ext.to_lowercase().as_str() {
            "tif" | "tiff" => Some("GeoTIFF"),
            "json" | "geojson" => Some("GeoJSON"),
            "shp" => Some("Shapefile"),
            "fgb" => Some("FlatGeobuf"),
            "parquet" | "geoparquet" => Some("GeoParquet"),
            "zarr" => Some("Zarr"),
            _ => None,
        })
}

/// Format file size for human-readable output
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_format() {
        assert_eq!(detect_format(Path::new("test.tif")), Some("GeoTIFF"));
        assert_eq!(detect_format(Path::new("test.geojson")), Some("GeoJSON"));
        assert_eq!(detect_format(Path::new("test.shp")), Some("Shapefile"));
        assert_eq!(detect_format(Path::new("test.unknown")), None);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1_048_576), "1.00 MB");
    }
}
