# oxigdal-metadata

Comprehensive metadata standards support for OxiGDAL.

## Overview

This crate provides Pure Rust implementations of multiple geospatial and data catalog metadata standards:

- **ISO 19115** - Geographic Information - Metadata (ISO 19115:2014)
- **FGDC** - Federal Geographic Data Committee (FGDC-STD-001-1998)
- **INSPIRE** - EU INSPIRE Directive metadata requirements
- **DataCite** - DOI metadata for research data (Schema 4.4)
- **DCAT** - W3C Data Catalog Vocabulary (v3)

## Features

- **Metadata Extraction** - Extract metadata from GeoTIFF, NetCDF, HDF5, and STAC
- **Validation** - Comprehensive validation with quality scoring
- **Transformation** - Convert between different metadata standards
- **Serialization** - XML and JSON support
- **Pure Rust** - No external dependencies on C/Fortran libraries

## Usage

### ISO 19115 Metadata

```rust
use oxigdal_metadata::iso19115::*;
use oxigdal_metadata::common::BoundingBox;

let metadata = Iso19115Metadata::builder()
    .title("Sentinel-2 Level-2A")
    .abstract_text("Atmospherically corrected satellite imagery")
    .keywords(vec!["satellite", "sentinel-2", "optical"])
    .bbox(BoundingBox::new(-10.0, 5.0, 35.0, 45.0))
    .build()?;

// Export to XML
let xml = metadata.to_xml()?;

// Validate
let validation = validate::validate_iso19115(&metadata)?;
println!("Quality score: {}", validation.quality_score);
```

### FGDC Metadata

```rust
use oxigdal_metadata::fgdc::*;

let metadata = FgdcMetadata::builder()
    .title("Digital Elevation Model")
    .abstract_text("30-meter resolution DEM")
    .purpose("Terrain analysis and visualization")
    .keywords("Elevation", vec!["DEM", "elevation", "terrain"])
    .build()?;
```

### DataCite Metadata

```rust
use oxigdal_metadata::datacite::*;

let metadata = DataCiteMetadata::builder()
    .identifier("10.5281/zenodo.123456", IdentifierType::Doi)
    .creator(Creator::new("Smith, John"))
    .title("Research Dataset")
    .publisher("Zenodo")
    .publication_year(2024)
    .resource_type(ResourceTypeGeneral::Dataset)
    .build()?;
```

### Metadata Transformation

```rust
use oxigdal_metadata::transform;

// Transform ISO 19115 to FGDC
let fgdc = transform::iso19115_to_fgdc(&iso_metadata)?;

// Transform DataCite to ISO 19115
let iso = transform::datacite_to_iso19115(&datacite_metadata)?;
```

### Metadata Extraction

```rust
use oxigdal_metadata::extract;

// Extract from file
let metadata = extract::extract_metadata("data.tif")?;

// Convert to ISO 19115
let iso = extract::to_iso19115(&metadata)?;

// Custom extractor
let extractor = extract::MetadataExtractor::new()
    .with_spatial(true)
    .with_temporal(true)
    .with_max_keywords(10);

let metadata = extractor.extract("data.nc")?;
```

## Standards Compliance

### ISO 19115:2014

Implements core metadata elements including:
- Citation and responsible party information
- Data identification and classification
- Spatial and temporal extent
- Distribution and quality information
- Reference system information

### FGDC CSDGM

Implements FGDC-STD-001-1998 including:
- Identification information
- Data quality information
- Spatial data organization
- Spatial reference information
- Entity and attribute information
- Distribution information

### INSPIRE

Implements EU INSPIRE Directive requirements:
- Resource locators
- Unique resource identifiers
- Conformity declarations
- Service metadata
- All 34 INSPIRE themes (Annex I, II, III)

### DataCite

Implements DataCite Metadata Schema 4.4:
- DOI registration metadata
- Creator and contributor information
- Resource type and classification
- Related identifiers
- Geo-location support

### DCAT

Implements W3C DCAT v3:
- Dataset and catalog descriptions
- Distribution information
- Data services
- RDF/JSON-LD serialization

## Validation

All metadata standards include comprehensive validation:

```rust
use oxigdal_metadata::validate;

let report = validate::validate_iso19115(&metadata)?;

if !report.is_valid {
    println!("Errors: {:?}", report.errors);
}

println!("Missing fields: {:?}", report.missing_fields());
println!("Quality score: {}/100", report.quality_score);
println!("Completeness: {}%", report.completeness);
```

## License

Apache-2.0

## Copyright

Copyright (c) 2024 COOLJAPAN OU (Team Kitasan)
