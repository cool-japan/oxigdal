# oxigeo-jupyter

[![Crates.io](https://img.shields.io/crates/v/oxigeo-jupyter.svg)](https://crates.io/crates/oxigeo-jupyter)
[![Documentation](https://docs.rs/oxigeo-jupyter/badge.svg)](https://docs.rs/oxigeo-jupyter)
[![License](https://img.shields.io/crates/l/oxigeo-jupyter.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.89+-orange.svg)](https://www.rust-lang.org)

Jupyter kernel integration for OxiGeo with rich display capabilities, interactive widgets, and magic commands for geospatial data analysis in Jupyter notebooks.

## Features

- **Custom Jupyter Kernel**: Full-featured Jupyter kernel implementation for OxiGeo operations
- **Rich Display**: Multi-format display support (HTML, images, maps, tables, JSON)
- **Interactive Widgets**: Map widgets, data visualization, and interactive controls
- **Magic Commands**: Convenience commands for common geospatial operations
  - `%load_raster`: Load raster files
  - `%plot`: Visualize raster data with custom colormaps
  - `%info`: Display dataset metadata
  - `%crs`: Show coordinate reference system information
  - `%bounds`: Display spatial extents
  - `%stats`: Calculate raster statistics
- **Visualization**: Integration with plotters for charts and maps
- **100% Pure Rust**: No C/Fortran dependencies, everything in Pure Rust

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigeo-jupyter = "0.2"
oxigeo-core = "0.2"
```

Optional features:

```toml
[dependencies]
oxigeo-jupyter = { version = "0.2", features = ["async"] }
```

### Feature Flags

- `async`: Enable async/await support with Tokio (for async kernel operations)

## Quick Start

### Creating and Running a Kernel

```rust
use oxigeo_jupyter::OxiGeoKernel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new kernel
    let mut kernel = OxiGeoKernel::new()?;

    // Execute a magic command to load a raster
    let result = kernel.execute("%load_raster data/elevation.tif dem")?;
    println!("{:?}", result);

    // Display dataset information
    let info = kernel.execute("%info dem")?;
    println!("{:?}", info);

    // Plot the raster
    let plot = kernel.execute("%plot dem --colormap viridis")?;
    println!("{:?}", plot);

    Ok(())
}
```

### Rich Display in Notebooks

```rust
use oxigeo_jupyter::display::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create table display
    let mut table = Table::new(vec![
        "Latitude".to_string(),
        "Longitude".to_string(),
        "Elevation".to_string(),
    ]).with_title("Geographic Points");

    table.add_row(vec![
        "45.5".to_string(),
        "-122.7".to_string(),
        "50".to_string(),
    ])?;

    let display = table.display_data()?;
    println!("{:?}", display);

    // Create map display
    let map = MapDisplay::new((0.0, 0.0), 12)
        .with_dimensions(800, 600);

    let map_display = map.display_data()?;
    println!("{:?}", map_display);

    Ok(())
}
```

### Interactive Widgets

```rust
use oxigeo_jupyter::widgets::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an interactive map widget
    let mut widget = MapWidget::new("map1", (51.5074, -0.1278), 13)
        .with_dimensions(1000, 700)
        .with_basemap(BasemapProvider::OpenStreetMap);

    // Add layers
    widget.add_layer("https://example.com/geojson.json".to_string());

    // Render to HTML
    let html = widget.render()?;
    println!("{}", html);

    Ok(())
}
```

## Usage

### Module Overview

#### `kernel` - Jupyter Kernel Implementation

The core kernel module provides:

- `OxiGeoKernel`: Main kernel struct with execution capabilities
- `KernelConfig`: Configuration for the kernel
- `ExecutionResult`: Result of code execution with status and output
- `CompletionResult`: Code completion suggestions
- `InspectionResult`: Help and introspection information

```rust
use oxigeo_jupyter::kernel::OxiGeoKernel;

let mut kernel = OxiGeoKernel::new()?;

// Execute code
let result = kernel.execute("let x = 42")?;

// Get completions
let completions = kernel.complete("var", 3)?;

// Inspect variables
let info = kernel.inspect("x", 0)?;
```

#### `display` - Rich Display Support

Provides rich output formatting for Jupyter:

- `DisplayData`: Structured display data with multiple MIME types
- `RichDisplay`: Trait for custom display implementations
- `Table`: Formatted table display with HTML and plain text
- `MapDisplay`: Interactive map with Leaflet
- `ImageDisplay`: Image rendering (PNG, JPEG)

```rust
use oxigeo_jupyter::display::*;

// Create multi-format display
let display = DisplayData::new()
    .with_text("Hello, World!")
    .with_html("<h1>Hello, World!</h1>")
    .with_metadata("key", serde_json::json!("value"));

// Create and render table
let mut table = Table::new(vec!["Name".to_string(), "Value".to_string()]);
table.add_row(vec!["alpha".to_string(), "0.5".to_string()])?;
let display_data = table.display_data()?;
```

#### `magic` - Magic Commands

Convenient commands for common operations:

- `%load_raster <path> [name]`: Load raster files
- `%plot <dataset> [options]`: Plot with custom options
- `%info <dataset>`: Show metadata
- `%crs <dataset>`: Show CRS information
- `%bounds <dataset>`: Show spatial bounds
- `%stats <dataset> [band]`: Calculate statistics
- `%list`: List loaded datasets
- `%clear`: Clear namespace

```rust
use oxigeo_jupyter::magic::MagicCommand;

let cmd = MagicCommand::parse("%load_raster data.tif")?;
```

#### `widgets` - Interactive Components

Interactive Jupyter widgets:

- `Widget`: Base trait for all widgets
- `MapWidget`: Interactive map with basemap options
- `BasemapProvider`: Enum for different map sources

```rust
use oxigeo_jupyter::widgets::*;

let mut widget = MapWidget::new("map", (0.0, 0.0), 10);
widget.add_layer("layer_url".to_string());
```

#### `plotting` - Visualization

Integration with plotters for generating charts and visualizations.

### Error Handling

The crate follows the "no unwrap" policy. All fallible operations return `Result<T, JupyterError>`:

```rust
use oxigeo_jupyter::{OxiGeoKernel, Result as JupyterResult};

fn process() -> JupyterResult<()> {
    let mut kernel = OxiGeoKernel::new()?;
    let result = kernel.execute("let x = 42")?;
    Ok(())
}
```

Error types:

- `JupyterError::Io`: I/O operations
- `JupyterError::Kernel`: Kernel execution errors
- `JupyterError::Display`: Display formatting errors
- `JupyterError::Widget`: Widget operation errors
- `JupyterError::Magic`: Magic command errors
- `JupyterError::Plotting`: Visualization errors
- `JupyterError::OxiGeo`: Errors from OxiGeo core

## Architecture

### Kernel Architecture

```
OxiGeoKernel
├── KernelConfig (name, version, language)
├── Execution Engine
│   ├── Magic Command Parser
│   ├── Code Parser
│   └── Execution Context
├── Namespace (variable storage)
└── History (command history)
```

### Display Pipeline

```
Data
└── RichDisplay trait
    ├── DisplayData (MIME types: text, HTML, JSON)
    ├── Table (comfy_table + HTML)
    ├── MapDisplay (Leaflet)
    └── ImageDisplay (base64 encoded)
```

### Component Dependencies

```
oxigeo-jupyter
├── oxigeo-core
├── oxigeo-algorithms
├── oxigeo-geotiff
├── oxigeo-geojson
├── plotters (visualization)
├── serde (serialization)
└── thiserror (error handling)
```

## Examples

The `examples/` directory includes:

- **`01_basic_usage.rs`**: Basic kernel operations and magic commands
- **`02_visualization.rs`**: Creating charts and visualizations
- **`03_widgets.rs`**: Interactive widget examples

Run examples with:

```bash
cargo run --example 01_basic_usage
cargo run --example 02_visualization
cargo run --example 03_widgets
```

## Integration with OxiGeo Ecosystem

### Related Projects

- **[oxigeo-core](https://github.com/cool-japan/oxigeo)**: Core GIS functionality
- **[oxigeo-algorithms](https://github.com/cool-japan/oxigeo)**: Geospatial algorithms
- **[oxigeo-geotiff](https://github.com/cool-japan/oxigeo)**: GeoTIFF support
- **[oxigeo-geojson](https://github.com/cool-japan/oxigeo)**: GeoJSON support

### Use Cases

- Interactive geospatial analysis in Jupyter notebooks
- Raster and vector data visualization
- Statistical analysis and reporting
- Educational demonstrations
- Rapid prototyping of GIS workflows

## Performance Considerations

- **Display Rendering**: HTML rendering is efficient but large datasets may benefit from sampling
- **Widget Rendering**: Map widgets use Leaflet for client-side rendering
- **Memory**: Kernel namespace stores variables in memory; clear unnecessary data with `%clear`
- **Async Support**: Enable the `async` feature for async operations

## Testing

Run the test suite:

```bash
cargo test --all-features
```

Run specific test module:

```bash
cargo test display::tests
cargo test kernel::tests
cargo test magic::tests
```

## Pure Rust Implementation

This library is **100% Pure Rust** with no C/Fortran dependencies. All functionality works out-of-the-box without external system libraries.

### Dependencies Overview

- **serde/serde_json**: Serialization (no external tools)
- **plotters**: Pure Rust charting
- **base64**: Encoding/decoding
- **comfy-table**: Table formatting
- **thiserror**: Error macros
- **OxiGeo ecosystem**: Pure Rust GIS operations

## Documentation

- **[docs.rs](https://docs.rs/oxigeo-jupyter)**: Full API documentation with examples
- **Module documentation**: Each module has comprehensive doc comments
- **Examples**: See `examples/` directory for practical usage

Generate local documentation:

```bash
cargo doc --open
```

## Contributing

Contributions are welcome! Please follow the COOLJAPAN policies:

- No unwrap() usage (return Results)
- Keep files under 2000 lines (refactor with splitrs if needed)
- Use latest crate versions from crates.io
- 100% Pure Rust (no C/Fortran by default)
- Include tests for new functionality

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

## Credits

This project is part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem, developed by COOLJAPAN OU (Team Kitasan).

For more information on the broader OxiGeo project, visit:
- **Project**: https://github.com/cool-japan/oxigeo
- **Organization**: https://github.com/cool-japan

---

## See Also

- [oxigeo-core](https://docs.rs/oxigeo-core) - Core geospatial functionality
- [oxigeo-algorithms](https://docs.rs/oxigeo-algorithms) - Geospatial algorithms
- [Plotters](https://docs.rs/plotters) - Rust plotting library
- [Serde](https://serde.rs) - Serialization framework
- [Jupyter Protocol](https://jupyter-client.readthedocs.io/en/latest/messaging.html) - Jupyter messaging protocol

**Happy Jupyter + Rust + Geospatial computing!**
