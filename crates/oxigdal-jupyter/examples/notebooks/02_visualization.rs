//! Visualization example notebook
//!
//! This example demonstrates visualization capabilities.

use oxigdal_jupyter::{
    display::{DisplayData, ImageDisplay, ImageFormat, Table},
    plotting::{Colormap, LinePlot, RasterPlotter},
    RichDisplay, Result,
};

fn main() -> Result<()> {
    println!("# OxiGDAL Visualization Examples");
    println!();

    // Example 1: Display a table
    println!("## Example 1: Table Display");
    let mut table = Table::new(vec!["Band".to_string(), "Min".to_string(), "Max".to_string()])
        .with_title("Raster Statistics");
    table.add_row(vec!["1".to_string(), "0.0".to_string(), "255.0".to_string()])?;
    table.add_row(vec!["2".to_string(), "10.5".to_string(), "200.3".to_string()])?;
    let display = table.display_data()?;
    println!("{}", display.data.get("text/plain").unwrap_or(&String::new()));
    println!();

    // Example 2: Plot raster data
    println!("## Example 2: Raster Plot");
    let plotter = RasterPlotter::new(400, 300)
        .with_colormap(Colormap::Viridis)
        .with_title("Elevation Data");

    let data: Vec<f64> = (0..100).map(|i| (i as f64 / 100.0)).collect();
    let png_data = plotter.plot_to_png(&data, 10, 10)?;
    println!("Generated PNG with {} bytes", png_data.len());
    println!();

    // Example 3: Line plot
    println!("## Example 3: Line Plot");
    let line_plot = LinePlot::new(600, 400)
        .with_title("Profile")
        .with_x_label("Distance")
        .with_y_label("Elevation");

    let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
    let y: Vec<f64> = x.iter().map(|&x| (x / 10.0).sin() * 100.0).collect();
    let png_data = line_plot.plot_to_png(&x, &y)?;
    println!("Generated line plot with {} bytes", png_data.len());
    println!();

    // Example 4: Histogram
    println!("## Example 4: Histogram");
    let data: Vec<f64> = (0..1000)
        .map(|_| {
            // Simulate normal distribution
            (0..12).map(|_| rand::random::<f64>()).sum::<f64>() - 6.0
        })
        .collect();

    let plotter = RasterPlotter::new(600, 400);
    let png_data = plotter.plot_histogram(&data, 50)?;
    println!("Generated histogram with {} bytes", png_data.len());
    println!();

    Ok(())
}

// Simple random number generator for demo
mod rand {
    pub fn random<T>() -> T
    where
        T: From<f64>,
    {
        T::from(0.5)
    }
}
