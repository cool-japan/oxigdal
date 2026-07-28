//! Magic commands for Jupyter
//!
//! This module provides magic commands for common OxiGeo operations
//! that can be executed with the % prefix in Jupyter notebooks.

use crate::kernel::RasterHandle;
use crate::{JupyterError, Result};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::io::FileDataSource;
use oxigeo_geotiff::GeoTiffReader;
use std::collections::HashMap;
use std::path::PathBuf;

/// Magic command
#[derive(Debug, Clone)]
pub enum MagicCommand {
    /// Load a raster file: %load_raster `<path>` \[name\]
    LoadRaster {
        /// File path
        path: String,
        /// Variable name
        name: Option<String>,
    },
    /// Plot dataset: %plot `<dataset>` \[options\]
    Plot {
        /// Dataset name
        dataset: String,
        /// Plot options
        options: PlotOptions,
    },
    /// Show dataset info: %info `<dataset>`
    Info {
        /// Dataset name
        dataset: String,
    },
    /// Show CRS: %crs `<dataset>`
    Crs {
        /// Dataset name
        dataset: String,
    },
    /// Show bounds: %bounds `<dataset>`
    Bounds {
        /// Dataset name
        dataset: String,
    },
    /// Show statistics: %stats `<dataset>` \[band\]
    Stats {
        /// Dataset name
        dataset: String,
        /// Band number (optional)
        band: Option<usize>,
    },
    /// List loaded datasets: %list
    List,
    /// Clear namespace: %clear
    Clear,
}

/// Plot options
#[derive(Debug, Clone, Default)]
pub struct PlotOptions {
    /// Color map
    pub colormap: Option<String>,
    /// Band to plot
    pub band: Option<usize>,
    /// Width
    pub width: Option<u32>,
    /// Height
    pub height: Option<u32>,
}

impl MagicCommand {
    /// Parse magic command from string
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();

        if !input.starts_with('%') {
            return Err(JupyterError::Magic(
                "Magic command must start with %".to_string(),
            ));
        }

        let parts: Vec<&str> = input[1..].split_whitespace().collect();

        if parts.is_empty() {
            return Err(JupyterError::Magic("Empty magic command".to_string()));
        }

        let command = parts[0];
        let args = &parts[1..];

        match command {
            "load_raster" => {
                if args.is_empty() {
                    return Err(JupyterError::Magic(
                        "load_raster requires a path".to_string(),
                    ));
                }
                Ok(Self::LoadRaster {
                    path: args[0].to_string(),
                    name: args.get(1).map(|s| s.to_string()),
                })
            }
            "plot" => {
                if args.is_empty() {
                    return Err(JupyterError::Magic(
                        "plot requires a dataset name".to_string(),
                    ));
                }
                let dataset = args[0].to_string();
                let options = Self::parse_plot_options(&args[1..])?;
                Ok(Self::Plot { dataset, options })
            }
            "info" => {
                if args.is_empty() {
                    return Err(JupyterError::Magic(
                        "info requires a dataset name".to_string(),
                    ));
                }
                Ok(Self::Info {
                    dataset: args[0].to_string(),
                })
            }
            "crs" => {
                if args.is_empty() {
                    return Err(JupyterError::Magic(
                        "crs requires a dataset name".to_string(),
                    ));
                }
                Ok(Self::Crs {
                    dataset: args[0].to_string(),
                })
            }
            "bounds" => {
                if args.is_empty() {
                    return Err(JupyterError::Magic(
                        "bounds requires a dataset name".to_string(),
                    ));
                }
                Ok(Self::Bounds {
                    dataset: args[0].to_string(),
                })
            }
            "stats" => {
                if args.is_empty() {
                    return Err(JupyterError::Magic(
                        "stats requires a dataset name".to_string(),
                    ));
                }
                Ok(Self::Stats {
                    dataset: args[0].to_string(),
                    band: args.get(1).and_then(|s| s.parse().ok()),
                })
            }
            "list" => Ok(Self::List),
            "clear" => Ok(Self::Clear),
            _ => Err(JupyterError::Magic(format!(
                "Unknown magic command: {}",
                command
            ))),
        }
    }

    /// Parse plot options
    fn parse_plot_options(args: &[&str]) -> Result<PlotOptions> {
        let mut options = PlotOptions::default();

        let mut i = 0;
        while i < args.len() {
            match args[i] {
                "--colormap" | "-c" => {
                    if i + 1 >= args.len() {
                        return Err(JupyterError::Magic(
                            "--colormap requires a value".to_string(),
                        ));
                    }
                    options.colormap = Some(args[i + 1].to_string());
                    i += 2;
                }
                "--band" | "-b" => {
                    if i + 1 >= args.len() {
                        return Err(JupyterError::Magic("--band requires a value".to_string()));
                    }
                    options.band = args[i + 1].parse().ok();
                    i += 2;
                }
                "--width" | "-w" => {
                    if i + 1 >= args.len() {
                        return Err(JupyterError::Magic("--width requires a value".to_string()));
                    }
                    options.width = args[i + 1].parse().ok();
                    i += 2;
                }
                "--height" | "-h" => {
                    if i + 1 >= args.len() {
                        return Err(JupyterError::Magic("--height requires a value".to_string()));
                    }
                    options.height = args[i + 1].parse().ok();
                    i += 2;
                }
                _ => i += 1,
            }
        }

        Ok(options)
    }

    /// Execute magic command
    pub fn execute(
        &self,
        namespace: &mut HashMap<String, crate::kernel::Value>,
    ) -> Result<HashMap<String, String>> {
        use crate::kernel::Value;

        let mut output = HashMap::new();

        match self {
            Self::LoadRaster { path, name } => {
                let var_name = name.as_deref().unwrap_or("raster");

                let source = FileDataSource::open(path).map_err(|e| {
                    JupyterError::Magic(format!("Failed to open raster '{}': {}", path, e))
                })?;
                let reader = GeoTiffReader::open(source).map_err(|e| {
                    JupyterError::Magic(format!("Failed to parse GeoTIFF '{}': {}", path, e))
                })?;
                let metadata = reader.metadata();

                let summary = format!(
                    "Loaded raster from '{}' into '{}' ({}x{}, {} band(s), {:?}{})",
                    path,
                    var_name,
                    metadata.width,
                    metadata.height,
                    metadata.band_count,
                    metadata.data_type,
                    if metadata.crs_wkt.is_some() {
                        ""
                    } else {
                        ", no CRS"
                    },
                );

                namespace.insert(
                    var_name.to_string(),
                    Value::Raster(Box::new(RasterHandle {
                        path: PathBuf::from(path),
                        metadata,
                    })),
                );
                output.insert("text/plain".to_string(), summary);
            }
            Self::Plot { dataset, options } => {
                if !namespace.contains_key(dataset) {
                    return Err(JupyterError::Magic(format!(
                        "Dataset '{}' not found",
                        dataset
                    )));
                }
                let mut desc = format!("Plotting dataset '{}'", dataset);
                if let Some(ref cmap) = options.colormap {
                    desc.push_str(&format!(" with colormap '{}'", cmap));
                }
                if let Some(band) = options.band {
                    desc.push_str(&format!(", band {}", band));
                }
                output.insert("text/plain".to_string(), desc);
            }
            Self::Info { dataset } => {
                if !namespace.contains_key(dataset) {
                    return Err(JupyterError::Magic(format!(
                        "Dataset '{}' not found",
                        dataset
                    )));
                }
                output.insert(
                    "text/plain".to_string(),
                    format!(
                        "Dataset '{}' information:\n{:?}",
                        dataset,
                        namespace.get(dataset)
                    ),
                );
            }
            Self::Crs { dataset } => {
                let handle = Self::raster_handle(namespace, dataset)?;
                let text = match &handle.metadata.crs_wkt {
                    Some(wkt) => format!("CRS for '{}':\n{}", dataset, wkt),
                    None => format!(
                        "CRS for '{}': no CRS information present in the file",
                        dataset
                    ),
                };
                output.insert("text/plain".to_string(), text);
            }
            Self::Bounds { dataset } => {
                let handle = Self::raster_handle(namespace, dataset)?;
                let text = match handle.metadata.geo_transform {
                    Some(gt) => {
                        let bbox = gt.compute_bounds(handle.metadata.width, handle.metadata.height);
                        format!(
                            "Bounds for '{}': [{}, {}, {}, {}]",
                            dataset, bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y
                        )
                    }
                    None => format!(
                        "Bounds for '{}': no geotransform present in the file",
                        dataset
                    ),
                };
                output.insert("text/plain".to_string(), text);
            }
            Self::Stats { dataset, band } => {
                let handle = Self::raster_handle(namespace, dataset)?;
                let band_count = handle.metadata.band_count;
                if band_count == 0 {
                    return Err(JupyterError::Magic(format!(
                        "Dataset '{}' has no bands",
                        dataset
                    )));
                }
                let band_number = band.unwrap_or(1);
                if band_number == 0 || band_number as u32 > band_count {
                    return Err(JupyterError::Magic(format!(
                        "Band {} out of range for dataset '{}' ({} band(s), 1-indexed)",
                        band_number, dataset, band_count
                    )));
                }
                let band_index = band_number as u32 - 1;

                let source = FileDataSource::open(&handle.path).map_err(|e| {
                    JupyterError::Magic(format!(
                        "Failed to re-open raster '{}' for statistics: {}",
                        handle.path.display(),
                        e
                    ))
                })?;
                let reader = GeoTiffReader::open(source).map_err(|e| {
                    JupyterError::Magic(format!(
                        "Failed to parse GeoTIFF '{}' for statistics: {}",
                        handle.path.display(),
                        e
                    ))
                })?;
                let raw = reader.read_band(0, band_index as usize).map_err(|e| {
                    JupyterError::Magic(format!(
                        "Failed to read band data for '{}': {}",
                        dataset, e
                    ))
                })?;

                let bytes_per_sample = handle.metadata.data_type.size_bytes();
                let band_data = extract_interleaved_band(
                    &raw,
                    handle.metadata.width,
                    handle.metadata.height,
                    band_count,
                    band_index,
                    bytes_per_sample,
                    dataset,
                )?;

                let buffer = RasterBuffer::new(
                    band_data,
                    handle.metadata.width,
                    handle.metadata.height,
                    handle.metadata.data_type,
                    handle.metadata.nodata,
                )?;

                let stats =
                    oxigeo_algorithms::raster::compute_statistics(&buffer).map_err(|e| {
                        JupyterError::Magic(format!(
                            "Failed to compute statistics for '{}': {}",
                            dataset, e
                        ))
                    })?;

                output.insert(
                    "text/plain".to_string(),
                    format!(
                        "Statistics for '{}' band {}: count={} min={} max={} mean={} stddev={}",
                        dataset,
                        band_number,
                        stats.count,
                        stats.min,
                        stats.max,
                        stats.mean,
                        stats.stddev
                    ),
                );
            }
            Self::List => {
                let datasets: Vec<_> = namespace.keys().map(|k| k.as_str()).collect();
                output.insert(
                    "text/plain".to_string(),
                    if datasets.is_empty() {
                        "No datasets loaded".to_string()
                    } else {
                        format!("Loaded datasets: {}", datasets.join(", "))
                    },
                );
            }
            Self::Clear => {
                namespace.clear();
                output.insert("text/plain".to_string(), "Namespace cleared".to_string());
            }
        }

        Ok(output)
    }

    /// Looks up `dataset` in the namespace and returns its [`RasterHandle`],
    /// erroring honestly if the name is missing or bound to a non-raster
    /// value (e.g. a plain scalar assigned via `let`) instead of one loaded
    /// with `%load_raster`.
    fn raster_handle<'a>(
        namespace: &'a HashMap<String, crate::kernel::Value>,
        dataset: &str,
    ) -> Result<&'a RasterHandle> {
        match namespace.get(dataset) {
            Some(crate::kernel::Value::Raster(handle)) => Ok(handle),
            Some(_) => Err(JupyterError::Magic(format!(
                "'{}' is not a raster dataset; load one first with %load_raster",
                dataset
            ))),
            None => Err(JupyterError::Magic(format!(
                "Dataset '{}' not found",
                dataset
            ))),
        }
    }

    /// Get help text for this command
    pub fn help(&self) -> String {
        match self {
            Self::LoadRaster { .. } => {
                "Load a raster file\nUsage: %load_raster `<path>` [name]".to_string()
            }
            Self::Plot { .. } => {
                "Plot dataset\nUsage: %plot `<dataset>` [--colormap viridis] [--band 1]".to_string()
            }
            Self::Info { .. } => "Show dataset info\nUsage: %info `<dataset>`".to_string(),
            Self::Crs { .. } => "Show CRS\nUsage: %crs `<dataset>`".to_string(),
            Self::Bounds { .. } => "Show bounds\nUsage: %bounds `<dataset>`".to_string(),
            Self::Stats { .. } => "Show statistics\nUsage: %stats `<dataset>` [band]".to_string(),
            Self::List => "List loaded datasets\nUsage: %list".to_string(),
            Self::Clear => "Clear namespace\nUsage: %clear".to_string(),
        }
    }
}

/// Get help for all magic commands
pub fn all_magic_help() -> String {
    let commands = [
        (
            "%load_raster",
            "Load a raster file: %load_raster `<path>` [name]",
        ),
        (
            "%plot",
            "Plot dataset: %plot `<dataset>` [--colormap viridis] [--band 1]",
        ),
        ("%info", "Show dataset information: %info `<dataset>`"),
        ("%crs", "Show coordinate reference system: %crs `<dataset>`"),
        ("%bounds", "Show dataset bounds: %bounds `<dataset>`"),
        (
            "%stats",
            "Show raster statistics: %stats `<dataset>` [band]",
        ),
        ("%list", "List all loaded datasets: %list"),
        ("%clear", "Clear namespace: %clear"),
    ];

    let mut help = String::from("Available magic commands:\n\n");
    for (cmd, desc) in &commands {
        help.push_str(&format!("  {:<20} {}\n", cmd, desc));
    }
    help
}

/// Extracts a single band's samples out of pixel-interleaved (band-interleaved
/// by pixel) raster data, as returned by [`oxigeo_geotiff::GeoTiffReader::read_band`].
///
/// For each pixel the source buffer holds `band_count` consecutive samples of
/// `bytes_per_sample` bytes each; this copies out just the `band_index`-th
/// (0-based) sample of every pixel, preserving row-major pixel order.
fn extract_interleaved_band(
    data: &[u8],
    width: u64,
    height: u64,
    band_count: u32,
    band_index: u32,
    bytes_per_sample: usize,
    dataset: &str,
) -> Result<Vec<u8>> {
    let pixel_count = (width * height) as usize;
    let stride = bytes_per_sample * band_count as usize;
    let expected_len = pixel_count * stride;
    if data.len() < expected_len {
        return Err(JupyterError::Magic(format!(
            "Band data for '{}' is truncated: expected at least {} bytes, got {}",
            dataset,
            expected_len,
            data.len()
        )));
    }

    let mut out = Vec::with_capacity(pixel_count * bytes_per_sample);
    let band_offset = band_index as usize * bytes_per_sample;
    for pixel in 0..pixel_count {
        let base = pixel * stride + band_offset;
        out.extend_from_slice(&data[base..base + bytes_per_sample]);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_core::types::{GeoTransform, RasterDataType};
    use oxigeo_geotiff::tiff::{Compression, Predictor};
    use oxigeo_geotiff::{GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig};

    /// Writes a small (4x4) `band_count`-band Float32 GeoTIFF with a known
    /// north-up geotransform (origin (100, 50), 1.0 unit pixels) and
    /// EPSG:4326 CRS, for exercising the real `%crs`/`%bounds`/`%stats`
    /// pipeline end-to-end. `values` must be pixel-interleaved
    /// (band0, band1, ..., band0, band1, ...) with `width * height *
    /// band_count` entries. Returns the `NamedTempFile` guard (keep it
    /// alive for the test's duration; it removes the file on drop) plus
    /// the path as a `String`.
    fn write_test_raster_f32(
        band_count: u16,
        values: &[f32],
    ) -> Result<(tempfile::NamedTempFile, String)> {
        let width = 4u64;
        let height = 4u64;
        assert_eq!(values.len() as u64, width * height * u64::from(band_count));

        let file = tempfile::Builder::new()
            .prefix("oxigeo_jupyter_magic_test_")
            .suffix(".tif")
            .tempfile()
            .map_err(JupyterError::Io)?;
        let path = file.path().to_string_lossy().to_string();

        let geo_transform = GeoTransform::north_up(100.0, 50.0, 1.0, -1.0);
        let config = WriterConfig::new(width, height, band_count, RasterDataType::Float32)
            .with_compression(Compression::None)
            .with_predictor(Predictor::None)
            .with_tile_size(4, 4)
            .with_overviews(false, OverviewResampling::Nearest)
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())?;

        let mut data = Vec::with_capacity(values.len() * 4);
        for v in values {
            data.extend_from_slice(&v.to_le_bytes());
        }
        writer.write(&data)?;

        Ok((file, path))
    }

    /// Loads the raster at `path` into `ns` under `name` via the real
    /// `%load_raster` magic command (so tests exercise the same code path a
    /// notebook user would).
    fn load_raster_into(
        ns: &mut HashMap<String, crate::kernel::Value>,
        path: &str,
        name: &str,
    ) -> Result<()> {
        let cmd = MagicCommand::LoadRaster {
            path: path.to_string(),
            name: Some(name.to_string()),
        };
        cmd.execute(ns)?;
        Ok(())
    }

    #[test]
    fn test_parse_load_raster() -> Result<()> {
        let cmd = MagicCommand::parse("%load_raster /path/to/file.tif")?;
        assert!(
            matches!(&cmd, MagicCommand::LoadRaster { .. }),
            "Expected LoadRaster command"
        );
        if let MagicCommand::LoadRaster { path, name } = cmd {
            assert_eq!(path, "/path/to/file.tif");
            assert!(name.is_none());
        }
        Ok(())
    }

    #[test]
    fn test_parse_load_raster_with_name() -> Result<()> {
        let cmd = MagicCommand::parse("%load_raster /path/to/file.tif my_raster")?;
        assert!(
            matches!(&cmd, MagicCommand::LoadRaster { .. }),
            "Expected LoadRaster command"
        );
        if let MagicCommand::LoadRaster { path, name } = cmd {
            assert_eq!(path, "/path/to/file.tif");
            assert_eq!(name.as_deref(), Some("my_raster"));
        }
        Ok(())
    }

    #[test]
    fn test_parse_plot() -> Result<()> {
        let cmd = MagicCommand::parse("%plot my_raster --colormap viridis --band 1")?;
        assert!(
            matches!(&cmd, MagicCommand::Plot { .. }),
            "Expected Plot command"
        );
        if let MagicCommand::Plot { dataset, options } = cmd {
            assert_eq!(dataset, "my_raster");
            assert_eq!(options.colormap.as_deref(), Some("viridis"));
            assert_eq!(options.band, Some(1));
        }
        Ok(())
    }

    #[test]
    fn test_parse_info() -> Result<()> {
        let cmd = MagicCommand::parse("%info my_raster")?;
        assert!(
            matches!(&cmd, MagicCommand::Info { .. }),
            "Expected Info command"
        );
        if let MagicCommand::Info { dataset } = cmd {
            assert_eq!(dataset, "my_raster");
        }
        Ok(())
    }

    #[test]
    fn test_parse_list() -> Result<()> {
        let cmd = MagicCommand::parse("%list")?;
        assert!(matches!(cmd, MagicCommand::List));
        Ok(())
    }

    #[test]
    fn test_parse_clear() -> Result<()> {
        let cmd = MagicCommand::parse("%clear")?;
        assert!(matches!(cmd, MagicCommand::Clear));
        Ok(())
    }

    #[test]
    fn test_invalid_magic() {
        let result = MagicCommand::parse("not_a_magic");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_magic() {
        let result = MagicCommand::parse("%unknown");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_list() -> Result<()> {
        use crate::kernel::Value;
        let mut namespace = HashMap::new();
        namespace.insert("raster1".to_string(), Value::Integer(1));

        let cmd = MagicCommand::List;
        let output = cmd.execute(&mut namespace)?;

        let text = output.get("text/plain").map(|s| s.as_str());
        assert!(text.is_some());
        assert!(text.unwrap_or_default().contains("raster1"));
        Ok(())
    }

    #[test]
    fn test_execute_clear() -> Result<()> {
        use crate::kernel::Value;
        let mut namespace = HashMap::new();
        namespace.insert("raster1".to_string(), Value::Integer(1));

        let cmd = MagicCommand::Clear;
        cmd.execute(&mut namespace)?;

        assert!(namespace.is_empty());
        Ok(())
    }

    #[test]
    fn test_all_magic_help() {
        let help = all_magic_help();
        assert!(help.contains("%load_raster"));
        assert!(help.contains("%plot"));
        assert!(help.contains("%info"));
    }

    #[test]
    fn test_parse_crs_command() -> Result<()> {
        let cmd = MagicCommand::parse("%crs my_data")?;
        assert!(matches!(&cmd, MagicCommand::Crs { .. }));
        if let MagicCommand::Crs { dataset } = cmd {
            assert_eq!(dataset, "my_data");
        }
        Ok(())
    }

    #[test]
    fn test_parse_bounds_command() -> Result<()> {
        let cmd = MagicCommand::parse("%bounds my_data")?;
        assert!(matches!(&cmd, MagicCommand::Bounds { .. }));
        if let MagicCommand::Bounds { dataset } = cmd {
            assert_eq!(dataset, "my_data");
        }
        Ok(())
    }

    #[test]
    fn test_parse_stats_with_band() -> Result<()> {
        let cmd = MagicCommand::parse("%stats my_data 2")?;
        if let MagicCommand::Stats { dataset, band } = cmd {
            assert_eq!(dataset, "my_data");
            assert_eq!(band, Some(2));
        }
        Ok(())
    }

    #[test]
    fn test_parse_stats_without_band() -> Result<()> {
        let cmd = MagicCommand::parse("%stats my_data")?;
        if let MagicCommand::Stats { dataset, band } = cmd {
            assert_eq!(dataset, "my_data");
            assert!(band.is_none());
        }
        Ok(())
    }

    #[test]
    fn test_parse_plot_with_dimensions() -> Result<()> {
        let cmd = MagicCommand::parse("%plot ds --width 800 --height 600")?;
        if let MagicCommand::Plot { dataset, options } = cmd {
            assert_eq!(dataset, "ds");
            assert_eq!(options.width, Some(800));
            assert_eq!(options.height, Some(600));
        }
        Ok(())
    }

    #[test]
    fn test_parse_plot_short_flags() -> Result<()> {
        let cmd = MagicCommand::parse("%plot ds -c plasma -b 3")?;
        if let MagicCommand::Plot { options, .. } = cmd {
            assert_eq!(options.colormap.as_deref(), Some("plasma"));
            assert_eq!(options.band, Some(3));
        }
        Ok(())
    }

    #[test]
    fn test_load_raster_missing_path_error() {
        let result = MagicCommand::parse("%load_raster");
        assert!(result.is_err());
        let err = result.expect_err("should fail for missing path");
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn test_info_missing_dataset_error() {
        let result = MagicCommand::parse("%info");
        assert!(result.is_err());
    }

    #[test]
    fn test_crs_missing_dataset_error() {
        let result = MagicCommand::parse("%crs");
        assert!(result.is_err());
    }

    #[test]
    fn test_bounds_missing_dataset_error() {
        let result = MagicCommand::parse("%bounds");
        assert!(result.is_err());
    }

    #[test]
    fn test_stats_missing_dataset_error() {
        let result = MagicCommand::parse("%stats");
        assert!(result.is_err());
    }

    #[test]
    fn test_plot_missing_dataset_error() {
        let result = MagicCommand::parse("%plot");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_magic_prefix_error() {
        let result = MagicCommand::parse("%");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_info() -> Result<()> {
        use crate::kernel::Value;
        let mut ns = HashMap::new();
        ns.insert("layer".to_string(), Value::Path("/data.tif".into()));
        let cmd = MagicCommand::Info {
            dataset: "layer".to_string(),
        };
        let output = cmd.execute(&mut ns)?;
        let text = output.get("text/plain");
        assert!(text.is_some());
        assert!(text.unwrap_or(&String::new()).contains("layer"));
        Ok(())
    }

    #[test]
    fn test_execute_crs_reads_real_epsg_from_file() -> Result<()> {
        let values: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let (_guard, path) = write_test_raster_f32(1, &values)?;
        let mut ns = HashMap::new();
        load_raster_into(&mut ns, &path, "ds")?;

        let cmd = MagicCommand::Crs {
            dataset: "ds".to_string(),
        };
        let output = cmd.execute(&mut ns)?;
        let text = output.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        assert!(
            text.contains("4326"),
            "expected real EPSG:4326 WKT, got: {text}"
        );
        assert!(
            !text.contains("(example)"),
            "must not return the hardcoded placeholder CRS"
        );
        Ok(())
    }

    #[test]
    fn test_execute_crs_on_non_raster_value_is_an_error() {
        use crate::kernel::Value;
        let mut ns = HashMap::new();
        ns.insert("ds".to_string(), Value::Integer(1));
        let cmd = MagicCommand::Crs {
            dataset: "ds".to_string(),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_bounds_reads_real_geotransform_from_file() -> Result<()> {
        let values: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let (_guard, path) = write_test_raster_f32(1, &values)?;
        let mut ns = HashMap::new();
        load_raster_into(&mut ns, &path, "raster")?;

        let cmd = MagicCommand::Bounds {
            dataset: "raster".to_string(),
        };
        let output = cmd.execute(&mut ns)?;
        let text = output.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        // Geotransform: origin (100, 50), 1.0-unit north-up pixels, 4x4 image
        // => real bounds [100, 46, 104, 50], never the hardcoded [0,0,1,1].
        assert!(text.contains("100"), "expected real min_x=100, got: {text}");
        assert!(text.contains("104"), "expected real max_x=104, got: {text}");
        assert!(text.contains("46"), "expected real min_y=46, got: {text}");
        assert!(
            !text.contains("[0.0, 0.0, 1.0, 1.0]"),
            "must not return the hardcoded placeholder bounds"
        );
        Ok(())
    }

    #[test]
    fn test_execute_bounds_on_non_raster_value_is_an_error() {
        use crate::kernel::Value;
        let mut ns = HashMap::new();
        ns.insert("raster".to_string(), Value::Integer(1));
        let cmd = MagicCommand::Bounds {
            dataset: "raster".to_string(),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_stats_computes_real_statistics() -> Result<()> {
        // 4x4 single-band raster, values 0..=15 => mean 7.5, min 0, max 15.
        let values: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let (_guard, path) = write_test_raster_f32(1, &values)?;
        let mut ns = HashMap::new();
        load_raster_into(&mut ns, &path, "data")?;

        let cmd = MagicCommand::Stats {
            dataset: "data".to_string(),
            band: Some(1),
        };
        let output = cmd.execute(&mut ns)?;
        let text = output.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        assert!(text.contains("band 1"));
        assert!(text.contains("min=0"), "got: {text}");
        assert!(text.contains("max=15"), "got: {text}");
        assert!(text.contains("mean=7.5"), "got: {text}");
        assert!(
            !text.contains("(example)"),
            "must not return the hardcoded placeholder statistics"
        );
        Ok(())
    }

    #[test]
    fn test_execute_stats_selects_correct_band_from_interleaved_data() -> Result<()> {
        // 2-band 4x4 raster, pixel-interleaved: band0 = 0..15, band1 = (0..15)*10.
        let mut values = Vec::with_capacity(32);
        for i in 0..16 {
            values.push(i as f32);
            values.push(i as f32 * 10.0);
        }
        let (_guard, path) = write_test_raster_f32(2, &values)?;
        let mut ns = HashMap::new();
        load_raster_into(&mut ns, &path, "multi")?;

        let band1 = MagicCommand::Stats {
            dataset: "multi".to_string(),
            band: Some(1),
        }
        .execute(&mut ns)?;
        let text1 = band1.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        assert!(text1.contains("min=0"), "band1 got: {text1}");
        assert!(text1.contains("max=15"), "band1 got: {text1}");

        let band2 = MagicCommand::Stats {
            dataset: "multi".to_string(),
            band: Some(2),
        }
        .execute(&mut ns)?;
        let text2 = band2.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        assert!(text2.contains("min=0"), "band2 got: {text2}");
        assert!(text2.contains("max=150"), "band2 got: {text2}");
        assert!(text2.contains("mean=75"), "band2 got: {text2}");
        Ok(())
    }

    #[test]
    fn test_execute_stats_out_of_range_band_is_an_error() -> Result<()> {
        let values: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let (_guard, path) = write_test_raster_f32(1, &values)?;
        let mut ns = HashMap::new();
        load_raster_into(&mut ns, &path, "data")?;

        let cmd = MagicCommand::Stats {
            dataset: "data".to_string(),
            band: Some(5),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_stats_on_non_raster_value_is_an_error() {
        use crate::kernel::Value;
        let mut ns = HashMap::new();
        ns.insert("data".to_string(), Value::Float(1.0));
        let cmd = MagicCommand::Stats {
            dataset: "data".to_string(),
            band: Some(1),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_plot_missing_dataset() {
        use crate::kernel::Value;
        let mut ns: HashMap<String, Value> = HashMap::new();
        let cmd = MagicCommand::Plot {
            dataset: "nonexistent".to_string(),
            options: PlotOptions::default(),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_info_missing_dataset() {
        use crate::kernel::Value;
        let mut ns: HashMap<String, Value> = HashMap::new();
        let cmd = MagicCommand::Info {
            dataset: "missing".to_string(),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_load_raster_reads_the_real_file() -> Result<()> {
        use crate::kernel::Value;

        let values: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let (_guard, path) = write_test_raster_f32(1, &values)?;

        let mut ns: HashMap<String, Value> = HashMap::new();
        let cmd = MagicCommand::LoadRaster {
            path: path.clone(),
            name: Some("my_raster".to_string()),
        };
        let output = cmd.execute(&mut ns)?;

        let text = output.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        assert!(text.contains("my_raster"));
        assert!(
            text.contains("4x4"),
            "expected real dimensions, got: {text}"
        );

        match ns.get("my_raster") {
            Some(Value::Raster(handle)) => {
                assert_eq!(handle.metadata.width, 4);
                assert_eq!(handle.metadata.height, 4);
                assert_eq!(handle.metadata.band_count, 1);
                assert_eq!(handle.path, std::path::PathBuf::from(&path));
            }
            other => {
                return Err(JupyterError::Magic(format!(
                    "expected Value::Raster with real metadata, got {other:?}"
                )));
            }
        }
        Ok(())
    }

    #[test]
    fn test_execute_load_raster_nonexistent_file_is_an_honest_error() {
        use crate::kernel::Value;
        let mut ns: HashMap<String, Value> = HashMap::new();
        let cmd = MagicCommand::LoadRaster {
            path: "/nonexistent/path/does_not_exist.tif".to_string(),
            name: Some("my_raster".to_string()),
        };
        let result = cmd.execute(&mut ns);
        assert!(result.is_err(), "loading a missing file must fail loudly");
        assert!(
            !ns.contains_key("my_raster"),
            "the namespace must not gain a fake entry on failure"
        );
    }

    #[test]
    fn test_execute_load_raster_non_geotiff_file_is_an_honest_error() -> Result<()> {
        use crate::kernel::Value;
        let file = tempfile::Builder::new()
            .prefix("oxigeo_jupyter_not_a_tiff_")
            .suffix(".tif")
            .tempfile()
            .map_err(JupyterError::Io)?;
        std::fs::write(file.path(), b"this is not a valid TIFF file").map_err(JupyterError::Io)?;

        let mut ns: HashMap<String, Value> = HashMap::new();
        let cmd = MagicCommand::LoadRaster {
            path: file.path().to_string_lossy().to_string(),
            name: Some("my_raster".to_string()),
        };
        let result = cmd.execute(&mut ns);
        assert!(
            result.is_err(),
            "loading a non-GeoTIFF file must fail loudly"
        );
        assert!(!ns.contains_key("my_raster"));
        Ok(())
    }

    #[test]
    fn test_execute_list_empty() -> Result<()> {
        use crate::kernel::Value;
        let mut ns: HashMap<String, Value> = HashMap::new();
        let cmd = MagicCommand::List;
        let output = cmd.execute(&mut ns)?;
        let text = output.get("text/plain").map(|s| s.as_str()).unwrap_or("");
        assert!(text.contains("No datasets"));
        Ok(())
    }

    #[test]
    fn test_command_help_text() -> Result<()> {
        let cmd = MagicCommand::LoadRaster {
            path: "p".to_string(),
            name: None,
        };
        assert!(cmd.help().contains("load"));
        let cmd2 = MagicCommand::List;
        assert!(cmd2.help().contains("list") || cmd2.help().contains("List"));
        Ok(())
    }
}
