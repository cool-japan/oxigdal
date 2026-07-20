//! Plotting utilities for Jupyter
//!
//! This module provides plotting capabilities for raster and vector data
//! using the plotters library.

use crate::{JupyterError, Result};
use plotters::prelude::*;
use std::str::FromStr;

/// Colormap types
#[derive(Debug, Clone, Copy)]
pub enum Colormap {
    /// Grayscale
    Grayscale,
    /// Viridis
    Viridis,
    /// Plasma
    Plasma,
    /// Inferno
    Inferno,
    /// Magma
    Magma,
    /// Jet
    Jet,
    /// Rainbow
    Rainbow,
}

impl FromStr for Colormap {
    type Err = JupyterError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "grayscale" | "gray" | "grey" => Ok(Self::Grayscale),
            "viridis" => Ok(Self::Viridis),
            "plasma" => Ok(Self::Plasma),
            "inferno" => Ok(Self::Inferno),
            "magma" => Ok(Self::Magma),
            "jet" => Ok(Self::Jet),
            "rainbow" => Ok(Self::Rainbow),
            _ => Err(JupyterError::Plotting(format!("Unknown colormap: {}", s))),
        }
    }
}

impl Colormap {
    /// Map value [0.0, 1.0] to RGB color
    pub fn map(&self, value: f64) -> RGBColor {
        let value = value.clamp(0.0, 1.0);

        match self {
            Self::Grayscale => {
                let gray = (value * 255.0) as u8;
                RGBColor(gray, gray, gray)
            }
            Self::Viridis => self.viridis(value),
            Self::Plasma => self.plasma(value),
            Self::Inferno => self.inferno(value),
            Self::Magma => self.magma(value),
            Self::Jet => self.jet(value),
            Self::Rainbow => self.rainbow(value),
        }
    }

    /// Viridis colormap
    fn viridis(&self, value: f64) -> RGBColor {
        // Simplified viridis approximation
        let r = 255.0 * (0.282 * value + 0.718 * value * value);
        let g = 255.0 * (value * value * (1.5 - value));
        let b = 255.0 * (0.5 + 0.5 * value);
        RGBColor(r as u8, g as u8, b as u8)
    }

    /// Plasma colormap
    fn plasma(&self, value: f64) -> RGBColor {
        let r = 255.0 * (0.5 + 0.5 * value);
        let g = 255.0 * (value * value);
        let b = 255.0 * (1.0 - value);
        RGBColor(r as u8, g as u8, b as u8)
    }

    /// Inferno colormap
    fn inferno(&self, value: f64) -> RGBColor {
        let r = 255.0 * value;
        let g = 255.0 * (value * value * 0.8);
        let b = 255.0 * (value * value * value * 0.5);
        RGBColor(r as u8, g as u8, b as u8)
    }

    /// Magma colormap
    fn magma(&self, value: f64) -> RGBColor {
        let r = 255.0 * value;
        let g = 255.0 * (value * value * 0.6);
        let b = 255.0 * (value * 0.8);
        RGBColor(r as u8, g as u8, b as u8)
    }

    /// Jet colormap
    fn jet(&self, value: f64) -> RGBColor {
        let r = if value < 0.5 {
            0.0
        } else {
            255.0 * (2.0 * value - 1.0)
        };
        let g = if value < 0.5 {
            255.0 * (2.0 * value)
        } else {
            255.0 * (2.0 * (1.0 - value))
        };
        let b = if value < 0.5 {
            255.0 * (1.0 - 2.0 * value)
        } else {
            0.0
        };
        RGBColor(r as u8, g as u8, b as u8)
    }

    /// Rainbow colormap
    fn rainbow(&self, value: f64) -> RGBColor {
        let hue = value * 300.0; // 0-300 degrees (avoiding wrap to red)
        hsv_to_rgb(hue, 1.0, 1.0)
    }
}

/// Convert HSV to RGB
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> RGBColor {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    RGBColor(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Raster plotter
pub struct RasterPlotter {
    /// Width in pixels
    width: u32,
    /// Height in pixels
    height: u32,
    /// Colormap
    colormap: Colormap,
    /// Title
    title: Option<String>,
    /// Show colorbar
    show_colorbar: bool,
}

impl RasterPlotter {
    /// Create new raster plotter
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            colormap: Colormap::Viridis,
            title: None,
            show_colorbar: true,
        }
    }

    /// Set colormap
    pub fn with_colormap(mut self, colormap: Colormap) -> Self {
        self.colormap = colormap;
        self
    }

    /// Set title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set whether to show colorbar
    pub fn with_colorbar(mut self, show: bool) -> Self {
        self.show_colorbar = show;
        self
    }

    /// Plot raster data to PNG
    pub fn plot_to_png(&self, data: &[f64], rows: usize, cols: usize) -> Result<Vec<u8>> {
        if data.len() != rows * cols {
            return Err(JupyterError::Plotting(format!(
                "Data length {} doesn't match dimensions {}x{}",
                data.len(),
                rows,
                cols
            )));
        }

        let mut buffer = Vec::new();
        {
            let root = BitMapBackend::with_buffer(&mut buffer, (self.width, self.height))
                .into_drawing_area();

            root.fill(&WHITE)
                .map_err(|e| JupyterError::Plotting(format!("Failed to fill background: {}", e)))?;

            let (min, max) = data
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &v| {
                    (min.min(v), max.max(v))
                });

            let range = if (max - min).abs() < 1e-10 {
                1.0
            } else {
                max - min
            };

            // Draw raster
            let cell_width = self.width as f64 / cols as f64;
            let cell_height = self.height as f64 / rows as f64;

            for row in 0..rows {
                for col in 0..cols {
                    let value = data[row * cols + col];
                    let normalized = (value - min) / range;
                    let color = self.colormap.map(normalized);

                    let x = (col as f64 * cell_width) as i32;
                    let y = (row as f64 * cell_height) as i32;
                    let w = cell_width.ceil() as u32;
                    let h = cell_height.ceil() as u32;

                    root.draw(&Rectangle::new(
                        [(x, y), (x + w as i32, y + h as i32)],
                        color.filled(),
                    ))
                    .map_err(|e| JupyterError::Plotting(format!("Failed to draw cell: {}", e)))?;
                }
            }

            root.present()
                .map_err(|e| JupyterError::Plotting(format!("Failed to present: {}", e)))?;
        }

        Ok(buffer)
    }

    /// Plot histogram
    pub fn plot_histogram(&self, data: &[f64], bins: usize) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        {
            let root = BitMapBackend::with_buffer(&mut buffer, (self.width, self.height))
                .into_drawing_area();

            root.fill(&WHITE)
                .map_err(|e| JupyterError::Plotting(format!("Failed to fill background: {}", e)))?;

            // Calculate histogram
            let (min, max) = data
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &v| {
                    (min.min(v), max.max(v))
                });

            let bin_width = (max - min) / bins as f64;
            let mut hist = vec![0usize; bins];

            for &value in data {
                let bin = ((value - min) / bin_width).floor() as usize;
                let bin = bin.min(bins - 1);
                hist[bin] += 1;
            }

            let max_count = *hist.iter().max().unwrap_or(&1);

            let mut chart = ChartBuilder::on(&root)
                .caption(
                    self.title.as_deref().unwrap_or("Histogram"),
                    ("sans-serif", 30),
                )
                .margin(10)
                .x_label_area_size(30)
                .y_label_area_size(40)
                .build_cartesian_2d(min..max, 0..max_count)
                .map_err(|e| JupyterError::Plotting(format!("Failed to build chart: {}", e)))?;

            chart
                .configure_mesh()
                .draw()
                .map_err(|e| JupyterError::Plotting(format!("Failed to draw mesh: {}", e)))?;

            chart
                .draw_series(hist.iter().enumerate().map(|(i, &count)| {
                    let x0 = min + i as f64 * bin_width;
                    let x1 = x0 + bin_width;
                    Rectangle::new([(x0, 0), (x1, count)], BLUE.filled())
                }))
                .map_err(|e| JupyterError::Plotting(format!("Failed to draw series: {}", e)))?;

            root.present()
                .map_err(|e| JupyterError::Plotting(format!("Failed to present: {}", e)))?;
        }

        Ok(buffer)
    }
}

/// Line plot
pub struct LinePlot {
    /// Width
    width: u32,
    /// Height
    height: u32,
    /// Title
    title: Option<String>,
    /// X label
    x_label: Option<String>,
    /// Y label
    y_label: Option<String>,
}

impl LinePlot {
    /// Create new line plot
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            title: None,
            x_label: None,
            y_label: None,
        }
    }

    /// Set title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set X label
    pub fn with_x_label(mut self, label: impl Into<String>) -> Self {
        self.x_label = Some(label.into());
        self
    }

    /// Set Y label
    pub fn with_y_label(mut self, label: impl Into<String>) -> Self {
        self.y_label = Some(label.into());
        self
    }

    /// Plot line to PNG
    pub fn plot_to_png(&self, x: &[f64], y: &[f64]) -> Result<Vec<u8>> {
        if x.len() != y.len() {
            return Err(JupyterError::Plotting(format!(
                "X and Y lengths don't match: {} vs {}",
                x.len(),
                y.len()
            )));
        }

        if x.is_empty() {
            return Err(JupyterError::Plotting("Empty data".to_string()));
        }

        let mut buffer = Vec::new();
        {
            let root = BitMapBackend::with_buffer(&mut buffer, (self.width, self.height))
                .into_drawing_area();

            root.fill(&WHITE)
                .map_err(|e| JupyterError::Plotting(format!("Failed to fill background: {}", e)))?;

            let x_min = x.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let x_max = x.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            let y_min = y.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let y_max = y.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

            let mut chart = ChartBuilder::on(&root)
                .caption(
                    self.title.as_deref().unwrap_or("Line Plot"),
                    ("sans-serif", 30),
                )
                .margin(10)
                .x_label_area_size(30)
                .y_label_area_size(40)
                .build_cartesian_2d(x_min..x_max, y_min..y_max)
                .map_err(|e| JupyterError::Plotting(format!("Failed to build chart: {}", e)))?;

            chart
                .configure_mesh()
                .draw()
                .map_err(|e| JupyterError::Plotting(format!("Failed to draw mesh: {}", e)))?;

            chart
                .draw_series(LineSeries::new(
                    x.iter().zip(y.iter()).map(|(&x, &y)| (x, y)),
                    &BLUE,
                ))
                .map_err(|e| JupyterError::Plotting(format!("Failed to draw series: {}", e)))?;

            root.present()
                .map_err(|e| JupyterError::Plotting(format!("Failed to present: {}", e)))?;
        }

        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colormap_parsing() -> Result<()> {
        assert!(matches!(Colormap::from_str("viridis")?, Colormap::Viridis));
        assert!(matches!(
            Colormap::from_str("grayscale")?,
            Colormap::Grayscale
        ));
        assert!(Colormap::from_str("unknown").is_err());
        Ok(())
    }

    #[test]
    fn test_colormap_mapping() {
        let cmap = Colormap::Grayscale;
        let color = cmap.map(0.5);
        // Should be approximately mid-gray
        assert!((color.0 as i32 - 127).abs() < 5);
    }

    #[test]
    fn test_raster_plot_dimension_mismatch() {
        let plotter = RasterPlotter::new(100, 100);
        let data = vec![0.0; 10];
        let result = plotter.plot_to_png(&data, 5, 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_line_plot_length_mismatch() {
        let plotter = LinePlot::new(400, 300);
        let x = vec![1.0, 2.0, 3.0];
        let y = vec![1.0, 2.0];
        let result = plotter.plot_to_png(&x, &y);
        assert!(result.is_err());
    }

    #[test]
    fn test_line_plot_empty_data() {
        let plotter = LinePlot::new(400, 300);
        let x: Vec<f64> = vec![];
        let y: Vec<f64> = vec![];
        let result = plotter.plot_to_png(&x, &y);
        assert!(result.is_err());
    }

    #[test]
    fn test_raster_plotter_creation() {
        let plotter = RasterPlotter::new(800, 600)
            .with_colormap(Colormap::Viridis)
            .with_title("Test")
            .with_colorbar(true);

        assert_eq!(plotter.width, 800);
        assert_eq!(plotter.height, 600);
        assert!(plotter.title.is_some());
        assert!(plotter.show_colorbar);
    }

    #[test]
    fn test_hsv_to_rgb() {
        let color = hsv_to_rgb(0.0, 1.0, 1.0);
        // Red
        assert_eq!(color.0, 255);
        assert_eq!(color.1, 0);
        assert_eq!(color.2, 0);
    }
}
