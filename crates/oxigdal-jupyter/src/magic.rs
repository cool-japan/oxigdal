//! Magic commands for Jupyter
//!
//! This module provides magic commands for common OxiGDAL operations
//! that can be executed with the % prefix in Jupyter notebooks.

use crate::{JupyterError, Result};
use std::collections::HashMap;

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
                namespace.insert(var_name.to_string(), Value::Path(path.into()));
                output.insert(
                    "text/plain".to_string(),
                    format!("Loaded raster from '{}' into '{}'", path, var_name),
                );
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
                if !namespace.contains_key(dataset) {
                    return Err(JupyterError::Magic(format!(
                        "Dataset '{}' not found",
                        dataset
                    )));
                }
                output.insert(
                    "text/plain".to_string(),
                    format!("CRS for '{}': EPSG:4326 (example)", dataset),
                );
            }
            Self::Bounds { dataset } => {
                if !namespace.contains_key(dataset) {
                    return Err(JupyterError::Magic(format!(
                        "Dataset '{}' not found",
                        dataset
                    )));
                }
                output.insert(
                    "text/plain".to_string(),
                    format!("Bounds for '{}': [0.0, 0.0, 1.0, 1.0] (example)", dataset),
                );
            }
            Self::Stats { dataset, band } => {
                if !namespace.contains_key(dataset) {
                    return Err(JupyterError::Magic(format!(
                        "Dataset '{}' not found",
                        dataset
                    )));
                }
                let band_str = band.map(|b| format!(" band {}", b)).unwrap_or_default();
                output.insert(
                    "text/plain".to_string(),
                    format!(
                        "Statistics for '{}'{}: min=0.0, max=1.0, mean=0.5 (example)",
                        dataset, band_str
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
