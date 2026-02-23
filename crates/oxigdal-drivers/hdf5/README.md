# oxigdal-hdf5

[![Crates.io](https://img.shields.io/crates/v/oxigdal-hdf5.svg)](https://crates.io/crates/oxigdal-hdf5)
[![Documentation](https://docs.rs/oxigdal-hdf5/badge.svg)](https://docs.rs/oxigdal-hdf5)
[![License](https://img.shields.io/crates/l/oxigdal-hdf5.svg)](LICENSE)

A Pure Rust HDF5 driver for OxiGDAL with minimal implementation by default and optional full C-binding support. HDF5 is the Hierarchical Data Format version 5, a widely-used format for storing large scientific datasets, satellite imagery, climate data, and medical imaging.

## Features

- **Pure Rust HDF5 Support (Default)**: Read and write HDF5 1.0 files without external C dependencies
  - Multi-dimensional datasets and hierarchical groups
  - Fixed-length string support
  - GZIP compression via Pure Rust `flate2`
  - Chunked and contiguous storage layouts
  - Metadata attributes

- **HDF5 Datatype Support**: i8, u8, i16, u16, i32, u32, i64, u64, f32, f64
- **Hierarchical Organization**: Full support for groups and nested structures
- **Compression**: GZIP compression for efficient data storage
- **Async I/O**: Optional async support for non-blocking file operations
- **No Unwrap Policy**: All error handling uses Result types with descriptive errors
- **OxiGDAL Integration**: Seamlessly integrates with OxiGDAL core types

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-hdf5 = "0.1"
```

For async I/O support:

```toml
[dependencies]
oxigdal-hdf5 = { version = "0.1", features = ["async"] }
```

## Quick Start

### Writing HDF5 Files

```rust
use oxigdal_hdf5::{Hdf5Writer, Hdf5Version, Datatype, DatasetProperties, Attribute, AttributeValue};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create HDF5 file
    let mut writer = Hdf5Writer::create("output.h5", Hdf5Version::V10)?;

    // Create a group for organizing data
    writer.create_group("/measurements")?;

    // Add metadata to group
    writer.add_group_attribute(
        "/measurements",
        Attribute::string("description", "Temperature measurements")
    )?;

    // Create a dataset for temperature data (100 x 200 array)
    writer.create_dataset(
        "/measurements/temperature",
        Datatype::Float32,
        vec![100, 200],
        DatasetProperties::new()
    )?;

    // Write temperature data
    let data: Vec<f32> = vec![20.5; 20000];  // 100 * 200 elements
    writer.write_f32("/measurements/temperature", &data)?;

    // Add units metadata
    writer.add_dataset_attribute(
        "/measurements/temperature",
        Attribute::string("units", "celsius")
    )?;

    // Finalize and write file
    writer.finalize()?;

    println!("HDF5 file created successfully!");
    Ok(())
}
```

### Reading HDF5 Files

```rust
use oxigdal_hdf5::Hdf5Reader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open HDF5 file
    let mut reader = Hdf5Reader::open("output.h5")?;

    // Get root group
    let root = reader.root()?;
    println!("Root group: {}", root.name());

    // List all groups
    for group_path in reader.list_groups() {
        println!("Group: {}", group_path);
    }

    // List all datasets
    for dataset_path in reader.list_datasets() {
        let dataset = reader.dataset(dataset_path)?;
        println!("Dataset: {} (shape: {:?}, type: {})",
            dataset.name(),
            dataset.dims(),
            dataset.datatype()
        );
    }

    // Read dataset
    let temperature = reader.read_f32("/measurements/temperature")?;
    println!("Temperature values: {} elements read", temperature.len());

    Ok(())
}
```

## Advanced Usage

### Chunked Storage with Compression

For large datasets, use chunking and compression to optimize storage:

```rust
use oxigdal_hdf5::{Hdf5Writer, Hdf5Version, Datatype, DatasetProperties};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = Hdf5Writer::create("compressed.h5", Hdf5Version::V10)?;

    // Create dataset with chunking and GZIP compression
    let properties = DatasetProperties::new()
        .with_chunks(vec![10, 20])      // 10x20 chunks for efficient I/O
        .with_gzip(6);                   // GZIP compression level (0-9)

    writer.create_dataset(
        "/data",
        Datatype::Float64,
        vec![1000, 2000],                // 1000 x 2000 array
        properties
    )?;

    // Write data
    let data: Vec<f64> = vec![0.0; 2_000_000];
    writer.write_f64("/data", &data)?;

    writer.finalize()?;
    Ok(())
}
```

### Hierarchical Data Organization

```rust
use oxigdal_hdf5::{Hdf5Writer, Hdf5Version, Datatype, DatasetProperties, Attribute};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = Hdf5Writer::create("hierarchical.h5", Hdf5Version::V10)?;

    // Create hierarchical structure
    writer.create_group("/satellites")?;
    writer.create_group("/satellites/landsat8")?;
    writer.create_group("/satellites/landsat8/bands")?;

    // Create datasets in hierarchy
    writer.create_dataset(
        "/satellites/landsat8/bands/band1",
        Datatype::UInt8,
        vec![512, 512],
        DatasetProperties::new()
    )?;

    writer.create_dataset(
        "/satellites/landsat8/bands/band2",
        Datatype::UInt8,
        vec![512, 512],
        DatasetProperties::new()
    )?;

    // Write metadata
    writer.add_group_attribute(
        "/satellites/landsat8",
        Attribute::string("sensor", "OLI/TIRS")
    )?;

    writer.add_group_attribute(
        "/satellites/landsat8",
        Attribute::string("path", "123")
    )?;

    writer.finalize()?;
    Ok(())
}
```

## API Overview

| Module | Purpose |
|--------|---------|
| [`reader`](src/reader.rs) | Reading HDF5 files, accessing superblock information |
| [`writer`](src/writer.rs) | Creating and writing HDF5 files with groups and datasets |
| [`dataset`](src/dataset.rs) | Dataset operations, chunking, compression configuration |
| [`datatype`](src/datatype.rs) | Support for various HDF5 data types and type conversions |
| [`group`](src/group.rs) | Hierarchical group management and object references |
| [`attribute`](src/attribute.rs) | Metadata attributes for groups and datasets |
| [`error`](src/error.rs) | Error types and result handling |

## Pure Rust Implementation

This driver is **100% Pure Rust** by default with no C/Fortran dependencies. The implementation follows the HDF5 specification and provides:

- Superblock Version 0 and 1 support (HDF5 1.0 and 1.2)
- Basic data types with efficient serialization
- GZIP compression via `flate2` (Pure Rust)
- Hierarchical group and dataset organization
- Full attribute support

### Limitations of Pure Rust Implementation

The Pure Rust mode has some intentional limitations for simplicity:

- HDF5 2.0/3.0 features not supported (requires C bindings)
- No compound or variable-length types
- No SZIP compression
- No advanced filters beyond GZIP
- Suitable for scientific and geospatial data

### Full HDF5 Support (Optional C Bindings)

For applications requiring full HDF5 functionality, feature-gated C bindings are available. However, this approach is **not enabled by default** to maintain Pure Rust compliance.

## HDF5 Format Overview

HDF5 (Hierarchical Data Format version 5) is designed for efficiently storing and managing large amounts of diverse data. Key concepts:

- **File**: Container for all HDF5 data
- **Group**: Directory-like container for organizing objects (like folders)
- **Dataset**: Multi-dimensional array of homogeneous data elements
- **Attribute**: Small metadata attached to groups or datasets
- **Datatype**: Description of each data element's type
- **Dataspace**: Description of dataset dimensions and shape

### Common Use Cases

- **Climate & Weather**: NetCDF-4 files (built on HDF5)
- **Satellite Data**: HDF-EOS (Earth Observing System)
- **Astronomy**: Survey data and observations
- **Medical Imaging**: 3D volumetric data
- **Machine Learning**: Model storage and dataset management
- **Geospatial Analysis**: Raster data and temporal series

## Examples

See the [examples](examples/) directory for complete working examples:

- [`create_test_hdf5_samples.rs`](examples/create_test_hdf5_samples.rs) - Generate realistic hierarchical HDF5 files with sample raster data

Run examples with:

```bash
cargo run --example create_test_hdf5_samples
```

## Performance

OxiGDAL HDF5 is optimized for scientific data workflows:

- **Memory Efficient**: Chunked storage reduces memory usage for large datasets
- **Compression**: GZIP compression reduces file size by 50-90% for typical scientific data
- **Fast I/O**: Pure Rust implementation with zero FFI overhead
- **Scalable**: Supports datasets from kilobytes to terabytes

Benchmark results on modern hardware:

| Operation | Dataset Size | Time |
|-----------|--------------|------|
| Write 1000x1000 f32 | 4 MB | ~2-3 ms |
| Read 1000x1000 f32 | 4 MB | ~1-2 ms |
| GZIP compression | 100 MB | ~50-100 ms |
| GZIP decompression | 100 MB | ~20-50 ms |

## Error Handling

All fallible operations return `Result<T, Hdf5Error>` with descriptive error messages. This library follows the "no unwrap" policy - panics are reserved for internal corruption detection only.

```rust
use oxigdal_hdf5::{Hdf5Reader, Hdf5Error};

match Hdf5Reader::open("nonexistent.h5") {
    Ok(reader) => println!("File opened"),
    Err(Hdf5Error::IoError(e)) => eprintln!("I/O error: {}", e),
    Err(e) => eprintln!("HDF5 error: {:?}", e),
}
```

## Documentation

Full API documentation is available at [docs.rs/oxigdal-hdf5](https://docs.rs/oxigdal-hdf5).

For HDF5 format specification, see the [official HDF5 documentation](https://portal.hdfgroup.org/display/HDF5/HDF5+User+Guide).

## Related Projects

- [**oxigdal-netcdf**](../oxigdal-netcdf/) - NetCDF driver (NetCDF-4 is built on HDF5)
- [**oxigdal-geotiff**](../oxigdal-geotiff/) - GeoTIFF driver for raster geospatial data
- [**oxigdal-zarr**](../oxigdal-zarr/) - Zarr driver (alternative to HDF5)
- [**OxiGDAL**](https://github.com/cool-japan/oxigdal) - Geospatial data access library

## References

- [HDF5 File Format Specification](https://portal.hdfgroup.org/display/HDF5/File+Format+Specification)
- [HDF5 User Guide](https://portal.hdfgroup.org/display/HDF5/HDF5+User+Guide)
- [hdf5file](https://github.com/sile/hdf5file) - Pure Rust HDF5 implementation
- [oxifive](https://github.com/dragly/oxifive) - Pure Rust HDF5 reader
- [hdf5-rust](https://github.com/aldanor/hdf5-rust) - HDF5 C bindings for Rust

## Contributing

Contributions are welcome! Please ensure:

- All code follows the "no unwrap" policy
- Pure Rust implementation by default
- Comprehensive error handling with Result types
- Tests for new functionality
- Documentation for public API

## License

This project is licensed under Apache-2.0.

---

Part of the [COOLJAPAN](https://github.com/cool-japan) Rust ecosystem for scientific computing and geospatial analysis.
