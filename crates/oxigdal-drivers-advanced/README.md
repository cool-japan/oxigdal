# OxiGDAL Advanced Drivers

Advanced geospatial format drivers for OxiGDAL, providing Pure Rust implementations for JPEG2000, GeoPackage, KML/KMZ, and GML formats.

## Features

- **JPEG2000 (JP2)** - Pure Rust JPEG2000 codec
  - JP2 box structure parsing
  - Codestream decoding (simplified)
  - Multi-resolution pyramid support
  - GeoJP2 metadata extraction
  - Metadata handling (XML boxes, ICC profiles)

- **GeoPackage (GPKG)** - SQLite-based vector and raster storage
  - Vector feature tables with multiple geometry types
  - Raster tile matrices
  - R-tree spatial indexing
  - GeoPackage 1.3 specification compliance
  - Extensions support

- **KML/KMZ** - Keyhole Markup Language for Google Earth
  - KML 2.2 support
  - Placemark, LineString, Polygon geometries
  - Styles and icons
  - NetworkLinks
  - KMZ (zipped KML) with embedded images

- **GML** - Geography Markup Language (OGC standard)
  - GML 3.2 support
  - Feature collections
  - Geometry encoding/decoding
  - CRS support
  - Schema validation

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-drivers-advanced = "0.1.3"
```

### JPEG2000

```rust
use oxigdal_drivers_advanced::jp2;
use std::fs::File;

let file = File::open("image.jp2")?;
let image = jp2::read_jp2(file)?;
println!("Dimensions: {}x{}", image.width, image.height);
```

### GeoPackage

```rust
use oxigdal_drivers_advanced::gpkg::*;

// Create GeoPackage
let mut gpkg = GeoPackage::create("data.gpkg")?;

// Create feature table
let table = gpkg.create_feature_table("points", GeometryType::Point, 4326)?;

// Open existing GeoPackage
let gpkg = GeoPackage::open("data.gpkg")?;
let tables = gpkg.feature_tables()?;
```

### KML/KMZ

```rust
use oxigdal_drivers_advanced::kml::*;

// Create KML document
let mut doc = KmlDocument::new()
    .with_name("My Places");

let placemark = Placemark::new()
    .with_name("Test Point")
    .with_geometry(KmlGeometry::Point(Coordinates::new(-122.08, 37.42)));

doc.add_placemark(placemark);

// Write KML
let mut file = File::create("output.kml")?;
write_kml(&mut file, &doc)?;

// Read KMZ
let kmz = read_kmz_file("places.kmz")?;
```

### GML

```rust
use oxigdal_drivers_advanced::gml::*;

// Create feature collection
let mut collection = GmlFeatureCollection::new()
    .with_crs("EPSG:4326");

let mut feature = GmlFeature::new()
    .with_id("f1")
    .with_geometry(GmlGeometry::Point {
        coordinates: vec![10.0, 20.0],
    });

feature.add_property("name", "Test Feature");
collection.add_feature(feature);

// Write GML
write_gml(&mut output, &collection)?;
```

## Features Flags

- `jpeg2000` - JPEG2000 format support (default)
- `geopackage` - GeoPackage format support (default)
- `kml` - KML/KMZ format support (default)
- `gml` - GML format support (default)
- `async` - Async I/O support (optional)

## COOLJAPAN Compliance

This crate follows COOLJAPAN ecosystem policies:

- ✅ Pure Rust implementation (no C/Fortran dependencies)
- ✅ No `unwrap()` calls in production code
- ✅ All files under 2000 lines
- ✅ Workspace dependency management
- ✅ Comprehensive tests and benchmarks

## Performance

Benchmarks are included for all format operations:

```bash
cargo bench --package oxigdal-drivers-advanced
```

## License

Apache-2.0

## Author

COOLJAPAN OU (Team Kitasan)
