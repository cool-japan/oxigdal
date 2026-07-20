//! Common geospatial workflow patterns and templates.
//!
//! This module provides predefined workflow templates for common geospatial
//! operations including:
//! - ETL (Extract, Transform, Load) workflows
//! - Mosaic operations
//! - Reprojection pipelines
//! - Raster calculations
//! - Vector processing
//! - Tile generation

use crate::dag::{ResourceRequirements, RetryPolicy, TaskNode, WorkflowDag};
use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use crate::templates::{
    Parameter, ParameterConstraints, ParameterType, ParameterValue, TemplateBuilder,
    TemplateCategory, WorkflowTemplate,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Geospatial workflow pattern types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeospatialPattern {
    /// Extract, Transform, Load pattern for geospatial data.
    Etl,
    /// Mosaic multiple rasters into a single output.
    Mosaic,
    /// Reproject data between coordinate reference systems.
    Reproject,
    /// Raster algebra and band calculations.
    RasterCalc,
    /// Vector data processing pipeline.
    VectorProcessing,
    /// Tile generation for web mapping.
    TileGeneration,
    /// Image classification pipeline.
    Classification,
    /// Time series analysis for multi-temporal data.
    TimeSeries,
    /// DEM-based terrain analysis.
    TerrainAnalysis,
    /// Point cloud processing pipeline.
    PointCloud,
}

/// Coordinate reference system specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrsSpec {
    /// EPSG code or WKT definition.
    pub definition: String,
    /// Whether this is an EPSG code.
    pub is_epsg: bool,
    /// Axis order specification.
    pub axis_order: AxisOrder,
}

impl CrsSpec {
    /// Create a new CRS specification from an EPSG code.
    pub fn from_epsg(code: u32) -> Self {
        Self {
            definition: format!("EPSG:{}", code),
            is_epsg: true,
            axis_order: AxisOrder::EastNorth,
        }
    }

    /// Create a CRS specification from WKT.
    pub fn from_wkt(wkt: String) -> Self {
        Self {
            definition: wkt,
            is_epsg: false,
            axis_order: AxisOrder::EastNorth,
        }
    }

    /// Get the EPSG code if available.
    pub fn epsg_code(&self) -> Option<u32> {
        if self.is_epsg {
            self.definition
                .strip_prefix("EPSG:")
                .and_then(|s| s.parse().ok())
        } else {
            None
        }
    }
}

/// Axis order for coordinate systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisOrder {
    /// East-North (X-Y) ordering.
    EastNorth,
    /// North-East (Y-X) ordering.
    NorthEast,
    /// Custom axis ordering.
    Custom(Vec<String>),
}

/// Resampling method for raster operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResamplingMethod {
    /// Nearest neighbor interpolation.
    NearestNeighbor,
    /// Bilinear interpolation.
    Bilinear,
    /// Bicubic interpolation.
    Bicubic,
    /// Cubic spline interpolation.
    CubicSpline,
    /// Lanczos windowed sinc.
    Lanczos,
    /// Average of contributing pixels.
    Average,
    /// Mode (most common value).
    Mode,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Median value.
    Median,
    /// Sum of values.
    Sum,
}

impl ResamplingMethod {
    /// Get the GDAL algorithm name.
    pub fn gdal_name(&self) -> &'static str {
        match self {
            Self::NearestNeighbor => "near",
            Self::Bilinear => "bilinear",
            Self::Bicubic => "cubic",
            Self::CubicSpline => "cubicspline",
            Self::Lanczos => "lanczos",
            Self::Average => "average",
            Self::Mode => "mode",
            Self::Min => "min",
            Self::Max => "max",
            Self::Median => "med",
            Self::Sum => "sum",
        }
    }
}

/// Output format specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFormat {
    /// Format driver name (e.g., "GTiff", "COG", "GPKG").
    pub driver: String,
    /// Creation options.
    pub creation_options: HashMap<String, String>,
    /// Compression type.
    pub compression: Option<CompressionType>,
    /// Tile size for tiled formats.
    pub tile_size: Option<(u32, u32)>,
}

impl OutputFormat {
    /// Create a Cloud-Optimized GeoTIFF format specification.
    pub fn cog() -> Self {
        let mut options = HashMap::new();
        options.insert("COMPRESS".to_string(), "LZW".to_string());
        options.insert("TILED".to_string(), "YES".to_string());
        options.insert("COPY_SRC_OVERVIEWS".to_string(), "YES".to_string());

        Self {
            driver: "COG".to_string(),
            creation_options: options,
            compression: Some(CompressionType::Lzw),
            tile_size: Some((512, 512)),
        }
    }

    /// Create a standard GeoTIFF format specification.
    pub fn geotiff() -> Self {
        let mut options = HashMap::new();
        options.insert("COMPRESS".to_string(), "DEFLATE".to_string());
        options.insert("TILED".to_string(), "YES".to_string());

        Self {
            driver: "GTiff".to_string(),
            creation_options: options,
            compression: Some(CompressionType::Deflate),
            tile_size: Some((256, 256)),
        }
    }

    /// Create a GeoPackage format specification.
    pub fn geopackage() -> Self {
        Self {
            driver: "GPKG".to_string(),
            creation_options: HashMap::new(),
            compression: None,
            tile_size: None,
        }
    }
}

/// Compression types for raster data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionType {
    /// No compression.
    None,
    /// LZW compression.
    Lzw,
    /// DEFLATE compression.
    Deflate,
    /// ZSTD compression.
    Zstd,
    /// JPEG compression.
    Jpeg,
    /// JPEG 2000 compression.
    Jp2,
    /// WebP compression.
    Webp,
}

/// ETL workflow configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtlConfig {
    /// Source data paths or patterns.
    pub sources: Vec<String>,
    /// Destination path.
    pub destination: String,
    /// Transformation steps.
    pub transforms: Vec<TransformStep>,
    /// Output format.
    pub output_format: OutputFormat,
    /// Whether to validate input data.
    pub validate_input: bool,
    /// Whether to create overviews.
    pub create_overviews: bool,
    /// Parallel processing level.
    pub parallelism: u32,
}

/// Transformation step in an ETL pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformStep {
    /// Step name.
    pub name: String,
    /// Step type.
    pub step_type: TransformType,
    /// Step parameters.
    pub params: HashMap<String, serde_json::Value>,
}

/// Types of transformation operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformType {
    /// Reproject to a different CRS.
    Reproject,
    /// Clip to a boundary.
    Clip,
    /// Apply a raster calculation.
    Calculate,
    /// Resample to different resolution.
    Resample,
    /// Apply a color map.
    ColorMap,
    /// Filter data by attribute or value.
    Filter,
    /// Convert data type.
    Convert,
    /// Apply a buffer.
    Buffer,
    /// Simplify geometry.
    Simplify,
}

/// Mosaic configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MosaicConfig {
    /// Input raster paths.
    pub inputs: Vec<String>,
    /// Output path.
    pub output: String,
    /// NoData value.
    pub nodata: Option<f64>,
    /// Resampling method.
    pub resampling: ResamplingMethod,
    /// Output resolution (optional, uses input resolution if not specified).
    pub target_resolution: Option<(f64, f64)>,
    /// Target CRS (optional, uses first input CRS if not specified).
    pub target_crs: Option<CrsSpec>,
    /// Blend mode for overlapping areas.
    pub blend_mode: BlendMode,
    /// Output format.
    pub output_format: OutputFormat,
}

/// Blend mode for mosaic operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    /// First valid value (from input order).
    First,
    /// Last valid value (from input order).
    Last,
    /// Average of overlapping values.
    Average,
    /// Maximum value.
    Max,
    /// Minimum value.
    Min,
}

/// Reprojection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReprojectConfig {
    /// Source CRS.
    pub source_crs: Option<CrsSpec>,
    /// Target CRS.
    pub target_crs: CrsSpec,
    /// Resampling method.
    pub resampling: ResamplingMethod,
    /// Target resolution (optional).
    pub target_resolution: Option<(f64, f64)>,
    /// Target extent (optional, [xmin, ymin, xmax, ymax]).
    pub target_extent: Option<[f64; 4]>,
    /// Error threshold for approximation.
    pub error_threshold: f64,
    /// Memory limit in MB.
    pub memory_limit_mb: u32,
}

impl Default for ReprojectConfig {
    fn default() -> Self {
        Self {
            source_crs: None,
            target_crs: CrsSpec::from_epsg(4326),
            resampling: ResamplingMethod::Bilinear,
            target_resolution: None,
            target_extent: None,
            error_threshold: 0.125,
            memory_limit_mb: 500,
        }
    }
}

/// Factory for creating geospatial workflow templates.
pub struct GeospatialTemplateFactory;

impl GeospatialTemplateFactory {
    /// Create an ETL workflow template.
    pub fn create_etl_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "geospatial-etl",
            "Geospatial ETL Pipeline",
            "Extract, transform, and load geospatial data with validation and optimization",
        );

        template.set_category(TemplateCategory::Etl);
        template.metadata.complexity = 3;
        template.add_tag("etl");
        template.add_tag("geospatial");
        template.add_tag("pipeline");

        // Source parameters
        template.add_parameter(Parameter {
            name: "source_paths".to_string(),
            param_type: ParameterType::Array,
            description: "List of source data paths or patterns".to_string(),
            required: true,
            default_value: None,
            constraints: Some(ParameterConstraints {
                min: None,
                max: None,
                min_length: Some(1),
                max_length: Some(1000),
                pattern: None,
            }),
        });

        // Destination parameter
        template.add_parameter(Parameter {
            name: "destination".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output destination path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Output format
        template.add_parameter(Parameter {
            name: "output_format".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "COG".to_string(),
                    "GTiff".to_string(),
                    "GPKG".to_string(),
                    "Parquet".to_string(),
                ],
            },
            description: "Output format driver".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("COG".to_string())),
            constraints: None,
        });

        // Validation flag
        template.add_parameter(Parameter {
            name: "validate_input".to_string(),
            param_type: ParameterType::Boolean,
            description: "Validate input data before processing".to_string(),
            required: false,
            default_value: Some(ParameterValue::Boolean(true)),
            constraints: None,
        });

        // Parallelism
        template.add_parameter(Parameter {
            name: "parallelism".to_string(),
            param_type: ParameterType::Integer,
            description: "Number of parallel workers".to_string(),
            required: false,
            default_value: Some(ParameterValue::Integer(4)),
            constraints: Some(ParameterConstraints {
                min: Some(1.0),
                max: Some(64.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        template.set_template(Self::etl_template_json());

        Ok(template)
    }

    /// Create a mosaic workflow template.
    pub fn create_mosaic_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "geospatial-mosaic",
            "Raster Mosaic Pipeline",
            "Combine multiple rasters into a seamless mosaic with configurable blending",
        );

        template.set_category(TemplateCategory::BatchProcessing);
        template.metadata.complexity = 3;
        template.add_tag("mosaic");
        template.add_tag("raster");
        template.add_tag("merge");

        // Input files
        template.add_parameter(Parameter {
            name: "input_files".to_string(),
            param_type: ParameterType::Array,
            description: "List of input raster files to mosaic".to_string(),
            required: true,
            default_value: None,
            constraints: Some(ParameterConstraints {
                min: None,
                max: None,
                min_length: Some(2),
                max_length: Some(10000),
                pattern: None,
            }),
        });

        // Output path
        template.add_parameter(Parameter {
            name: "output_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output mosaic file path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // NoData value
        template.add_parameter(Parameter {
            name: "nodata".to_string(),
            param_type: ParameterType::Float,
            description: "NoData value for output".to_string(),
            required: false,
            default_value: Some(ParameterValue::Float(-9999.0)),
            constraints: None,
        });

        // Resampling method
        template.add_parameter(Parameter {
            name: "resampling".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "near".to_string(),
                    "bilinear".to_string(),
                    "cubic".to_string(),
                    "cubicspline".to_string(),
                    "lanczos".to_string(),
                    "average".to_string(),
                ],
            },
            description: "Resampling method for warping".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("bilinear".to_string())),
            constraints: None,
        });

        // Blend mode
        template.add_parameter(Parameter {
            name: "blend_mode".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "first".to_string(),
                    "last".to_string(),
                    "average".to_string(),
                    "max".to_string(),
                    "min".to_string(),
                ],
            },
            description: "Blend mode for overlapping areas".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("first".to_string())),
            constraints: None,
        });

        // Target CRS
        template.add_parameter(Parameter {
            name: "target_crs".to_string(),
            param_type: ParameterType::String,
            description: "Target coordinate reference system (EPSG code or WKT)".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("EPSG:4326".to_string())),
            constraints: None,
        });

        template.set_template(Self::mosaic_template_json());

        Ok(template)
    }

    /// Create a reprojection workflow template.
    pub fn create_reproject_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "geospatial-reproject",
            "Coordinate Reprojection Pipeline",
            "Transform geospatial data between coordinate reference systems",
        );

        template.set_category(TemplateCategory::SatelliteProcessing);
        template.metadata.complexity = 2;
        template.add_tag("reproject");
        template.add_tag("crs");
        template.add_tag("transform");

        // Input path
        template.add_parameter(Parameter {
            name: "input_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Input file or directory path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Output path
        template.add_parameter(Parameter {
            name: "output_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output file or directory path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Source CRS
        template.add_parameter(Parameter {
            name: "source_crs".to_string(),
            param_type: ParameterType::String,
            description: "Source CRS (optional, auto-detect if not provided)".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("auto".to_string())),
            constraints: None,
        });

        // Target CRS
        template.add_parameter(Parameter {
            name: "target_crs".to_string(),
            param_type: ParameterType::String,
            description: "Target CRS (EPSG code or WKT)".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Resampling method
        template.add_parameter(Parameter {
            name: "resampling".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "near".to_string(),
                    "bilinear".to_string(),
                    "cubic".to_string(),
                    "cubicspline".to_string(),
                    "lanczos".to_string(),
                ],
            },
            description: "Resampling algorithm".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("bilinear".to_string())),
            constraints: None,
        });

        // Target resolution
        template.add_parameter(Parameter {
            name: "target_resolution_x".to_string(),
            param_type: ParameterType::Float,
            description: "Target X resolution (optional)".to_string(),
            required: false,
            default_value: None,
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: None,
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        template.add_parameter(Parameter {
            name: "target_resolution_y".to_string(),
            param_type: ParameterType::Float,
            description: "Target Y resolution (optional)".to_string(),
            required: false,
            default_value: None,
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: None,
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        template.set_template(Self::reproject_template_json());

        Ok(template)
    }

    /// Create a tile generation workflow template.
    pub fn create_tile_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "geospatial-tiles",
            "Map Tile Generation Pipeline",
            "Generate XYZ/TMS map tiles from raster data",
        );

        template.set_category(TemplateCategory::BatchProcessing);
        template.metadata.complexity = 3;
        template.add_tag("tiles");
        template.add_tag("xyz");
        template.add_tag("web-mapping");

        // Input path
        template.add_parameter(Parameter {
            name: "input_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Input raster file path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Output directory
        template.add_parameter(Parameter {
            name: "output_directory".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output tile directory".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Min zoom
        template.add_parameter(Parameter {
            name: "min_zoom".to_string(),
            param_type: ParameterType::Integer,
            description: "Minimum zoom level".to_string(),
            required: false,
            default_value: Some(ParameterValue::Integer(0)),
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: Some(24.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        // Max zoom
        template.add_parameter(Parameter {
            name: "max_zoom".to_string(),
            param_type: ParameterType::Integer,
            description: "Maximum zoom level".to_string(),
            required: false,
            default_value: Some(ParameterValue::Integer(18)),
            constraints: Some(ParameterConstraints {
                min: Some(0.0),
                max: Some(24.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        // Tile format
        template.add_parameter(Parameter {
            name: "tile_format".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "png".to_string(),
                    "jpeg".to_string(),
                    "webp".to_string(),
                ],
            },
            description: "Output tile format".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("png".to_string())),
            constraints: None,
        });

        // Tile size
        template.add_parameter(Parameter {
            name: "tile_size".to_string(),
            param_type: ParameterType::Integer,
            description: "Tile size in pixels".to_string(),
            required: false,
            default_value: Some(ParameterValue::Integer(256)),
            constraints: Some(ParameterConstraints {
                min: Some(64.0),
                max: Some(4096.0),
                min_length: None,
                max_length: None,
                pattern: None,
            }),
        });

        template.set_template(Self::tile_template_json());

        Ok(template)
    }

    /// Create a raster calculation workflow template.
    pub fn create_raster_calc_template() -> Result<WorkflowTemplate> {
        let mut template = WorkflowTemplate::new(
            "geospatial-raster-calc",
            "Raster Calculator Pipeline",
            "Perform band algebra and raster calculations",
        );

        template.set_category(TemplateCategory::SatelliteProcessing);
        template.metadata.complexity = 2;
        template.add_tag("raster");
        template.add_tag("calculation");
        template.add_tag("band-math");

        // Input path
        template.add_parameter(Parameter {
            name: "input_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Input raster file path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Expression
        template.add_parameter(Parameter {
            name: "expression".to_string(),
            param_type: ParameterType::String,
            description: "Calculation expression (e.g., '(B4-B3)/(B4+B3)' for NDVI)".to_string(),
            required: true,
            default_value: None,
            constraints: Some(ParameterConstraints {
                min: None,
                max: None,
                min_length: Some(1),
                max_length: Some(1000),
                pattern: None,
            }),
        });

        // Output path
        template.add_parameter(Parameter {
            name: "output_path".to_string(),
            param_type: ParameterType::FilePath,
            description: "Output raster file path".to_string(),
            required: true,
            default_value: None,
            constraints: None,
        });

        // Output data type
        template.add_parameter(Parameter {
            name: "output_dtype".to_string(),
            param_type: ParameterType::Enum {
                allowed_values: vec![
                    "Float32".to_string(),
                    "Float64".to_string(),
                    "Int16".to_string(),
                    "Int32".to_string(),
                    "UInt8".to_string(),
                    "UInt16".to_string(),
                ],
            },
            description: "Output data type".to_string(),
            required: false,
            default_value: Some(ParameterValue::String("Float32".to_string())),
            constraints: None,
        });

        template.set_template(Self::raster_calc_template_json());

        Ok(template)
    }

    /// ETL template JSON.
    fn etl_template_json() -> &'static str {
        r#"{
            "id": "etl-{{workflow_id}}",
            "name": "{{workflow_name}}",
            "version": "1.0.0",
            "dag": {
                "nodes": [
                    {
                        "id": "validate",
                        "name": "Validate Input",
                        "config": {
                            "operation": "validate",
                            "sources": "{{source_paths}}"
                        }
                    },
                    {
                        "id": "extract",
                        "name": "Extract Data",
                        "config": {
                            "operation": "extract",
                            "sources": "{{source_paths}}"
                        }
                    },
                    {
                        "id": "transform",
                        "name": "Transform Data",
                        "config": {
                            "operation": "transform",
                            "parallelism": "{{parallelism}}"
                        }
                    },
                    {
                        "id": "load",
                        "name": "Load Data",
                        "config": {
                            "operation": "load",
                            "destination": "{{destination}}",
                            "format": "{{output_format}}"
                        }
                    }
                ],
                "edges": [
                    {"from": "validate", "to": "extract"},
                    {"from": "extract", "to": "transform"},
                    {"from": "transform", "to": "load"}
                ]
            },
            "description": "ETL workflow for geospatial data processing"
        }"#
    }

    /// Mosaic template JSON.
    fn mosaic_template_json() -> &'static str {
        r#"{
            "id": "mosaic-{{workflow_id}}",
            "name": "{{workflow_name}}",
            "version": "1.0.0",
            "dag": {
                "nodes": [
                    {
                        "id": "collect",
                        "name": "Collect Inputs",
                        "config": {
                            "operation": "collect",
                            "inputs": "{{input_files}}"
                        }
                    },
                    {
                        "id": "analyze",
                        "name": "Analyze Extents",
                        "config": {
                            "operation": "analyze_extents",
                            "target_crs": "{{target_crs}}"
                        }
                    },
                    {
                        "id": "mosaic",
                        "name": "Create Mosaic",
                        "config": {
                            "operation": "mosaic",
                            "nodata": "{{nodata}}",
                            "resampling": "{{resampling}}",
                            "blend_mode": "{{blend_mode}}"
                        }
                    },
                    {
                        "id": "output",
                        "name": "Write Output",
                        "config": {
                            "operation": "write",
                            "output_path": "{{output_path}}"
                        }
                    }
                ],
                "edges": [
                    {"from": "collect", "to": "analyze"},
                    {"from": "analyze", "to": "mosaic"},
                    {"from": "mosaic", "to": "output"}
                ]
            },
            "description": "Mosaic workflow for combining multiple rasters"
        }"#
    }

    /// Reproject template JSON.
    fn reproject_template_json() -> &'static str {
        r#"{
            "id": "reproject-{{workflow_id}}",
            "name": "{{workflow_name}}",
            "version": "1.0.0",
            "dag": {
                "nodes": [
                    {
                        "id": "detect_crs",
                        "name": "Detect Source CRS",
                        "config": {
                            "operation": "detect_crs",
                            "input_path": "{{input_path}}",
                            "source_crs": "{{source_crs}}"
                        }
                    },
                    {
                        "id": "reproject",
                        "name": "Reproject Data",
                        "config": {
                            "operation": "reproject",
                            "target_crs": "{{target_crs}}",
                            "resampling": "{{resampling}}"
                        }
                    },
                    {
                        "id": "write",
                        "name": "Write Output",
                        "config": {
                            "operation": "write",
                            "output_path": "{{output_path}}"
                        }
                    }
                ],
                "edges": [
                    {"from": "detect_crs", "to": "reproject"},
                    {"from": "reproject", "to": "write"}
                ]
            },
            "description": "Reprojection workflow for CRS transformation"
        }"#
    }

    /// Tile generation template JSON.
    fn tile_template_json() -> &'static str {
        r#"{
            "id": "tiles-{{workflow_id}}",
            "name": "{{workflow_name}}",
            "version": "1.0.0",
            "dag": {
                "nodes": [
                    {
                        "id": "prepare",
                        "name": "Prepare Input",
                        "config": {
                            "operation": "prepare_for_tiling",
                            "input_path": "{{input_path}}"
                        }
                    },
                    {
                        "id": "tile",
                        "name": "Generate Tiles",
                        "config": {
                            "operation": "generate_tiles",
                            "min_zoom": "{{min_zoom}}",
                            "max_zoom": "{{max_zoom}}",
                            "tile_format": "{{tile_format}}",
                            "tile_size": "{{tile_size}}"
                        }
                    },
                    {
                        "id": "output",
                        "name": "Write Tiles",
                        "config": {
                            "operation": "write_tiles",
                            "output_directory": "{{output_directory}}"
                        }
                    }
                ],
                "edges": [
                    {"from": "prepare", "to": "tile"},
                    {"from": "tile", "to": "output"}
                ]
            },
            "description": "Tile generation workflow for web mapping"
        }"#
    }

    /// Raster calculation template JSON.
    fn raster_calc_template_json() -> &'static str {
        r#"{
            "id": "raster-calc-{{workflow_id}}",
            "name": "{{workflow_name}}",
            "version": "1.0.0",
            "dag": {
                "nodes": [
                    {
                        "id": "read",
                        "name": "Read Input",
                        "config": {
                            "operation": "read_raster",
                            "input_path": "{{input_path}}"
                        }
                    },
                    {
                        "id": "calculate",
                        "name": "Calculate",
                        "config": {
                            "operation": "raster_calc",
                            "expression": "{{expression}}"
                        }
                    },
                    {
                        "id": "write",
                        "name": "Write Output",
                        "config": {
                            "operation": "write_raster",
                            "output_path": "{{output_path}}",
                            "dtype": "{{output_dtype}}"
                        }
                    }
                ],
                "edges": [
                    {"from": "read", "to": "calculate"},
                    {"from": "calculate", "to": "write"}
                ]
            },
            "description": "Raster calculation workflow"
        }"#
    }

    /// Create all geospatial templates and return them.
    pub fn create_all_templates() -> Vec<Result<WorkflowTemplate>> {
        vec![
            Self::create_etl_template(),
            Self::create_mosaic_template(),
            Self::create_reproject_template(),
            Self::create_tile_template(),
            Self::create_raster_calc_template(),
        ]
    }
}

/// Instantiation helper for geospatial templates.
pub struct GeospatialInstantiator {
    /// Default output format.
    default_format: OutputFormat,
    /// Default CRS.
    default_crs: CrsSpec,
    /// Default resampling method.
    default_resampling: ResamplingMethod,
}

impl GeospatialInstantiator {
    /// Create a new geospatial instantiator with defaults.
    pub fn new() -> Self {
        Self {
            default_format: OutputFormat::cog(),
            default_crs: CrsSpec::from_epsg(4326),
            default_resampling: ResamplingMethod::Bilinear,
        }
    }

    /// Set the default output format.
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.default_format = format;
        self
    }

    /// Set the default CRS.
    pub fn with_crs(mut self, crs: CrsSpec) -> Self {
        self.default_crs = crs;
        self
    }

    /// Set the default resampling method.
    pub fn with_resampling(mut self, resampling: ResamplingMethod) -> Self {
        self.default_resampling = resampling;
        self
    }

    /// Instantiate an ETL workflow.
    pub fn instantiate_etl(
        &self,
        template: &WorkflowTemplate,
        config: &EtlConfig,
    ) -> Result<WorkflowDefinition> {
        let mut params = HashMap::new();

        let sources_json = serde_json::to_value(&config.sources).map_err(|e| {
            WorkflowError::template(format!("Failed to serialize sources: {}", e))
        })?;

        params.insert(
            "source_paths".to_string(),
            ParameterValue::Array(
                config
                    .sources
                    .iter()
                    .map(|s| ParameterValue::String(s.clone()))
                    .collect(),
            ),
        );
        params.insert(
            "destination".to_string(),
            ParameterValue::String(config.destination.clone()),
        );
        params.insert(
            "output_format".to_string(),
            ParameterValue::String(config.output_format.driver.clone()),
        );
        params.insert(
            "validate_input".to_string(),
            ParameterValue::Boolean(config.validate_input),
        );
        params.insert(
            "parallelism".to_string(),
            ParameterValue::Integer(config.parallelism as i64),
        );

        template.instantiate(params)
    }

    /// Instantiate a mosaic workflow.
    pub fn instantiate_mosaic(
        &self,
        template: &WorkflowTemplate,
        config: &MosaicConfig,
    ) -> Result<WorkflowDefinition> {
        let mut params = HashMap::new();

        params.insert(
            "input_files".to_string(),
            ParameterValue::Array(
                config
                    .inputs
                    .iter()
                    .map(|s| ParameterValue::String(s.clone()))
                    .collect(),
            ),
        );
        params.insert(
            "output_path".to_string(),
            ParameterValue::String(config.output.clone()),
        );
        params.insert(
            "nodata".to_string(),
            ParameterValue::Float(config.nodata.unwrap_or(-9999.0)),
        );
        params.insert(
            "resampling".to_string(),
            ParameterValue::String(config.resampling.gdal_name().to_string()),
        );

        let blend_mode_str = match &config.blend_mode {
            BlendMode::First => "first",
            BlendMode::Last => "last",
            BlendMode::Average => "average",
            BlendMode::Max => "max",
            BlendMode::Min => "min",
        };
        params.insert(
            "blend_mode".to_string(),
            ParameterValue::String(blend_mode_str.to_string()),
        );

        if let Some(crs) = &config.target_crs {
            params.insert(
                "target_crs".to_string(),
                ParameterValue::String(crs.definition.clone()),
            );
        } else {
            params.insert(
                "target_crs".to_string(),
                ParameterValue::String(self.default_crs.definition.clone()),
            );
        }

        template.instantiate(params)
    }

    /// Instantiate a reprojection workflow.
    pub fn instantiate_reproject(
        &self,
        template: &WorkflowTemplate,
        input_path: &str,
        output_path: &str,
        config: &ReprojectConfig,
    ) -> Result<WorkflowDefinition> {
        let mut params = HashMap::new();

        params.insert(
            "input_path".to_string(),
            ParameterValue::String(input_path.to_string()),
        );
        params.insert(
            "output_path".to_string(),
            ParameterValue::String(output_path.to_string()),
        );

        if let Some(source_crs) = &config.source_crs {
            params.insert(
                "source_crs".to_string(),
                ParameterValue::String(source_crs.definition.clone()),
            );
        } else {
            params.insert(
                "source_crs".to_string(),
                ParameterValue::String("auto".to_string()),
            );
        }

        params.insert(
            "target_crs".to_string(),
            ParameterValue::String(config.target_crs.definition.clone()),
        );
        params.insert(
            "resampling".to_string(),
            ParameterValue::String(config.resampling.gdal_name().to_string()),
        );

        template.instantiate(params)
    }
}

impl Default for GeospatialInstantiator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crs_spec_from_epsg() {
        let crs = CrsSpec::from_epsg(4326);
        assert_eq!(crs.definition, "EPSG:4326");
        assert!(crs.is_epsg);
        assert_eq!(crs.epsg_code(), Some(4326));
    }

    #[test]
    fn test_crs_spec_from_wkt() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]]"#;
        let crs = CrsSpec::from_wkt(wkt.to_string());
        assert!(!crs.is_epsg);
        assert_eq!(crs.epsg_code(), None);
    }

    #[test]
    fn test_resampling_gdal_name() {
        assert_eq!(ResamplingMethod::NearestNeighbor.gdal_name(), "near");
        assert_eq!(ResamplingMethod::Bilinear.gdal_name(), "bilinear");
        assert_eq!(ResamplingMethod::Lanczos.gdal_name(), "lanczos");
    }

    #[test]
    fn test_output_format_cog() {
        let format = OutputFormat::cog();
        assert_eq!(format.driver, "COG");
        assert_eq!(format.tile_size, Some((512, 512)));
    }

    #[test]
    fn test_create_etl_template() {
        let template = GeospatialTemplateFactory::create_etl_template();
        assert!(template.is_ok());
        let template = template.expect("template creation failed");
        assert_eq!(template.id, "geospatial-etl");
    }

    #[test]
    fn test_create_mosaic_template() {
        let template = GeospatialTemplateFactory::create_mosaic_template();
        assert!(template.is_ok());
        let template = template.expect("template creation failed");
        assert_eq!(template.id, "geospatial-mosaic");
    }

    #[test]
    fn test_create_reproject_template() {
        let template = GeospatialTemplateFactory::create_reproject_template();
        assert!(template.is_ok());
        let template = template.expect("template creation failed");
        assert_eq!(template.id, "geospatial-reproject");
    }

    #[test]
    fn test_geospatial_instantiator_default() {
        let instantiator = GeospatialInstantiator::new();
        assert_eq!(instantiator.default_crs.definition, "EPSG:4326");
    }

    #[test]
    fn test_geospatial_instantiator_with_crs() {
        let instantiator = GeospatialInstantiator::new().with_crs(CrsSpec::from_epsg(32632));
        assert_eq!(instantiator.default_crs.definition, "EPSG:32632");
    }

    #[test]
    fn test_create_all_templates() {
        let templates = GeospatialTemplateFactory::create_all_templates();
        assert_eq!(templates.len(), 5);

        for template_result in templates {
            assert!(template_result.is_ok());
        }
    }

    #[test]
    fn test_reproject_config_default() {
        let config = ReprojectConfig::default();
        assert_eq!(config.target_crs.definition, "EPSG:4326");
        assert_eq!(config.resampling, ResamplingMethod::Bilinear);
    }
}
