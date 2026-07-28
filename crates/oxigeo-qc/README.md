# OxiGeo QC - Quality Control & Validation Suite

[![Crates.io](https://img.shields.io/crates/v/oxigeo-qc.svg)](https://crates.io/crates/oxigeo-qc)
[![Documentation](https://docs.rs/oxigeo-qc/badge.svg)](https://docs.rs/oxigeo-qc)
[![License](https://img.shields.io/crates/l/oxigeo-qc.svg)](LICENSE)
[![Rust 1.70+](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

A comprehensive quality control and validation suite for geospatial data in OxiGeo. Provides automated detection, reporting, and repair of data integrity issues for both raster and vector datasets, with configurable validation rules and multi-format report generation.

## Features

- **Raster Quality Control**: Completeness, consistency, spatial accuracy, NoData consistency, CRS/extent, cloud-optimized GeoTIFF (COG) compliance, and per-sensor radiometric range checks
- **Vector Quality Control**: Topology validation (Bentley-Ottmann self-intersection + STRtree-backed gap/overlap detection) and attribute completeness checks for vector features
- **Metadata Validation**: ISO 19115 and STAC metadata standards compliance checking
- **STAC Validation**: Item/Collection/Catalog schema validation (required fields, bbox/geometry consistency, datetime format, `eo:cloud_cover` range)
- **GeoPackage Validation**: OGC GeoPackage 1.4 compliance checks (application ID, user version, required tables)
- **Batch QC Mode**: Recursive directory scanning with per-extension dispatch and aggregated severity counts
- **Rules Engine**: Flexible, configurable quality rules via TOML with custom validators
- **Automatic Fixes**: Safe, strategy-based automatic repairs for common data quality issues
- **Multi-Format Reporting**: HTML and JSON report generation with severity classification
- **Error Handling**: Comprehensive error types with severity levels (Info, Warning, Minor, Major, Critical)
- **Issue Tracking**: Detailed issue detection with location information and remediation suggestions
- **100% Pure Rust**: No C/Fortran dependencies, fully integrated with OxiGeo ecosystem

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigeo-qc = "0.2.1"
```

### With Optional Features

```toml
[dependencies]
oxigeo-qc = { version = "0.2.1", features = ["html"] }
```

**Available Features:**
- `std` (default): Standard library support
- `html` (default): HTML report generation with XML formatting
- `pdf`: PDF report generation (future)

## Quick Start

```rust
use oxigeo_qc::prelude::*;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::RasterDataType;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a raster buffer
    let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);

    // Check raster completeness
    let checker = CompletenessChecker::new();
    let result = checker.check_buffer(&buffer)?;

    println!("Valid pixels: {}/{}", result.valid_pixels, result.total_pixels);
    println!("Completeness: {:.2}%", result.completeness * 100.0);

    Ok(())
}
```

## Usage

### Basic Raster Quality Control

```rust
use oxigeo_qc::prelude::*;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{RasterDataType, BoundingBox, GeoTransform};

// Check raster completeness (valid vs invalid pixels)
let buffer = RasterBuffer::zeros(500, 500, RasterDataType::Float32);
let completeness_checker = CompletenessChecker::new();
let completeness_result = completeness_checker.check_buffer(&buffer)?;

// Check raster consistency (data type, CRS, bounds alignment)
let consistency_checker = ConsistencyChecker::new();
let consistency_result = consistency_checker.check_buffer(&buffer)?;

// Check spatial accuracy with geographic bounds
let bbox = BoundingBox::new(0.0, 0.0, 100.0, 100.0)?;
let geotransform = GeoTransform::from_bounds(&bbox, 500, 500)?;
let accuracy_checker = AccuracyChecker::new();
let accuracy_result = accuracy_checker.check_raster(&buffer, &geotransform, Some(&bbox))?;
```

### Vector Quality Control

```rust
use oxigeo_qc::prelude::*;
use oxigeo_core::vector::{Feature, FeatureCollection, Geometry, Point};

// Create a feature collection
let point = Point::new(10.5, 20.3);
let feature = Feature::new(Geometry::Point(point));
let collection = FeatureCollection {
    features: vec![feature],
    metadata: None,
};

// Check topology validity
let topo_checker = TopologyChecker::new();
let topo_result = topo_checker.validate(&collection)?;

println!("Invalid geometries: {}", topo_result.invalid_geometries);

// Check attribute completeness
let attr_checker = AttributionChecker::new();
let attr_result = attr_checker.validate(&collection)?;

println!("Missing attributes: {}", attr_result.missing_attributes);
```

### Metadata Validation

```rust
use oxigeo_qc::prelude::*;
use std::collections::HashMap;

let mut metadata = HashMap::new();
metadata.insert("title".to_string(), "Elevation Dataset".to_string());
metadata.insert("abstract".to_string(), "Global elevation model".to_string());
metadata.insert("topic_category".to_string(), "elevation".to_string());
metadata.insert("contact".to_string(), "data@example.com".to_string());
metadata.insert("date".to_string(), "2024-01-15".to_string());
metadata.insert("spatial_extent".to_string(), "-180,-90,180,90".to_string());

let checker = MetadataChecker::new();
let result = checker.check(&metadata)?;

println!("Missing fields: {}", result.required_fields_missing);
println!("Metadata assessment: {:?}", result.assessment);
```

### Rules Engine with Custom Validators

```rust
use oxigeo_qc::prelude::*;
use std::collections::HashMap;

// Create a rule set
let mut ruleset = RuleSet::new("Data Quality Rules", "Custom validation rules");

// Add a threshold rule
let rule = RuleBuilder::new("MAX_VAL", "Maximum Value Check")
    .description("Ensures raster values don't exceed 1000")
    .category(RuleCategory::Raster)
    .severity(Severity::Major)
    .threshold("max_value", ComparisonOperator::LessThanOrEqual, 1000.0)
    .build();

ruleset.add_rule(rule);

// Execute rules against data
let engine = RulesEngine::new(ruleset);
let mut data = HashMap::new();
data.insert("max_value".to_string(), QcValue::Number(950.0));

let issues = engine.execute_all(&data)?;
if issues.is_empty() {
    println!("All rules passed!");
}
```

### Automatic Fixing with Strategies

```rust
use oxigeo_qc::prelude::*;
use oxigeo_core::vector::{Feature, FeatureCollection, Geometry, Point};

let point = Point::new(0.0, 0.0);
let feature = Feature::new(Geometry::Point(point));
let collection = FeatureCollection {
    features: vec![feature],
    metadata: None,
};

// Use different fix strategies
let conservative_fixer = TopologyFixer::new(FixStrategy::Conservative);
let (fixed_collection, fix_result) = conservative_fixer.fix_topology(&collection)?;

println!("Features fixed: {}", fix_result.features_fixed);
println!("Features unchanged: {}", fix_result.features_unchanged);
```

### Quality Report Generation

```rust
use oxigeo_qc::prelude::*;

let mut report = QualityReport::new("Geospatial Dataset Assessment");

// Add report sections with detailed results
let section = ReportSection {
    title: "Raster Completeness".to_string(),
    description: "Checking for missing or invalid pixels".to_string(),
    results: vec![
        ("Total pixels".to_string(), "1000000".to_string()),
        ("Valid pixels".to_string(), "999500".to_string()),
        ("Completeness".to_string(), "99.95%".to_string()),
    ],
    issues: vec![],
    passed: true,
};

report.add_section(section);
report.finalize();

// Generate HTML report (with html feature enabled)
let html_report = report.to_html()?;
```

## API Overview

### Core Modules

| Module | Purpose |
|--------|---------|
| `raster` | Raster data quality control (completeness, consistency, accuracy, NoData, CRS/extent, COG compliance, radiometric range) |
| `vector` | Vector data validation (topology, attributes) |
| `metadata` | Metadata standards compliance checking |
| `stac` | STAC Item/Collection/Catalog schema validation |
| `gpkg` | OGC GeoPackage 1.4 compliance validation |
| `batch` | Batch QC mode — recursive directory scan with per-extension dispatch |
| `report` | Multi-format report generation (HTML, JSON) |
| `rules` | Configurable validation rules engine |
| `fix` | Automatic data quality issue remediation |
| `error` | Comprehensive error types and severity levels |

### Primary Types

**Raster Checks:**
- `CompletenessChecker` - Detects missing or invalid pixels
- `ConsistencyChecker` - Validates data consistency (CRS, bounds, etc.)
- `AccuracyChecker` - Checks spatial accuracy and georeferencing
- `NoDataValidator` - Detects NoData inconsistencies across bands
- `CrsAndExtentValidator` - Validates CRS parseability, axis order, and bounds plausibility
- `CogComplianceChecker` - Validates Cloud-Optimized GeoTIFF structure (OGC COG 1.0 / GDAL-cogger strict mode)
- `RadiometricValidator` - Checks per-band value ranges against sensor profiles (Landsat, Sentinel-2, MODIS, custom)

**Vector Checks:**
- `TopologyChecker` - Validates geometry validity and topological rules (self-intersection, ring orientation, gaps, overlaps)
- `AttributionChecker` - Ensures required attributes are present

**Catalog & Container Checks:**
- `StacValidator` - Validates STAC Item/Collection/Catalog JSON against the spec
- `GpkgValidator` - Validates GeoPackage 1.4 container structure

**Metadata & Reporting:**
- `MetadataChecker` - ISO 19115 and STAC compliance validation
- `QualityReport` - Generates comprehensive QC reports
- `RulesEngine` - Executes custom validation rules
- `BatchRunner` - Runs QC checks recursively over a directory tree

**Data Repair:**
- `TopologyFixer` - Fixes common geometry issues
- `FixStrategy` - Conservative, Moderate, or Aggressive repair modes

### Error Types

The crate defines specialized error types for different failure modes:

- `InvalidConfiguration` - Rule or checker configuration errors
- `InvalidInput` - Input data validation failures
- `ValidationRule` - Rule engine validation errors
- `TopologyError` - Geometry topology issues
- `AttributeError` - Feature attribute problems
- `MetadataError` - Metadata validation failures
- `RasterError` - Raster-specific errors

### Severity Levels

Issues are classified by severity:

- **Info**: Informational messages requiring no action
- **Warning**: Minor issues that don't prevent usage
- **Minor**: Issues that should be reviewed
- **Major**: Significant quality concerns
- **Critical**: Data is unusable or severely compromised

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, E>` with descriptive error types. Use the `?` operator for convenient error propagation:

```rust
use oxigeo_qc::prelude::*;

fn validate_dataset() -> Result<(), QcError> {
    let checker = CompletenessChecker::new();
    let buffer = /* ... */;

    let result = checker.check_buffer(&buffer)?;
    println!("Valid: {}/{}", result.valid_pixels, result.total_pixels);

    Ok(())
}
```

All `QcError` variants implement `std::error::Error` for seamless integration with error handling libraries like `anyhow` and `eyre`.

## Pure Rust

OxiGeo QC is **100% Pure Rust** with no C/Fortran dependencies. All functionality:

- Works out of the box without external system libraries
- Leverages the OxiGeo ecosystem for geospatial operations
- Implements standards-compliant algorithms in Rust
- Provides safe, zero-cost abstractions over geospatial data structures

## Performance

The library is optimized for:

- **Large rasters**: Efficient pixel-level analysis without full data materialization
- **Large feature sets**: Streaming geometry validation
- **Batch operations**: Process multiple checks in a single pass
- **Memory safety**: Rust's ownership system prevents data corruption

## Examples

See the [tests](tests/) directory for comprehensive usage examples:

- `qc_test.rs` - Complete integration tests demonstrating all QC checks

Example scenarios:
- Raster completeness and consistency validation
- Vector topology and attribute validation
- Metadata standards compliance
- Report generation in HTML and JSON
- Automatic geometry repair strategies
- Custom rule engine execution

## Documentation

Full API documentation is available at [docs.rs](https://docs.rs/oxigeo-qc).

Key documentation sections:
- [Module Documentation](https://docs.rs/oxigeo-qc/latest/oxigeo_qc/)
- [Error Handling](https://docs.rs/oxigeo-qc/latest/oxigeo_qc/error/)
- [Prelude Imports](https://docs.rs/oxigeo-qc/latest/oxigeo_qc/prelude/)

## Integration with OxiGeo

OxiGeo QC is tightly integrated with the OxiGeo ecosystem:

- Uses `oxigeo-core` for raster and vector data types
- Leverages `oxigeo-algorithms` for advanced spatial operations
- Compatible with `oxigeo-geojson` for vector data I/O
- Part of the cohesive geospatial processing pipeline

## Contributing

Contributions are welcome! Areas for enhancement:

- Additional metadata standards (Dublin Core, MIAOW, etc.)
- Performance optimizations for large-scale datasets
- Additional automatic fix strategies
- Custom report format templates
- Extended validation rules library

Please ensure:
- No use of `unwrap()` or `panic!()`
- Comprehensive error handling with `Result` types
- Adequate test coverage for new features
- Documentation with examples

## License

This project is licensed under **Apache-2.0**.

See [LICENSE](LICENSE) for details.

## Related COOLJAPAN Ecosystem Projects

- [OxiGeo Core](https://github.com/cool-japan/oxigeo/tree/main/crates/oxigeo-core) - Core geospatial data structures
- [OxiGeo Algorithms](https://github.com/cool-japan/oxigeo/tree/main/crates/oxigeo-algorithms) - Spatial algorithms
- [OxiGeo GeoJSON](https://github.com/cool-japan/oxigeo/tree/main/crates/oxigeo-geojson) - GeoJSON I/O
- [OxiBLAS](https://github.com/cool-japan/oxiblas) - Pure Rust BLAS operations
- [Oxicode](https://github.com/cool-japan/oxicode) - Pure Rust serialization (bincode replacement)
- [OxiFFT](https://github.com/cool-japan/oxifft) - Pure Rust FFT implementation
- [SciRS2](https://github.com/cool-japan/scirs) - Scientific computing ecosystem

---

**OxiGeo QC** is part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem - Pure Rust, high-performance geospatial and scientific computing libraries.
