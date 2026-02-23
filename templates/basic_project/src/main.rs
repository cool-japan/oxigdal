//! Basic OxiGDAL Project Template

use anyhow::Result;

fn main() -> Result<()> {
    println!("Basic OxiGDAL Project");

    // Example: Read a GeoTIFF file
    // let dataset = oxigdal_geotiff::read("path/to/file.tif")?;

    // Example: Process raster data
    // let processed = oxigdal_algorithms::ndvi(&nir_band, &red_band)?;

    // Example: Write output
    // oxigdal_geotiff::write("output.tif", &processed)?;

    println!("Processing complete!");

    Ok(())
}
