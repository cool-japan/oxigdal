//! Tileindex command - Generate tile index of raster file extents

use crate::OutputFormat;
use crate::util::raster;
use anyhow::{Context, Result};
use clap::Args;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

/// Generate tile index of raster file extents
#[derive(Args, Debug)]
pub struct TileIndexArgs {
    /// Input raster files
    #[arg(value_name = "INPUT", required = true)]
    pub inputs: Vec<PathBuf>,

    /// Output file (Shapefile .shp or GeoJSON .geojson)
    #[arg(short = 'o', long = "output", value_name = "OUTPUT")]
    pub output: PathBuf,

    /// Field name for source file path
    #[arg(long, default_value = "location")]
    pub src_field: String,
}

struct TileEntry {
    path: String,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

pub fn execute(args: TileIndexArgs, _format: OutputFormat) -> Result<()> {
    if args.inputs.is_empty() {
        anyhow::bail!("no input files specified");
    }

    let mut entries: Vec<TileEntry> = Vec::new();

    for input in &args.inputs {
        let info = match raster::read_raster_info(input) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("Skipping {}: {}", input.display(), e);
                continue;
            }
        };

        let gt = match &info.geo_transform {
            Some(g) => *g,
            None => {
                tracing::warn!("Skipping {}: no geotransform", input.display());
                continue;
            }
        };

        let w = info.width as f64;
        let h = info.height as f64;

        // Four corners in pixel space → geographic
        let corners = [(0.0_f64, 0.0_f64), (w, 0.0), (0.0, h), (w, h)];

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for (px, py) in corners {
            let gx = gt.origin_x + px * gt.pixel_width + py * gt.row_rotation;
            let gy = gt.origin_y + px * gt.col_rotation + py * gt.pixel_height;
            min_x = min_x.min(gx);
            min_y = min_y.min(gy);
            max_x = max_x.max(gx);
            max_y = max_y.max(gy);
        }

        entries.push(TileEntry {
            path: input.display().to_string(),
            min_x,
            min_y,
            max_x,
            max_y,
        });
    }

    if entries.is_empty() {
        anyhow::bail!("no valid input tiles to index");
    }

    let ext = args
        .output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "shp" => write_shapefile(&args.output, &entries, &args.src_field)
            .context("Failed to write Shapefile tile index")?,
        _ => write_geojson(&args.output, &entries, &args.src_field)
            .context("Failed to write GeoJSON tile index")?,
    }

    tracing::info!("Created tile index: {} tiles indexed", entries.len());
    Ok(())
}

fn bbox_polygon_coords(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<(f64, f64)> {
    // Closed exterior ring, CW winding (Shapefile spec)
    vec![
        (min_x, max_y),
        (max_x, max_y),
        (max_x, min_y),
        (min_x, min_y),
        (min_x, max_y),
    ]
}

fn write_geojson(output: &std::path::Path, entries: &[TileEntry], src_field: &str) -> Result<()> {
    let features: Vec<Value> = entries
        .iter()
        .map(|e| {
            let ring: Vec<Vec<f64>> = bbox_polygon_coords(e.min_x, e.min_y, e.max_x, e.max_y)
                .into_iter()
                .map(|(x, y)| vec![x, y])
                .collect();
            json!({
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [ring]
                },
                "properties": {
                    src_field: e.path
                }
            })
        })
        .collect();

    let fc = json!({
        "type": "FeatureCollection",
        "features": features
    });

    let contents = serde_json::to_string_pretty(&fc)?;
    std::fs::write(output, contents)?;
    Ok(())
}

fn write_shapefile(output: &std::path::Path, entries: &[TileEntry], src_field: &str) -> Result<()> {
    use oxigeo_core::vector::{Coordinate, FieldValue, Geometry, LineString, Polygon};
    use oxigeo_shapefile::{ShapeType, ShapefileSchemaBuilder, ShapefileWriter};

    let base = output.with_extension("");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field(src_field, 254)
        .map_err(|e| anyhow::anyhow!("Schema build failed: {e}"))?
        .build();

    let mut writer = ShapefileWriter::new(&base, ShapeType::Polygon, schema)
        .map_err(|e| anyhow::anyhow!("ShapefileWriter::new failed: {e}"))?;

    let features: Vec<oxigeo_shapefile::reader::ShapefileFeature> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let ring_pts: Vec<Coordinate> = bbox_polygon_coords(e.min_x, e.min_y, e.max_x, e.max_y)
                .into_iter()
                .map(|(x, y)| Coordinate::new_2d(x, y))
                .collect();

            let exterior =
                LineString::new(ring_pts).unwrap_or_else(|_| LineString { coords: Vec::new() });
            let polygon = Polygon::new(exterior, Vec::new()).unwrap_or_else(|_| Polygon {
                exterior: LineString { coords: Vec::new() },
                interiors: Vec::new(),
            });

            let geom = Geometry::Polygon(polygon);
            let mut attrs = HashMap::new();
            attrs.insert(src_field.to_string(), FieldValue::String(e.path.clone()));

            oxigeo_shapefile::ShapefileFeature::new((i + 1) as i32, Some(geom), attrs)
        })
        .collect();

    writer
        .write_features(&features)
        .map_err(|e| anyhow::anyhow!("write_features failed: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_cli_tileindex_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_bbox_polygon_closes() {
        let coords = bbox_polygon_coords(0.0, 0.0, 1.0, 1.0);
        assert_eq!(coords.len(), 5);
        assert_eq!(coords[0], coords[4], "polygon ring must be closed");
    }

    #[test]
    fn test_execute_empty_input_errors() {
        let output = TempPath::new("out.geojson");
        let args = TileIndexArgs {
            inputs: Vec::new(),
            output: output.to_path_buf(),
            src_field: "location".to_string(),
        };
        let result = execute(args, OutputFormat::Text);
        assert!(result.is_err());
    }
}
