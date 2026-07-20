//! Basic OxiGeo Project Template

use anyhow::Result;

fn main() -> Result<()> {
    println!("Basic OxiGeo Project");

    // Example: Read a GeoTIFF file
    // let dataset = oxigeo_geotiff::read("path/to/file.tif")?;

    // Example: Process raster data
    // let processed = oxigeo_algorithms::ndvi(&nir_band, &red_band)?;

    // Example: Write output
    // oxigeo_geotiff::write("output.tif", &processed)?;

    println!("Processing complete!");

    Ok(())
}
