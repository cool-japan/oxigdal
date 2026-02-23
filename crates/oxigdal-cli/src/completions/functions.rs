//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use std::path::Path;

use super::types::{CompletionGenerator, DynamicCompleter, FilePathCompleter, GeoFormats, ShellType};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_shell_type_parsing() {
        assert!(matches!("bash".parse::< ShellType > (), Ok(ShellType::Bash)));
        assert!(matches!("zsh".parse::< ShellType > (), Ok(ShellType::Zsh)));
        assert!(matches!("fish".parse::< ShellType > (), Ok(ShellType::Fish)));
        assert!(
            matches!("powershell".parse::< ShellType > (), Ok(ShellType::PowerShell))
        );
        assert!("unknown".parse::< ShellType > ().is_err());
    }
    #[test]
    fn test_geo_formats() {
        let formats = GeoFormats::new();
        assert!(! formats.raster.is_empty());
        assert!(! formats.vector.is_empty());
        assert!(! formats.all.is_empty());
    }
    #[test]
    fn test_bash_generation() {
        let gen = CompletionGenerator::new();
        let mut output = Vec::new();
        gen.generate(ShellType::Bash, &mut output)
            .expect("Bash generation should succeed");
        let script = String::from_utf8(output).expect("Output should be valid UTF-8");
        assert!(script.contains("_oxigdal_completions"));
        assert!(script.contains("info"));
        assert!(script.contains("convert"));
    }
    #[test]
    fn test_zsh_generation() {
        let gen = CompletionGenerator::new();
        let mut output = Vec::new();
        gen.generate(ShellType::Zsh, &mut output)
            .expect("Zsh generation should succeed");
        let script = String::from_utf8(output).expect("Output should be valid UTF-8");
        assert!(script.contains("#compdef"));
        assert!(script.contains("_oxigdal"));
    }
    #[test]
    fn test_fish_generation() {
        let gen = CompletionGenerator::new();
        let mut output = Vec::new();
        gen.generate(ShellType::Fish, &mut output)
            .expect("Fish generation should succeed");
        let script = String::from_utf8(output).expect("Output should be valid UTF-8");
        assert!(script.contains("complete -c oxigdal"));
    }
    #[test]
    fn test_powershell_generation() {
        let gen = CompletionGenerator::new();
        let mut output = Vec::new();
        gen.generate(ShellType::PowerShell, &mut output)
            .expect("PowerShell generation should succeed");
        let script = String::from_utf8(output).expect("Output should be valid UTF-8");
        assert!(script.contains("Register-ArgumentCompleter"));
    }
    #[test]
    fn test_dynamic_completer_crs() {
        let completer = DynamicCompleter::new();
        let suggestions = completer.suggest_crs("EPSG:43");
        assert!(suggestions.iter().any(| s | s.contains("4326")));
    }
    #[test]
    fn test_dynamic_completer_resampling() {
        let completer = DynamicCompleter::new();
        let suggestions = completer.suggest_resampling("bi");
        assert!(suggestions.iter().any(| s | s.contains("bilinear")));
        assert!(suggestions.iter().any(| s | s.contains("bicubic")));
    }
    #[test]
    fn test_file_path_completer() {
        let completer = FilePathCompleter::new();
        assert!(completer.is_raster_file(Path::new("test.tif")));
        assert!(completer.is_vector_file(Path::new("test.geojson")));
        assert!(completer.is_geospatial_file(Path::new("test.shp")));
        assert!(! completer.is_geospatial_file(Path::new("test.txt")));
    }
    #[test]
    fn test_format_description() {
        let completer = FilePathCompleter::new();
        assert_eq!(
            completer.get_format_description(Path::new("test.tif")), Some("GeoTIFF")
        );
        assert_eq!(
            completer.get_format_description(Path::new("test.geojson")), Some("GeoJSON")
        );
        assert!(completer.get_format_description(Path::new("test.txt")).is_none());
    }
    #[test]
    fn test_command_definitions() {
        let commands = CompletionGenerator::build_command_defs();
        assert!(! commands.is_empty());
        for cmd in &commands {
            assert!(! cmd.name.is_empty());
            assert!(! cmd.description.is_empty());
        }
        let dem_cmd = commands.iter().find(|c| c.name == "dem");
        assert!(dem_cmd.is_some());
        assert!(! dem_cmd.map(| c | & c.subcommands).unwrap_or(& vec![]).is_empty());
    }
}
