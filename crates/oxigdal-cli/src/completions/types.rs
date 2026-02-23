//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use std::collections::{HashMap};

/// Geospatial file extensions supported by OxiGDAL
#[derive(Debug, Clone)]
pub struct GeoFormats {
    /// Raster formats with extensions
    pub raster: Vec<(&'static str, &'static str)>,
    /// Vector formats with extensions
    pub vector: Vec<(&'static str, &'static str)>,
    /// All supported formats combined
    pub all: Vec<&'static str>,
}
impl GeoFormats {
    /// Create a new GeoFormats instance with all supported formats
    #[must_use]
    pub fn new() -> Self {
        let raster = vec![
            ("tif", "GeoTIFF"), ("tiff", "GeoTIFF"), ("vrt", "Virtual Raster"), ("nc",
            "NetCDF"), ("hdf", "HDF5"), ("h5", "HDF5"), ("zarr", "Zarr"), ("jp2",
            "JPEG2000"), ("png", "PNG"), ("jpg", "JPEG"), ("jpeg", "JPEG"), ("img",
            "ERDAS Imagine"), ("asc", "ASCII Grid"), ("bil", "ESRI BIL"), ("bip",
            "ESRI BIP"), ("bsq", "ESRI BSQ"), ("dem", "Digital Elevation Model"), ("cog",
            "Cloud Optimized GeoTIFF"),
        ];
        let vector = vec![
            ("geojson", "GeoJSON"), ("json", "GeoJSON"), ("shp", "Shapefile"), ("fgb",
            "FlatGeobuf"), ("gpkg", "GeoPackage"), ("kml", "KML"), ("kmz", "KMZ"),
            ("gml", "GML"), ("parquet", "GeoParquet"), ("geoparquet", "GeoParquet"),
            ("mbtiles", "MBTiles"), ("pmtiles", "PMTiles"), ("pbf", "Protocol Buffers"),
            ("mvt", "Mapbox Vector Tiles"), ("csv", "CSV"), ("gpx", "GPX"),
        ];
        let mut all: Vec<&'static str> = raster.iter().map(|(ext, _)| *ext).collect();
        all.extend(vector.iter().map(|(ext, _)| *ext));
        Self { raster, vector, all }
    }
    /// Get raster extensions as a string pattern
    #[must_use]
    pub fn raster_pattern(&self) -> String {
        self.raster
            .iter()
            .map(|(ext, _)| format!("*.{}", ext))
            .collect::<Vec<_>>()
            .join(" ")
    }
    /// Get vector extensions as a string pattern
    #[must_use]
    pub fn vector_pattern(&self) -> String {
        self.vector
            .iter()
            .map(|(ext, _)| format!("*.{}", ext))
            .collect::<Vec<_>>()
            .join(" ")
    }
    /// Get all extensions as a string pattern
    #[must_use]
    pub fn all_pattern(&self) -> String {
        self.all.iter().map(|ext| format!("*.{}", ext)).collect::<Vec<_>>().join(" ")
    }
}
/// Command definition for completion generation
#[derive(Debug, Clone)]
pub struct CommandDef {
    /// Command name
    pub name: &'static str,
    /// Short description
    pub description: &'static str,
    /// Available options
    pub options: Vec<OptionDef>,
    /// Subcommands (if any)
    pub subcommands: Vec<CommandDef>,
    /// File argument type (raster, vector, or any)
    pub file_type: FileArgType,
}
/// Completion generator for OxiGDAL CLI
#[derive(Debug)]
pub struct CompletionGenerator {
    /// Program name
    program_name: String,
    /// Command definitions
    commands: Vec<CommandDef>,
    /// Supported geospatial formats
    formats: GeoFormats,
}
impl CompletionGenerator {
    /// Create a new completion generator with OxiGDAL commands
    #[must_use]
    pub fn new() -> Self {
        Self {
            program_name: "oxigdal".to_string(),
            commands: Self::build_command_defs(),
            formats: GeoFormats::new(),
        }
    }
    /// Build command definitions for all OxiGDAL commands
    fn build_command_defs() -> Vec<CommandDef> {
        vec![
            CommandDef { name : "info", description :
            "Display information about a raster or vector file", options : vec![OptionDef
            { short : Some("-s"), long : Some("--stats"), description :
            "Show detailed statistics", takes_value : false, possible_values : vec![], },
            OptionDef { short : None, long : Some("--compute-minmax"), description :
            "Compute min/max values", takes_value : false, possible_values : vec![], },
            OptionDef { short : Some("-m"), long : Some("--metadata"), description :
            "Show all metadata", takes_value : false, possible_values : vec![], },
            OptionDef { short : None, long : Some("--crs"), description :
            "Show coordinate reference system details", takes_value : false,
            possible_values : vec![], }, OptionDef { short : Some("-b"), long :
            Some("--bands"), description : "Show band/layer information", takes_value :
            false, possible_values : vec![], },], subcommands : vec![], file_type :
            FileArgType::Any, }, CommandDef { name : "convert", description :
            "Convert between geospatial formats", options : vec![OptionDef { short :
            Some("-f"), long : Some("--format"), description : "Output format",
            takes_value : true, possible_values : vec!["geotiff", "geojson", "shapefile",
            "flatgeobuf", "geoparquet",], }, OptionDef { short : Some("-t"), long :
            Some("--tile-size"), description : "Tile size for COG output", takes_value :
            true, possible_values : vec!["256", "512", "1024"], }, OptionDef { short :
            Some("-c"), long : Some("--compression"), description : "Compression method",
            takes_value : true, possible_values : vec!["none", "lzw", "deflate", "zstd",
            "jpeg"], }, OptionDef { short : None, long : Some("--compression-level"),
            description : "Compression level (1-9)", takes_value : true, possible_values
            : vec!["1", "2", "3", "4", "5", "6", "7", "8", "9"], }, OptionDef { short :
            None, long : Some("--cog"), description : "Create Cloud-Optimized GeoTIFF",
            takes_value : false, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--overviews"), description : "Number of overview levels",
            takes_value : true, possible_values : vec!["0", "2", "4", "8"], }, OptionDef
            { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], }, OptionDef { short : None, long : Some("--progress"), description :
            "Show progress bar", takes_value : true, possible_values : vec!["true",
            "false"], },], subcommands : vec![], file_type : FileArgType::Any, },
            CommandDef { name : "translate", description : "Subset and resample rasters",
            options : vec![OptionDef { short : Some("-f"), long : Some("--format"),
            description : "Output format", takes_value : true, possible_values :
            vec!["geotiff", "vrt", "cog"], }, OptionDef { short : None, long :
            Some("--src-win"), description : "Source window (xoff yoff xsize ysize)",
            takes_value : true, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--projwin"), description :
            "Projection window (ulx uly lrx lry)", takes_value : true, possible_values :
            vec![], }, OptionDef { short : Some("-r"), long : Some("--resampling"),
            description : "Resampling method", takes_value : true, possible_values :
            vec!["nearest", "bilinear", "bicubic", "lanczos"], }, OptionDef { short :
            None, long : Some("--outsize"), description :
            "Output size (width height or percentage)", takes_value : true,
            possible_values : vec![], }, OptionDef { short : Some("-b"), long :
            Some("--band"), description : "Select band(s)", takes_value : true,
            possible_values : vec![], }, OptionDef { short : None, long :
            Some("--scale"), description : "Scale values", takes_value : true,
            possible_values : vec![], }, OptionDef { short : None, long :
            Some("--unscale"), description : "Apply offset and scale", takes_value :
            false, possible_values : vec![], }, OptionDef { short : None, long :
            Some("--overwrite"), description : "Overwrite existing output file",
            takes_value : false, possible_values : vec![], },], subcommands : vec![],
            file_type : FileArgType::Raster, }, CommandDef { name : "warp", description :
            "Reproject and warp rasters", options : vec![OptionDef { short : Some("-s"),
            long : Some("-s_srs"), description : "Source coordinate reference system",
            takes_value : true, possible_values : vec![], }, OptionDef { short :
            Some("-t"), long : Some("-t_srs"), description :
            "Target coordinate reference system", takes_value : true, possible_values :
            vec![], }, OptionDef { short : None, long : Some("--ts-x"), description :
            "Output width in pixels", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--ts-y"), description :
            "Output height in pixels", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--tr"), description :
            "Output resolution in target units", takes_value : true, possible_values :
            vec![], }, OptionDef { short : Some("-r"), long : Some("--resampling"),
            description : "Resampling method", takes_value : true, possible_values :
            vec!["nearest", "bilinear", "bicubic", "lanczos"], }, OptionDef { short :
            None, long : Some("--te"), description :
            "Output extent (minx miny maxx maxy)", takes_value : true, possible_values :
            vec![], }, OptionDef { short : None, long : Some("--no-data"), description :
            "NoData value for output", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "calc", description : "Raster calculator operations",
            options : vec![OptionDef { short : None, long : Some("--calc"), description :
            "Calculation expression", takes_value : true, possible_values : vec![], },
            OptionDef { short : Some("-A"), long : None, description : "Input raster A",
            takes_value : true, possible_values : vec![], }, OptionDef { short :
            Some("-B"), long : None, description : "Input raster B", takes_value : true,
            possible_values : vec![], }, OptionDef { short : Some("-C"), long : None,
            description : "Input raster C", takes_value : true, possible_values : vec![],
            }, OptionDef { short : None, long : Some("--no-data"), description :
            "NoData value for output", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--type"), description :
            "Output data type", takes_value : true, possible_values : vec!["uint8",
            "int16", "uint16", "int32", "uint32", "float32", "float64",], }, OptionDef {
            short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "build-vrt", description :
            "Build virtual raster from multiple files", options : vec![OptionDef { short
            : None, long : Some("--resolution"), description : "Resolution mode",
            takes_value : true, possible_values : vec!["highest", "lowest", "average",
            "user"], }, OptionDef { short : Some("-r"), long : Some("--resampling"),
            description : "Resampling method", takes_value : true, possible_values :
            vec!["nearest", "bilinear", "bicubic", "lanczos"], }, OptionDef { short :
            None, long : Some("--separate"), description :
            "Create separate band for each input", takes_value : false, possible_values :
            vec![], }, OptionDef { short : None, long : Some("--input-file-list"),
            description : "File containing input file paths", takes_value : true,
            possible_values : vec![], }, OptionDef { short : None, long :
            Some("--overwrite"), description : "Overwrite existing output file",
            takes_value : false, possible_values : vec![], },], subcommands : vec![],
            file_type : FileArgType::Raster, }, CommandDef { name : "merge", description
            : "Merge multiple rasters into a single output", options : vec![OptionDef {
            short : None, long : Some("--no-data"), description :
            "NoData value for output", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--init"), description :
            "Initialize output with value", takes_value : true, possible_values : vec![],
            }, OptionDef { short : Some("-n"), long : Some("--n-input-no-data"),
            description : "Input NoData value", takes_value : true, possible_values :
            vec![], }, OptionDef { short : None, long : Some("--type"), description :
            "Output data type", takes_value : true, possible_values : vec!["uint8",
            "int16", "uint16", "int32", "uint32", "float32", "float64",], }, OptionDef {
            short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "validate", description :
            "Validate file format and compliance", options : vec![OptionDef { short :
            None, long : Some("--strict"), description : "Enable strict validation mode",
            takes_value : false, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--check-crs"), description :
            "Validate coordinate reference system", takes_value : false, possible_values
            : vec![], }, OptionDef { short : None, long : Some("--check-bounds"),
            description : "Validate geographic bounds", takes_value : false,
            possible_values : vec![], },], subcommands : vec![], file_type :
            FileArgType::Any, }, CommandDef { name : "inspect", description :
            "Inspect file format and metadata", options : vec![OptionDef { short : None,
            long : Some("--detailed"), description : "Show detailed inspection results",
            takes_value : false, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--raw"), description : "Show raw metadata", takes_value : false,
            possible_values : vec![], },], subcommands : vec![], file_type :
            FileArgType::Any, }, CommandDef { name : "profile", description :
            "Profile operation performance", options : vec![OptionDef { short : None,
            long : Some("--iterations"), description : "Number of iterations",
            takes_value : true, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--warmup"), description : "Warmup iterations", takes_value :
            true, possible_values : vec![], },], subcommands : vec![], file_type :
            FileArgType::Any, }, Self::build_dem_command(), CommandDef { name :
            "rasterize", description : "Convert vector geometries to raster", options :
            vec![OptionDef { short : Some("-a"), long : Some("--attribute"), description
            : "Attribute field for burn values", takes_value : true, possible_values :
            vec![], }, OptionDef { short : None, long : Some("--burn"), description :
            "Fixed burn value", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--ts"), description :
            "Output size (width height)", takes_value : true, possible_values : vec![],
            }, OptionDef { short : None, long : Some("--tr"), description :
            "Output resolution", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--te"), description : "Output extent",
            takes_value : true, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--init"), description : "Initialize raster with value",
            takes_value : true, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--type"), description : "Output data type", takes_value : true,
            possible_values : vec!["uint8", "int16", "uint16", "int32", "uint32",
            "float32", "float64",], }, OptionDef { short : None, long :
            Some("--overwrite"), description : "Overwrite existing output file",
            takes_value : false, possible_values : vec![], },], subcommands : vec![],
            file_type : FileArgType::Vector, }, CommandDef { name : "contour",
            description : "Generate contour lines from DEM", options : vec![OptionDef {
            short : Some("-i"), long : Some("--interval"), description :
            "Contour interval", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--off"), description :
            "Offset from zero", takes_value : true, possible_values : vec![], },
            OptionDef { short : Some("-a"), long : Some("--attribute"), description :
            "Elevation attribute name", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--fl"), description :
            "Fixed level(s)", takes_value : true, possible_values : vec![], }, OptionDef
            { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "proximity", description :
            "Compute proximity (distance) raster", options : vec![OptionDef { short :
            None, long : Some("--values"), description : "Target pixel values",
            takes_value : true, possible_values : vec![], }, OptionDef { short : None,
            long : Some("--distunits"), description : "Distance units", takes_value :
            true, possible_values : vec!["pixel", "geo"], }, OptionDef { short : None,
            long : Some("--maxdist"), description : "Maximum distance", takes_value :
            true, possible_values : vec![], }, OptionDef { short : None, long :
            Some("--no-data"), description : "NoData value for output", takes_value :
            true, possible_values : vec![], }, OptionDef { short : None, long :
            Some("--type"), description : "Output data type", takes_value : true,
            possible_values : vec!["uint8", "uint16", "int32", "float32", "float64"], },
            OptionDef { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "sieve", description :
            "Remove small raster polygons (sieve filter)", options : vec![OptionDef {
            short : Some("-s"), long : Some("--threshold"), description :
            "Size threshold", takes_value : true, possible_values : vec![], }, OptionDef
            { short : Some("-c"), long : Some("--connectedness"), description :
            "Connectivity (4 or 8)", takes_value : true, possible_values : vec!["4",
            "8"], }, OptionDef { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "fillnodata", description :
            "Fill NoData values using interpolation", options : vec![OptionDef { short :
            None, long : Some("--mask"), description : "Mask band/file", takes_value :
            true, possible_values : vec![], }, OptionDef { short : None, long :
            Some("--md"), description : "Maximum distance to search", takes_value : true,
            possible_values : vec![], }, OptionDef { short : None, long : Some("--si"),
            description : "Smoothing iterations", takes_value : true, possible_values :
            vec![], }, OptionDef { short : Some("-b"), long : Some("--band"), description
            : "Band to process", takes_value : true, possible_values : vec![], },
            OptionDef { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },], subcommands : vec![], file_type : FileArgType::Raster, },
            CommandDef { name : "completions", description :
            "Generate shell completions", options : vec![], subcommands : vec![],
            file_type : FileArgType::None, },
        ]
    }
    /// Build DEM command with subcommands
    fn build_dem_command() -> CommandDef {
        let common_dem_options = vec![
            OptionDef { short : None, long : Some("--overwrite"), description :
            "Overwrite existing output file", takes_value : false, possible_values :
            vec![], },
        ];
        CommandDef {
            name: "dem",
            description: "DEM analysis operations (hillshade, slope, aspect, TRI, TPI, roughness)",
            options: vec![],
            subcommands: vec![
                CommandDef { name : "hillshade", description :
                "Generate hillshade from DEM", options : { let mut opts = vec![OptionDef
                { short : Some("-z"), long : Some("--azimuth"), description :
                "Azimuth of light source (0-360 degrees)", takes_value : true,
                possible_values : vec![], }, OptionDef { short : Some("-a"), long :
                Some("--altitude"), description :
                "Altitude of light source (0-90 degrees)", takes_value : true,
                possible_values : vec![], }, OptionDef { short : Some("-z"), long :
                Some("--z-factor"), description : "Z factor (vertical exaggeration)",
                takes_value : true, possible_values : vec![], }, OptionDef { short :
                Some("-s"), long : Some("--scale"), description : "Scale factor",
                takes_value : true, possible_values : vec![], }, OptionDef { short :
                None, long : Some("--combined"), description :
                "Combined shading (multidirectional)", takes_value : false,
                possible_values : vec![], },]; opts.extend(common_dem_options.clone());
                opts }, subcommands : vec![], file_type : FileArgType::Raster, },
                CommandDef { name : "slope", description : "Calculate slope", options : {
                let mut opts = vec![OptionDef { short : None, long :
                Some("--slope-format"), description : "Slope format", takes_value : true,
                possible_values : vec!["degree", "percent"], }, OptionDef { short :
                Some("-s"), long : Some("--scale"), description : "Scale factor",
                takes_value : true, possible_values : vec![], },]; opts
                .extend(common_dem_options.clone()); opts }, subcommands : vec![],
                file_type : FileArgType::Raster, }, CommandDef { name : "aspect",
                description : "Calculate aspect", options : { let mut opts =
                vec![OptionDef { short : None, long : Some("--zero-for-flat"),
                description : "Return zero for flat areas", takes_value : false,
                possible_values : vec![], }]; opts.extend(common_dem_options.clone());
                opts }, subcommands : vec![], file_type : FileArgType::Raster, },
                CommandDef { name : "tri", description :
                "Calculate Terrain Ruggedness Index", options : common_dem_options
                .clone(), subcommands : vec![], file_type : FileArgType::Raster, },
                CommandDef { name : "tpi", description :
                "Calculate Topographic Position Index", options : common_dem_options
                .clone(), subcommands : vec![], file_type : FileArgType::Raster, },
                CommandDef { name : "roughness", description : "Calculate roughness",
                options : common_dem_options, subcommands : vec![], file_type :
                FileArgType::Raster, },
            ],
            file_type: FileArgType::Raster,
        }
    }
    /// Generate completion script for the specified shell
    ///
    /// # Errors
    ///
    /// Returns an error if writing to the output fails.
    pub fn generate<W: Write>(&self, shell: ShellType, out: &mut W) -> io::Result<()> {
        match shell {
            ShellType::Bash => self.generate_bash(out),
            ShellType::Zsh => self.generate_zsh(out),
            ShellType::Fish => self.generate_fish(out),
            ShellType::PowerShell => self.generate_powershell(out),
        }
    }
    /// Generate Bash completion script
    fn generate_bash<W: Write>(&self, out: &mut W) -> io::Result<()> {
        let mut script = String::with_capacity(8192);
        writeln!(script, "# Bash completion script for {0}", self.program_name).ok();
        writeln!(script, "# Generated by OxiGDAL CLI").ok();
        writeln!(script).ok();
        writeln!(script, "_{0}_completions() {{", self.program_name.replace('-', "_"))
            .ok();
        writeln!(script, "    local cur prev words cword").ok();
        writeln!(script, "    _init_completion || return").ok();
        writeln!(script).ok();
        writeln!(script, "    local cmd=\"\"").ok();
        writeln!(script, "    local subcmd=\"\"").ok();
        writeln!(script, "    for ((i=1; i < cword; i++)); do").ok();
        writeln!(script, "        case \"${{words[i]}}\" in").ok();
        for cmd in &self.commands {
            writeln!(script, "            {}) cmd=\"{}\"; break;;", cmd.name, cmd.name)
                .ok();
        }
        writeln!(script, "        esac").ok();
        writeln!(script, "    done").ok();
        writeln!(script).ok();
        writeln!(script, "    if [[ \"$cmd\" == \"dem\" ]]; then").ok();
        writeln!(script, "        for ((i=1; i < cword; i++)); do").ok();
        writeln!(script, "            case \"${{words[i]}}\" in").ok();
        for cmd in &self.commands {
            if cmd.name == "dem" {
                for sub in &cmd.subcommands {
                    writeln!(
                        script, "                {}) subcmd=\"{}\"; break;;", sub.name,
                        sub.name
                    )
                        .ok();
                }
            }
        }
        writeln!(script, "            esac").ok();
        writeln!(script, "        done").ok();
        writeln!(script, "    fi").ok();
        writeln!(script).ok();
        writeln!(
            script,
            "    local global_opts=\"-v --verbose -q --quiet --format --help --version\""
        )
            .ok();
        writeln!(script).ok();
        writeln!(script, "    if [[ -z \"$cmd\" ]]; then").ok();
        writeln!(script, "        # Complete commands").ok();
        let cmds: Vec<&str> = self.commands.iter().map(|c| c.name).collect();
        writeln!(
            script, "        COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))", cmds
            .join(" ")
        )
            .ok();
        writeln!(script, "        return").ok();
        writeln!(script, "    fi").ok();
        writeln!(script).ok();
        writeln!(script, "    case \"$cmd\" in").ok();
        for cmd in &self.commands {
            self.generate_bash_command_case(&mut script, cmd);
        }
        writeln!(script, "    esac").ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
        self.generate_bash_file_completion(&mut script);
        writeln!(
            script, "complete -F _{0}_completions {0}", self.program_name.replace('-',
            "_")
        )
            .ok();
        out.write_all(script.as_bytes())
    }
    /// Generate Bash case statement for a command
    fn generate_bash_command_case(&self, script: &mut String, cmd: &CommandDef) {
        writeln!(script, "        {})".replace("{}", cmd.name)).ok();
        if !cmd.subcommands.is_empty() {
            writeln!(script, "            if [[ -z \"$subcmd\" ]]; then").ok();
            let subcmds: Vec<&str> = cmd.subcommands.iter().map(|s| s.name).collect();
            writeln!(
                script, "                COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))",
                subcmds.join(" ")
            )
                .ok();
            writeln!(script, "                return").ok();
            writeln!(script, "            fi").ok();
            writeln!(script, "            case \"$subcmd\" in").ok();
            for sub in &cmd.subcommands {
                self.generate_bash_subcommand_case(script, sub);
            }
            writeln!(script, "            esac").ok();
        } else {
            let mut opts = Vec::new();
            for opt in &cmd.options {
                if let Some(short) = opt.short {
                    opts.push(short.to_string());
                }
                if let Some(long) = opt.long {
                    opts.push(long.to_string());
                }
            }
            writeln!(script, "            case \"$prev\" in").ok();
            for opt in &cmd.options {
                if opt.takes_value && !opt.possible_values.is_empty() {
                    let flag = opt.long.or(opt.short).unwrap_or("");
                    writeln!(script, "                {})", flag).ok();
                    writeln!(
                        script,
                        "                    COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))",
                        opt.possible_values.join(" ")
                    )
                        .ok();
                    writeln!(script, "                    return;;").ok();
                }
            }
            writeln!(script, "            esac").ok();
            match cmd.file_type {
                FileArgType::Raster => {
                    writeln!(
                        script,
                        "            _oxigdal_file_completion \"raster\" \"$cur\""
                    )
                        .ok();
                }
                FileArgType::Vector => {
                    writeln!(
                        script,
                        "            _oxigdal_file_completion \"vector\" \"$cur\""
                    )
                        .ok();
                }
                FileArgType::Any => {
                    writeln!(
                        script, "            _oxigdal_file_completion \"any\" \"$cur\""
                    )
                        .ok();
                }
                FileArgType::Generic | FileArgType::None => {
                    writeln!(
                        script,
                        "            COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))", opts
                        .join(" ")
                    )
                        .ok();
                }
            }
        }
        writeln!(script, "            ;;").ok();
    }
    /// Generate Bash case for subcommand
    fn generate_bash_subcommand_case(&self, script: &mut String, sub: &CommandDef) {
        writeln!(script, "                {})".replace("{}", sub.name)).ok();
        let mut opts = Vec::new();
        for opt in &sub.options {
            if let Some(short) = opt.short {
                opts.push(short.to_string());
            }
            if let Some(long) = opt.long {
                opts.push(long.to_string());
            }
        }
        match sub.file_type {
            FileArgType::Raster => {
                writeln!(
                    script,
                    "                    _oxigdal_file_completion \"raster\" \"$cur\""
                )
                    .ok();
            }
            FileArgType::Vector => {
                writeln!(
                    script,
                    "                    _oxigdal_file_completion \"vector\" \"$cur\""
                )
                    .ok();
            }
            FileArgType::Any => {
                writeln!(
                    script,
                    "                    _oxigdal_file_completion \"any\" \"$cur\""
                )
                    .ok();
            }
            FileArgType::Generic | FileArgType::None => {
                writeln!(
                    script,
                    "                    COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))",
                    opts.join(" ")
                )
                    .ok();
            }
        }
        writeln!(script, "                    ;;").ok();
    }
    /// Generate Bash file completion function
    fn generate_bash_file_completion(&self, script: &mut String) {
        writeln!(script, "_oxigdal_file_completion() {{").ok();
        writeln!(script, "    local type=\"$1\"").ok();
        writeln!(script, "    local cur=\"$2\"").ok();
        writeln!(script).ok();
        writeln!(script, "    local raster_exts=\"{}\"", self.formats.raster_pattern())
            .ok();
        writeln!(script, "    local vector_exts=\"{}\"", self.formats.vector_pattern())
            .ok();
        writeln!(script).ok();
        writeln!(script, "    case \"$type\" in").ok();
        writeln!(script, "        raster)").ok();
        writeln!(
            script,
            "            COMPREPLY=($(compgen -f -X '!@(${{raster_exts// /|}})' -- \"$cur\"))"
        )
            .ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "        vector)").ok();
        writeln!(
            script,
            "            COMPREPLY=($(compgen -f -X '!@(${{vector_exts// /|}})' -- \"$cur\"))"
        )
            .ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "        any|*)").ok();
        writeln!(script, "            local all_exts=\"$raster_exts $vector_exts\"")
            .ok();
        writeln!(
            script,
            "            COMPREPLY=($(compgen -f -X '!@(${{all_exts// /|}})' -- \"$cur\"))"
        )
            .ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "    esac").ok();
        writeln!(script).ok();
        writeln!(script, "    # Also complete directories").ok();
        writeln!(script, "    COMPREPLY+=($(compgen -d -- \"$cur\" | sed 's/$/\\//'))")
            .ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
    }
    /// Generate Zsh completion script
    fn generate_zsh<W: Write>(&self, out: &mut W) -> io::Result<()> {
        let mut script = String::with_capacity(12288);
        writeln!(script, "#compdef {0}", self.program_name).ok();
        writeln!(script, "# Zsh completion script for {}", self.program_name).ok();
        writeln!(script, "# Generated by OxiGDAL CLI").ok();
        writeln!(script).ok();
        writeln!(script, "# Geospatial file extensions").ok();
        writeln!(script, "local -a raster_exts vector_exts").ok();
        writeln!(
            script, "raster_exts=({})", self.formats.raster.iter().map(| (ext, _) |
            format!("'*.{}'", ext)).collect::< Vec < _ >> ().join(" ")
        )
            .ok();
        writeln!(
            script, "vector_exts=({})", self.formats.vector.iter().map(| (ext, _) |
            format!("'*.{}'", ext)).collect::< Vec < _ >> ().join(" ")
        )
            .ok();
        writeln!(script).ok();
        writeln!(script, "_oxigdal_raster_files() {{").ok();
        writeln!(script, "    _files -g \"${{(j:|:)raster_exts}}\"").ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
        writeln!(script, "_oxigdal_vector_files() {{").ok();
        writeln!(script, "    _files -g \"${{(j:|:)vector_exts}}\"").ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
        writeln!(script, "_oxigdal_any_files() {{").ok();
        writeln!(
            script, "    _files -g \"${{(j:|:)raster_exts}}|${{(j:|:)vector_exts}}\""
        )
            .ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
        writeln!(script, "_{0}() {{", self.program_name.replace('-', "_")).ok();
        writeln!(script, "    local -a commands").ok();
        writeln!(script, "    local context state state_descr line").ok();
        writeln!(script, "    typeset -A opt_args").ok();
        writeln!(script).ok();
        writeln!(script, "    _arguments -C \\").ok();
        writeln!(script, "        '-v[Enable verbose output]' \\").ok();
        writeln!(script, "        '--verbose[Enable verbose output]' \\").ok();
        writeln!(script, "        '-q[Suppress all output except errors]' \\").ok();
        writeln!(script, "        '--quiet[Suppress all output except errors]' \\").ok();
        writeln!(script, "        '--format[Output format]:format:(text json)' \\").ok();
        writeln!(script, "        '1:command:->commands' \\").ok();
        writeln!(script, "        '*::arg:->args'").ok();
        writeln!(script).ok();
        writeln!(script, "    case $state in").ok();
        writeln!(script, "        commands)").ok();
        writeln!(script, "            commands=(").ok();
        for cmd in &self.commands {
            writeln!(script, "                '{}:{}'", cmd.name, cmd.description).ok();
        }
        writeln!(script, "            )").ok();
        writeln!(script, "            _describe 'command' commands").ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "        args)").ok();
        writeln!(script, "            case $line[1] in").ok();
        for cmd in &self.commands {
            self.generate_zsh_command_case(&mut script, cmd);
        }
        writeln!(script, "            esac").ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "    esac").ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
        for cmd in &self.commands {
            if !cmd.subcommands.is_empty() {
                self.generate_zsh_subcommand_function(&mut script, cmd);
            }
        }
        writeln!(script, "compdef _{0} {0}", self.program_name.replace('-', "_")).ok();
        out.write_all(script.as_bytes())
    }
    /// Generate Zsh case for a command
    fn generate_zsh_command_case(&self, script: &mut String, cmd: &CommandDef) {
        writeln!(script, "                {})".replace("{}", cmd.name)).ok();
        if !cmd.subcommands.is_empty() {
            writeln!(
                script, "                    _{}_{}", self.program_name.replace('-',
                "_"), cmd.name.replace('-', "_")
            )
                .ok();
        } else {
            writeln!(script, "                    _arguments \\").ok();
            for opt in &cmd.options {
                if let Some(short) = opt.short {
                    if opt.takes_value {
                        if opt.possible_values.is_empty() {
                            writeln!(
                                script, "                        '{}[{}]:value:' \\", short,
                                opt.description.replace('[', "\\[").replace(']', "\\]")
                            )
                                .ok();
                        } else {
                            writeln!(
                                script, "                        '{}[{}]:value:({})' \\",
                                short, opt.description.replace('[', "\\[").replace(']',
                                "\\]"), opt.possible_values.join(" ")
                            )
                                .ok();
                        }
                    } else {
                        writeln!(
                            script, "                        '{}[{}]' \\", short, opt
                            .description.replace('[', "\\[").replace(']', "\\]")
                        )
                            .ok();
                    }
                }
                if let Some(long) = opt.long {
                    if opt.takes_value {
                        if opt.possible_values.is_empty() {
                            writeln!(
                                script, "                        '{}[{}]:value:' \\", long,
                                opt.description.replace('[', "\\[").replace(']', "\\]")
                            )
                                .ok();
                        } else {
                            writeln!(
                                script, "                        '{}[{}]:value:({})' \\",
                                long, opt.description.replace('[', "\\[").replace(']',
                                "\\]"), opt.possible_values.join(" ")
                            )
                                .ok();
                        }
                    } else {
                        writeln!(
                            script, "                        '{}[{}]' \\", long, opt
                            .description.replace('[', "\\[").replace(']', "\\]")
                        )
                            .ok();
                    }
                }
            }
            let file_comp = match cmd.file_type {
                FileArgType::Raster => "_oxigdal_raster_files",
                FileArgType::Vector => "_oxigdal_vector_files",
                FileArgType::Any => "_oxigdal_any_files",
                FileArgType::Generic => "_files",
                FileArgType::None => "",
            };
            if !file_comp.is_empty() {
                writeln!(
                    script, "                        '1:input file:{}' \\", file_comp
                )
                    .ok();
                writeln!(script, "                        '2:output file:_files'").ok();
            }
        }
        writeln!(script, "                    ;;").ok();
    }
    /// Generate Zsh subcommand function
    fn generate_zsh_subcommand_function(&self, script: &mut String, cmd: &CommandDef) {
        writeln!(
            script, "_{}_{}() {{", self.program_name.replace('-', "_"), cmd.name
            .replace('-', "_")
        )
            .ok();
        writeln!(script, "    local -a subcommands").ok();
        writeln!(script, "    local context state state_descr line").ok();
        writeln!(script, "    typeset -A opt_args").ok();
        writeln!(script).ok();
        writeln!(script, "    _arguments -C \\").ok();
        writeln!(script, "        '1:subcommand:->subcommands' \\").ok();
        writeln!(script, "        '*::arg:->args'").ok();
        writeln!(script).ok();
        writeln!(script, "    case $state in").ok();
        writeln!(script, "        subcommands)").ok();
        writeln!(script, "            subcommands=(").ok();
        for sub in &cmd.subcommands {
            writeln!(script, "                '{}:{}'", sub.name, sub.description).ok();
        }
        writeln!(script, "            )").ok();
        writeln!(script, "            _describe 'subcommand' subcommands").ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "        args)").ok();
        writeln!(script, "            case $line[1] in").ok();
        for sub in &cmd.subcommands {
            writeln!(script, "                {})".replace("{}", sub.name)).ok();
            writeln!(script, "                    _arguments \\").ok();
            for opt in &sub.options {
                if let Some(long) = opt.long {
                    if opt.takes_value {
                        if opt.possible_values.is_empty() {
                            writeln!(
                                script, "                        '{}[{}]:value:' \\", long,
                                opt.description.replace('[', "\\[").replace(']', "\\]")
                            )
                                .ok();
                        } else {
                            writeln!(
                                script, "                        '{}[{}]:value:({})' \\",
                                long, opt.description.replace('[', "\\[").replace(']',
                                "\\]"), opt.possible_values.join(" ")
                            )
                                .ok();
                        }
                    } else {
                        writeln!(
                            script, "                        '{}[{}]' \\", long, opt
                            .description.replace('[', "\\[").replace(']', "\\]")
                        )
                            .ok();
                    }
                }
            }
            let file_comp = match sub.file_type {
                FileArgType::Raster => "_oxigdal_raster_files",
                FileArgType::Vector => "_oxigdal_vector_files",
                FileArgType::Any => "_oxigdal_any_files",
                FileArgType::Generic => "_files",
                FileArgType::None => "",
            };
            if !file_comp.is_empty() {
                writeln!(
                    script, "                        '1:input file:{}' \\", file_comp
                )
                    .ok();
                writeln!(script, "                        '2:output file:_files'").ok();
            }
            writeln!(script, "                    ;;").ok();
        }
        writeln!(script, "            esac").ok();
        writeln!(script, "            ;;").ok();
        writeln!(script, "    esac").ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
    }
    /// Generate Fish completion script
    fn generate_fish<W: Write>(&self, out: &mut W) -> io::Result<()> {
        let mut script = String::with_capacity(8192);
        writeln!(script, "# Fish completion script for {}", self.program_name).ok();
        writeln!(script, "# Generated by OxiGDAL CLI").ok();
        writeln!(script).ok();
        writeln!(script, "complete -c {} -f", self.program_name).ok();
        writeln!(script).ok();
        writeln!(
            script, "complete -c {0} -s v -l verbose -d 'Enable verbose output'", self
            .program_name
        )
            .ok();
        writeln!(
            script,
            "complete -c {0} -s q -l quiet -d 'Suppress all output except errors'", self
            .program_name
        )
            .ok();
        writeln!(
            script, "complete -c {0} -l format -d 'Output format' -xa 'text json'", self
            .program_name
        )
            .ok();
        writeln!(script).ok();
        writeln!(script, "# Commands").ok();
        for cmd in &self.commands {
            writeln!(
                script, "complete -c {0} -n '__fish_use_subcommand' -a {1} -d '{2}'",
                self.program_name, cmd.name, cmd.description
            )
                .ok();
        }
        writeln!(script).ok();
        for cmd in &self.commands {
            self.generate_fish_command_completions(&mut script, cmd);
        }
        self.generate_fish_file_functions(&mut script);
        out.write_all(script.as_bytes())
    }
    /// Generate Fish completions for a command
    fn generate_fish_command_completions(&self, script: &mut String, cmd: &CommandDef) {
        writeln!(script, "# {} command", cmd.name).ok();
        if !cmd.subcommands.is_empty() {
            for sub in &cmd.subcommands {
                writeln!(
                    script,
                    "complete -c {0} -n '__fish_seen_subcommand_from {1}; and not __fish_seen_subcommand_from {2}' -a {2} -d '{3}'",
                    self.program_name, cmd.name, sub.name, sub.description
                )
                    .ok();
                for opt in &sub.options {
                    self.generate_fish_option(
                        script,
                        &format!("{} {}", cmd.name, sub.name),
                        opt,
                    );
                }
                self.generate_fish_file_completion(
                    script,
                    &format!("{} {}", cmd.name, sub.name),
                    sub.file_type,
                );
            }
        } else {
            for opt in &cmd.options {
                self.generate_fish_option(script, cmd.name, opt);
            }
            self.generate_fish_file_completion(script, cmd.name, cmd.file_type);
        }
        writeln!(script).ok();
    }
    /// Generate Fish option completion
    fn generate_fish_option(
        &self,
        script: &mut String,
        cmd_context: &str,
        opt: &OptionDef,
    ) {
        let condition = format!(
            "'__fish_seen_subcommand_from {}'", cmd_context.split_whitespace().next()
            .unwrap_or("")
        );
        let short_flag = opt
            .short
            .map(|s| format!("-s {}", s.trim_start_matches('-')))
            .unwrap_or_default();
        let long_flag = opt
            .long
            .map(|l| format!("-l {}", l.trim_start_matches('-')))
            .unwrap_or_default();
        let requires_arg = if opt.takes_value { "-r" } else { "" };
        let values = if !opt.possible_values.is_empty() {
            format!("-xa '{}'", opt.possible_values.join(" "))
        } else {
            String::new()
        };
        writeln!(
            script, "complete -c {0} -n {1} {2} {3} {4} {5} -d '{6}'", self.program_name,
            condition, short_flag, long_flag, requires_arg, values, opt.description
        )
            .ok();
    }
    /// Generate Fish file completion for a command
    fn generate_fish_file_completion(
        &self,
        script: &mut String,
        cmd_context: &str,
        file_type: FileArgType,
    ) {
        let condition = format!(
            "'__fish_seen_subcommand_from {}'", cmd_context.split_whitespace().next()
            .unwrap_or("")
        );
        match file_type {
            FileArgType::Raster => {
                writeln!(
                    script, "complete -c {0} -n {1} -a '(__oxigdal_raster_files)'", self
                    .program_name, condition
                )
                    .ok();
            }
            FileArgType::Vector => {
                writeln!(
                    script, "complete -c {0} -n {1} -a '(__oxigdal_vector_files)'", self
                    .program_name, condition
                )
                    .ok();
            }
            FileArgType::Any => {
                writeln!(
                    script, "complete -c {0} -n {1} -a '(__oxigdal_any_files)'", self
                    .program_name, condition
                )
                    .ok();
            }
            FileArgType::Generic => {
                writeln!(
                    script, "complete -c {0} -n {1} -F", self.program_name, condition
                )
                    .ok();
            }
            FileArgType::None => {}
        }
    }
    /// Generate Fish file completion functions
    fn generate_fish_file_functions(&self, script: &mut String) {
        writeln!(script, "# File completion functions").ok();
        let raster_exts: Vec<String> = self
            .formats
            .raster
            .iter()
            .map(|(ext, _)| format!("-e {}", ext))
            .collect();
        writeln!(script, "function __oxigdal_raster_files").ok();
        writeln!(script, "    __fish_complete_suffix {}", raster_exts.join(" ")).ok();
        writeln!(script, "end").ok();
        writeln!(script).ok();
        let vector_exts: Vec<String> = self
            .formats
            .vector
            .iter()
            .map(|(ext, _)| format!("-e {}", ext))
            .collect();
        writeln!(script, "function __oxigdal_vector_files").ok();
        writeln!(script, "    __fish_complete_suffix {}", vector_exts.join(" ")).ok();
        writeln!(script, "end").ok();
        writeln!(script).ok();
        writeln!(script, "function __oxigdal_any_files").ok();
        writeln!(
            script, "    __fish_complete_suffix {} {}", raster_exts.join(" "),
            vector_exts.join(" ")
        )
            .ok();
        writeln!(script, "end").ok();
        writeln!(script).ok();
    }
    /// Generate PowerShell completion script
    fn generate_powershell<W: Write>(&self, out: &mut W) -> io::Result<()> {
        let mut script = String::with_capacity(10240);
        writeln!(script, "# PowerShell completion script for {}", self.program_name)
            .ok();
        writeln!(script, "# Generated by OxiGDAL CLI").ok();
        writeln!(script).ok();
        writeln!(script, "# Geospatial file extensions").ok();
        writeln!(
            script, "$script:RasterExtensions = @({})", self.formats.raster.iter().map(|
            (ext, _) | format!("'{}'", ext)).collect::< Vec < _ >> ().join(", ")
        )
            .ok();
        writeln!(
            script, "$script:VectorExtensions = @({})", self.formats.vector.iter().map(|
            (ext, _) | format!("'{}'", ext)).collect::< Vec < _ >> ().join(", ")
        )
            .ok();
        writeln!(script).ok();
        writeln!(script, "function Get-OxigdalFileCompletion {{").ok();
        writeln!(script, "    param(").ok();
        writeln!(script, "        [string]$WordToComplete,").ok();
        writeln!(script, "        [string]$FileType").ok();
        writeln!(script, "    )").ok();
        writeln!(script).ok();
        writeln!(script, "    $extensions = switch ($FileType) {{").ok();
        writeln!(script, "        'raster' {{ $script:RasterExtensions }}").ok();
        writeln!(script, "        'vector' {{ $script:VectorExtensions }}").ok();
        writeln!(
            script,
            "        default {{ $script:RasterExtensions + $script:VectorExtensions }}"
        )
            .ok();
        writeln!(script, "    }}").ok();
        writeln!(script).ok();
        writeln!(script, "    $pattern = \"*$WordToComplete*\"").ok();
        writeln!(script, "    Get-ChildItem -Path . -File | Where-Object {{").ok();
        writeln!(script, "        $_.Name -like $pattern -and").ok();
        writeln!(script, "        $extensions -contains $_.Extension.TrimStart('.')")
            .ok();
        writeln!(script, "    }} | ForEach-Object {{").ok();
        writeln!(script, "        [System.Management.Automation.CompletionResult]::new(")
            .ok();
        writeln!(script, "            $_.Name,").ok();
        writeln!(script, "            $_.Name,").ok();
        writeln!(
            script,
            "            [System.Management.Automation.CompletionResultType]::ParameterValue,"
        )
            .ok();
        writeln!(script, "            $_.FullName").ok();
        writeln!(script, "        )").ok();
        writeln!(script, "    }}").ok();
        writeln!(script, "}}").ok();
        writeln!(script).ok();
        writeln!(
            script, "Register-ArgumentCompleter -Native -CommandName {} -ScriptBlock {{",
            self.program_name
        )
            .ok();
        writeln!(script, "    param($wordToComplete, $commandAst, $cursorPosition)")
            .ok();
        writeln!(script).ok();
        writeln!(script, "    $commandElements = $commandAst.CommandElements").ok();
        writeln!(script, "    $command = @(").ok();
        writeln!(script, "        for ($i = 1; $i -lt $commandElements.Count; $i++) {{")
            .ok();
        writeln!(script, "            $element = $commandElements[$i]").ok();
        writeln!(script, "            if ($element -notlike '-*') {{").ok();
        writeln!(script, "                $element.Extent.Text").ok();
        writeln!(script, "                break").ok();
        writeln!(script, "            }}").ok();
        writeln!(script, "        }}").ok();
        writeln!(script, "    )[0]").ok();
        writeln!(script).ok();
        writeln!(script, "    $commands = @{{").ok();
        for cmd in &self.commands {
            writeln!(script, "        '{}' = @{{", cmd.name).ok();
            writeln!(script, "            Description = '{}'", cmd.description).ok();
            let opts: Vec<String> = cmd
                .options
                .iter()
                .filter_map(|o| o.long.map(|l| l.to_string()))
                .collect();
            writeln!(
                script, "            Options = @({})", opts.iter().map(| o |
                format!("'{}'", o)).collect::< Vec < _ >> ().join(", ")
            )
                .ok();
            let file_type = match cmd.file_type {
                FileArgType::Raster => "raster",
                FileArgType::Vector => "vector",
                FileArgType::Any => "any",
                _ => "none",
            };
            writeln!(script, "            FileType = '{}'", file_type).ok();
            if !cmd.subcommands.is_empty() {
                writeln!(script, "            Subcommands = @{{").ok();
                for sub in &cmd.subcommands {
                    writeln!(script, "                '{}' = @{{", sub.name).ok();
                    writeln!(
                        script, "                    Description = '{}'", sub.description
                    )
                        .ok();
                    let sub_file_type = match sub.file_type {
                        FileArgType::Raster => "raster",
                        FileArgType::Vector => "vector",
                        FileArgType::Any => "any",
                        _ => "none",
                    };
                    writeln!(
                        script, "                    FileType = '{}'", sub_file_type
                    )
                        .ok();
                    writeln!(script, "                }}").ok();
                }
                writeln!(script, "            }}").ok();
            }
            writeln!(script, "        }}").ok();
        }
        writeln!(script, "    }}").ok();
        writeln!(script).ok();
        writeln!(script, "    if (-not $command) {{").ok();
        writeln!(script, "        # Complete commands").ok();
        writeln!(
            script,
            "        $commands.Keys | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{"
        )
            .ok();
        writeln!(script, "            $desc = $commands[$_].Description").ok();
        writeln!(
            script, "            [System.Management.Automation.CompletionResult]::new("
        )
            .ok();
        writeln!(script, "                $_,").ok();
        writeln!(script, "                $_,").ok();
        writeln!(
            script,
            "                [System.Management.Automation.CompletionResultType]::ParameterValue,"
        )
            .ok();
        writeln!(script, "                $desc").ok();
        writeln!(script, "            )").ok();
        writeln!(script, "        }}").ok();
        writeln!(script, "        return").ok();
        writeln!(script, "    }}").ok();
        writeln!(script).ok();
        writeln!(script, "    if ($commands.ContainsKey($command)) {{").ok();
        writeln!(script, "        $cmdInfo = $commands[$command]").ok();
        writeln!(script).ok();
        writeln!(script, "        if ($wordToComplete -like '-*') {{").ok();
        writeln!(script, "            # Complete options").ok();
        writeln!(
            script,
            "            $cmdInfo.Options | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{"
        )
            .ok();
        writeln!(
            script,
            "                [System.Management.Automation.CompletionResult]::new("
        )
            .ok();
        writeln!(script, "                    $_,").ok();
        writeln!(script, "                    $_,").ok();
        writeln!(
            script,
            "                    [System.Management.Automation.CompletionResultType]::ParameterName,"
        )
            .ok();
        writeln!(script, "                    $_").ok();
        writeln!(script, "                )").ok();
        writeln!(script, "            }}").ok();
        writeln!(script, "        }} elseif ($cmdInfo.Subcommands) {{").ok();
        writeln!(script, "            # Complete subcommands").ok();
        writeln!(
            script,
            "            $cmdInfo.Subcommands.Keys | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{"
        )
            .ok();
        writeln!(script, "                $desc = $cmdInfo.Subcommands[$_].Description")
            .ok();
        writeln!(
            script,
            "                [System.Management.Automation.CompletionResult]::new("
        )
            .ok();
        writeln!(script, "                    $_,").ok();
        writeln!(script, "                    $_,").ok();
        writeln!(
            script,
            "                    [System.Management.Automation.CompletionResultType]::ParameterValue,"
        )
            .ok();
        writeln!(script, "                    $desc").ok();
        writeln!(script, "                )").ok();
        writeln!(script, "            }}").ok();
        writeln!(script, "        }} else {{").ok();
        writeln!(script, "            # Complete files").ok();
        writeln!(
            script,
            "            Get-OxigdalFileCompletion -WordToComplete $wordToComplete -FileType $cmdInfo.FileType"
        )
            .ok();
        writeln!(script, "        }}").ok();
        writeln!(script, "    }}").ok();
        writeln!(script, "}}").ok();
        out.write_all(script.as_bytes())
    }
}
/// File argument type for context-aware completion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileArgType {
    /// No file argument
    None,
    /// Raster file argument
    Raster,
    /// Vector file argument
    Vector,
    /// Any geospatial file argument
    Any,
    /// Generic file (any extension)
    Generic,
}
/// Supported shell types for completion generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// PowerShell
    PowerShell,
}
/// Dynamic completion suggestions based on context
#[derive(Debug, Default)]
pub struct DynamicCompleter {
    /// CRS suggestions cache
    crs_cache: HashMap<String, Vec<String>>,
    /// EPSG code suggestions
    epsg_codes: Vec<(u32, &'static str)>,
}
impl DynamicCompleter {
    /// Create a new dynamic completer
    #[must_use]
    pub fn new() -> Self {
        Self {
            crs_cache: HashMap::new(),
            epsg_codes: Self::build_common_epsg_codes(),
        }
    }
    /// Build common EPSG codes for suggestion
    fn build_common_epsg_codes() -> Vec<(u32, &'static str)> {
        vec![
            (4326, "WGS 84"), (3857, "Web Mercator"), (32601, "WGS 84 / UTM zone 1N"),
            (32610, "WGS 84 / UTM zone 10N"), (32611, "WGS 84 / UTM zone 11N"), (32618,
            "WGS 84 / UTM zone 18N"), (32633, "WGS 84 / UTM zone 33N"), (2154,
            "RGF93 / Lambert-93"), (27700, "British National Grid"), (28992,
            "Amersfoort / RD New"), (25832, "ETRS89 / UTM zone 32N"), (25833,
            "ETRS89 / UTM zone 33N"), (3035, "ETRS89-extended / LAEA Europe"), (4269,
            "NAD83"), (2163, "US National Atlas Equal Area"), (5070,
            "NAD83 / Conus Albers"), (6668, "JGD2011"), (6674,
            "JGD2011 / Japan Plane Zone 9"),
        ]
    }
    /// Get CRS suggestions for a partial input
    #[must_use]
    pub fn suggest_crs(&self, partial: &str) -> Vec<String> {
        let partial_upper = partial.to_uppercase();
        if partial_upper.starts_with("EPSG:") {
            let prefix = partial_upper.strip_prefix("EPSG:").unwrap_or("");
            self.epsg_codes
                .iter()
                .filter(|(code, _)| code.to_string().starts_with(prefix))
                .map(|(code, desc)| format!("EPSG:{} ({})", code, desc))
                .take(10)
                .collect()
        } else if partial.is_empty() || partial_upper.starts_with('E') {
            vec![
                "EPSG:4326 (WGS 84)".to_string(), "EPSG:3857 (Web Mercator)".to_string(),
            ]
        } else {
            vec![]
        }
    }
    /// Get resampling method suggestions
    #[must_use]
    pub fn suggest_resampling(&self, partial: &str) -> Vec<String> {
        let methods = [
            ("nearest", "Nearest neighbor (fastest, categorical data)"),
            ("bilinear", "Bilinear interpolation (smooth, continuous data)"),
            ("bicubic", "Bicubic interpolation (smoother, photos)"),
            ("lanczos", "Lanczos windowed sinc (sharpest, high quality)"),
            ("average", "Average of contributing pixels"),
            ("mode", "Most frequent value (categorical data)"),
            ("max", "Maximum value"),
            ("min", "Minimum value"),
            ("med", "Median value"),
            ("q1", "First quartile"),
            ("q3", "Third quartile"),
        ];
        methods
            .iter()
            .filter(|(name, _)| name.starts_with(&partial.to_lowercase()))
            .map(|(name, desc)| format!("{} - {}", name, desc))
            .collect()
    }
    /// Get compression method suggestions
    #[must_use]
    pub fn suggest_compression(&self, partial: &str) -> Vec<String> {
        let methods = [
            ("none", "No compression"),
            ("lzw", "LZW compression (lossless, good for many datasets)"),
            ("deflate", "DEFLATE/ZIP compression (lossless, smaller files)"),
            ("zstd", "Zstandard compression (lossless, fast, small files)"),
            ("jpeg", "JPEG compression (lossy, photos/imagery)"),
            ("webp", "WebP compression (lossy/lossless, modern)"),
            ("lerc", "Limited Error Raster Compression"),
            ("packbits", "PackBits compression (simple, fast)"),
        ];
        methods
            .iter()
            .filter(|(name, _)| name.starts_with(&partial.to_lowercase()))
            .map(|(name, desc)| format!("{} - {}", name, desc))
            .collect()
    }
    /// Get data type suggestions
    #[must_use]
    pub fn suggest_data_type(&self, partial: &str) -> Vec<String> {
        let types = [
            ("uint8", "Unsigned 8-bit integer (0-255)"),
            ("int8", "Signed 8-bit integer (-128 to 127)"),
            ("uint16", "Unsigned 16-bit integer (0-65535)"),
            ("int16", "Signed 16-bit integer"),
            ("uint32", "Unsigned 32-bit integer"),
            ("int32", "Signed 32-bit integer"),
            ("uint64", "Unsigned 64-bit integer"),
            ("int64", "Signed 64-bit integer"),
            ("float32", "32-bit floating point"),
            ("float64", "64-bit floating point (double precision)"),
            ("cfloat32", "Complex 32-bit floating point"),
            ("cfloat64", "Complex 64-bit floating point"),
        ];
        types
            .iter()
            .filter(|(name, _)| name.starts_with(&partial.to_lowercase()))
            .map(|(name, desc)| format!("{} - {}", name, desc))
            .collect()
    }
}
/// File path completion helper for geospatial formats
#[derive(Debug, Default)]
pub struct FilePathCompleter {
    /// Geospatial format definitions
    formats: GeoFormats,
}
impl FilePathCompleter {
    /// Create a new file path completer
    #[must_use]
    pub fn new() -> Self {
        Self { formats: GeoFormats::new() }
    }
    /// Check if a file matches geospatial format
    #[must_use]
    pub fn is_geospatial_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| self.formats.all.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
    }
    /// Check if a file is a raster format
    #[must_use]
    pub fn is_raster_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                self.formats.raster.iter().any(|(e, _)| *e == ext.to_lowercase())
            })
            .unwrap_or(false)
    }
    /// Check if a file is a vector format
    #[must_use]
    pub fn is_vector_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                self.formats.vector.iter().any(|(e, _)| *e == ext.to_lowercase())
            })
            .unwrap_or(false)
    }
    /// Get format description for a file
    #[must_use]
    pub fn get_format_description(&self, path: &Path) -> Option<&'static str> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        self.formats
            .raster
            .iter()
            .chain(self.formats.vector.iter())
            .find(|(e, _)| *e == ext)
            .map(|(_, desc)| *desc)
    }
    /// List files matching the specified type in a directory
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub fn list_files(
        &self,
        dir: &Path,
        file_type: FileArgType,
    ) -> io::Result<Vec<String>> {
        let entries = std::fs::read_dir(dir)?;
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name() {
                    if let Some(name_str) = name.to_str() {
                        files.push(format!("{}/", name_str));
                    }
                }
            } else {
                let matches = match file_type {
                    FileArgType::Raster => self.is_raster_file(&path),
                    FileArgType::Vector => self.is_vector_file(&path),
                    FileArgType::Any => self.is_geospatial_file(&path),
                    FileArgType::Generic => true,
                    FileArgType::None => false,
                };
                if matches {
                    if let Some(name) = path.file_name() {
                        if let Some(name_str) = name.to_str() {
                            files.push(name_str.to_string());
                        }
                    }
                }
            }
        }
        files.sort();
        Ok(files)
    }
}
/// Option definition for completion generation
#[derive(Debug, Clone)]
pub struct OptionDef {
    /// Short flag (e.g., "-v")
    pub short: Option<&'static str>,
    /// Long flag (e.g., "--verbose")
    pub long: Option<&'static str>,
    /// Description
    pub description: &'static str,
    /// Whether the option takes a value
    pub takes_value: bool,
    /// Possible values (for enum-like options)
    pub possible_values: Vec<&'static str>,
}
