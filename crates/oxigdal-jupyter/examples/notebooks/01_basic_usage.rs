//! Basic usage example notebook
//!
//! This example demonstrates basic OxiGDAL operations in a Jupyter-like environment.

use oxigdal_jupyter::{OxiGdalKernel, Result};

fn main() -> Result<()> {
    println!("# Basic OxiGDAL Usage Example");
    println!();

    let mut kernel = OxiGdalKernel::new()?;

    // Cell 1: Load a raster
    println!("## Cell 1: Load raster");
    let result = kernel.execute("%load_raster /path/to/raster.tif elevation")?;
    println!("{:?}", result);
    println!();

    // Cell 2: Show info
    println!("## Cell 2: Show information");
    let result = kernel.execute("%info elevation")?;
    println!("{:?}", result);
    println!();

    // Cell 3: Show CRS
    println!("## Cell 3: Show CRS");
    let result = kernel.execute("%crs elevation")?;
    println!("{:?}", result);
    println!();

    // Cell 4: Show bounds
    println!("## Cell 4: Show bounds");
    let result = kernel.execute("%bounds elevation")?;
    println!("{:?}", result);
    println!();

    // Cell 5: Calculate statistics
    println!("## Cell 5: Calculate statistics");
    let result = kernel.execute("%stats elevation 1")?;
    println!("{:?}", result);
    println!();

    // Cell 6: Plot raster
    println!("## Cell 6: Plot raster");
    let result = kernel.execute("%plot elevation --colormap viridis")?;
    println!("{:?}", result);
    println!();

    // Cell 7: List all datasets
    println!("## Cell 7: List datasets");
    let result = kernel.execute("%list")?;
    println!("{:?}", result);
    println!();

    Ok(())
}
