//! Hero-render example: DEM -> hillshade + colormap -> PNG gallery.
//!
//! Pipeline: `FileDataSource` -> `GeoTiffReader` -> assemble full-res band
//! -> sanitize NoData/void pixels -> upsample to gallery width (bilinear)
//! -> `combined_hillshade` (multidirectional) -> colorize elevation with
//! `RasterRenderer` -> multiply-blend RGB by the shade -> `encode_png`.
//!
//! ```bash
//! cargo run -p oxigeo-server --example render_hero --release -- --mode all
//! ```
//!
//! Run from the repository root so the default (relative) DEM path resolves.

use anyhow::{Context, Result, bail};
use oxigeo_algorithms::raster::{CombinedHillshadeParams, aspect, combined_hillshade, slope};
use oxigeo_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_server::handlers::rendering::{Colormap, RasterRenderer, RenderStyle, encode_png};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_DEM_PATH: &str = "demo/cog-viewer/395.tif";
const OUTPUT_DIR: &str = "docs/media";
const TARGET_WIDTH: u64 = 1600;

/// Physically plausible land-surface elevation bounds (meters), used to
/// guard against stray sentinel/overflow values that aren't flagged via
/// the file's declared NoData tag.
const ELEVATION_SANE_MIN: f64 = -500.0;
const ELEVATION_SANE_MAX: f64 = 9000.0;

/// Below this slope (degrees), aspect (direction of steepest descent) is not
/// visually meaningful — masked to a neutral gray to avoid speckle from
/// meter-level DEM quantization noise on near-flat terrain (water, valley floors).
const ASPECT_FLAT_SLOPE_DEGREES: f64 = 2.0;
const ASPECT_FLAT_COLOR: [u8; 3] = [176, 176, 176];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Terrain,
    Slope,
    Aspect,
    Elevation,
}

impl RenderMode {
    const ALL: [Self; 4] = [Self::Terrain, Self::Slope, Self::Aspect, Self::Elevation];

    fn slug(self) -> &'static str {
        match self {
            Self::Terrain => "terrain",
            Self::Slope => "slope",
            Self::Aspect => "aspect",
            Self::Elevation => "elevation",
        }
    }
}

fn parse_args() -> Result<(PathBuf, Vec<RenderMode>)> {
    let mut path = PathBuf::from(DEFAULT_DEM_PATH);
    let mut mode_arg = "terrain".to_string();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                mode_arg = args
                    .next()
                    .context("--mode requires a value (terrain|slope|aspect|elevation|all)")?;
            }
            other => path = PathBuf::from(other),
        }
    }

    let modes = match mode_arg.as_str() {
        "terrain" => vec![RenderMode::Terrain],
        "slope" => vec![RenderMode::Slope],
        "aspect" => vec![RenderMode::Aspect],
        "elevation" => vec![RenderMode::Elevation],
        "all" => RenderMode::ALL.to_vec(),
        other => bail!("unknown --mode '{other}' (expected terrain|slope|aspect|elevation|all)"),
    };

    Ok((path, modes))
}

/// Converts the raw DEM buffer to Float32, replacing declared NoData,
/// non-finite, and physically implausible sentinel values (e.g. SRTM void
/// fill `-32768`) with the lowest plausible elevation found in the scene.
/// This keeps flat/sea-level areas flat instead of letting sentinel cliffs
/// speckle the hillshade and colormap output.
fn sanitize_dem(raw: &RasterBuffer) -> Result<(RasterBuffer, f64, f64)> {
    let width = raw.width();
    let height = raw.height();
    let nodata_value = raw.nodata().as_f64();

    let is_valid = |v: f64| {
        v.is_finite()
            && (ELEVATION_SANE_MIN..=ELEVATION_SANE_MAX).contains(&v)
            && Some(v) != nodata_value
    };

    let mut valid_min = f64::INFINITY;
    let mut valid_max = f64::NEG_INFINITY;
    for y in 0..height {
        for x in 0..width {
            let v = raw.get_pixel(x, y).context("DEM pixel read failed")?;
            if is_valid(v) {
                valid_min = valid_min.min(v);
                valid_max = valid_max.max(v);
            }
        }
    }
    if !valid_min.is_finite() || !valid_max.is_finite() {
        bail!("DEM contains no valid elevation samples within the plausible range");
    }

    let mut out = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let v = raw.get_pixel(x, y).context("DEM pixel read failed")?;
            let filled = if is_valid(v) { v } else { valid_min };
            out.set_pixel(x, y, filled)
                .context("failed to write sanitized DEM pixel")?;
        }
    }

    Ok((out, valid_min, valid_max))
}

/// Overwrites aspect pixels with a neutral gray wherever the terrain is too
/// flat for aspect to be meaningful (see [`ASPECT_FLAT_SLOPE_DEGREES`]).
fn mask_flat_aspect(rgba: &mut [u8], slope_buf: &RasterBuffer) {
    let (width, height) = (slope_buf.width(), slope_buf.height());
    for y in 0..height {
        for x in 0..width {
            let is_flat = slope_buf
                .get_pixel(x, y)
                .map(|s| s < ASPECT_FLAT_SLOPE_DEGREES)
                .unwrap_or(false);
            if is_flat {
                let idx = ((y * width + x) * 4) as usize;
                rgba[idx..idx + 3].copy_from_slice(&ASPECT_FLAT_COLOR);
            }
        }
    }
}

/// Multidirectional shaded terrain: colorized elevation multiply-blended
/// with a `combined_hillshade` relief.
fn render_terrain(
    dem: &RasterBuffer,
    pixel_size: f64,
    elev_min: f64,
    elev_max: f64,
) -> Result<Vec<u8>> {
    let params =
        CombinedHillshadeParams::custom(vec![225.0, 270.0, 315.0, 360.0], vec![0.3, 0.2, 0.2, 0.3])
            .with_altitude(45.0)
            .with_z_factor(1.3)
            .with_pixel_size(pixel_size);
    let shade = combined_hillshade(dem, params).context("combined_hillshade failed")?;

    let style = RenderStyle {
        colormap: Some(Colormap::Terrain),
        value_range: Some((elev_min, elev_max)),
        ..RenderStyle::default()
    };
    let mut rgba =
        RasterRenderer::render_to_rgba(dem, &style).context("terrain colorize failed")?;

    let (width, height) = (dem.width(), dem.height());
    for y in 0..height {
        for x in 0..width {
            let factor = shade.get_pixel(x, y).unwrap_or(255.0).clamp(0.0, 255.0) / 255.0;
            let idx = ((y * width + x) * 4) as usize;
            for channel in &mut rgba[idx..idx + 3] {
                *channel = (f64::from(*channel) * factor).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    Ok(rgba)
}

fn render_mode(
    mode: RenderMode,
    dem: &RasterBuffer,
    pixel_size: f64,
    elev_min: f64,
    elev_max: f64,
) -> Result<Vec<u8>> {
    match mode {
        RenderMode::Terrain => render_terrain(dem, pixel_size, elev_min, elev_max),
        RenderMode::Elevation => {
            let style = RenderStyle {
                colormap: Some(Colormap::Viridis),
                value_range: Some((elev_min, elev_max)),
                ..RenderStyle::default()
            };
            RasterRenderer::render_to_rgba(dem, &style).context("elevation render failed")
        }
        RenderMode::Slope => {
            let slope_buf = slope(dem, pixel_size, 1.0).context("slope computation failed")?;
            let style = RenderStyle {
                colormap: Some(Colormap::Spectral),
                value_range: Some((0.0, 60.0)),
                ..RenderStyle::default()
            };
            RasterRenderer::render_to_rgba(&slope_buf, &style).context("slope render failed")
        }
        RenderMode::Aspect => {
            let slope_buf = slope(dem, pixel_size, 1.0).context("slope computation failed")?;
            let aspect_buf = aspect(dem, pixel_size, 1.0).context("aspect computation failed")?;
            let style = RenderStyle {
                colormap: Some(Colormap::Jet),
                value_range: Some((0.0, 360.0)),
                ..RenderStyle::default()
            };
            let mut rgba = RasterRenderer::render_to_rgba(&aspect_buf, &style)
                .context("aspect render failed")?;
            mask_flat_aspect(&mut rgba, &slope_buf);
            Ok(rgba)
        }
    }
}

fn main() -> Result<()> {
    let (path, modes) = parse_args()?;

    println!("Loading DEM: {}", path.display());
    let source = FileDataSource::open(&path)
        .with_context(|| format!("failed to open '{}'", path.display()))?;
    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("failed to parse GeoTIFF '{}'", path.display()))?;

    let (width, height) = (reader.width(), reader.height());
    let data_type = reader
        .data_type()
        .context("DEM has no readable data type")?;
    let nodata = reader.nodata();
    let pixel_size = reader
        .geo_transform()
        .map(|gt| gt.pixel_width.abs())
        .filter(|v| *v > 0.0)
        .unwrap_or(1.0);
    println!("  {width}x{height} px, {data_type:?}, nodata={nodata:?}, pixel_size={pixel_size:.3}");

    let raw_bytes = reader.read_band(0, 0).context("failed to read DEM band")?;
    let raw = RasterBuffer::new(raw_bytes, width, height, data_type, nodata)
        .context("failed to build raster buffer from DEM band")?;
    let (clean_dem, elev_min, elev_max) = sanitize_dem(&raw)?;
    println!("  elevation range (sanitized): {elev_min:.1}..{elev_max:.1} m");

    let target_height =
        (((height as f64) * (TARGET_WIDTH as f64) / (width as f64)).round() as u64).max(1);
    let dem_hi = Resampler::new(ResamplingMethod::Bilinear)
        .resample(&clean_dem, TARGET_WIDTH, target_height)
        .context("failed to upsample DEM")?;
    let pixel_size_hi = pixel_size * (width as f64) / (TARGET_WIDTH as f64);
    println!(
        "  upsampled to {}x{} px (pixel_size={pixel_size_hi:.3})",
        dem_hi.width(),
        dem_hi.height()
    );

    fs::create_dir_all(OUTPUT_DIR).with_context(|| format!("failed to create '{OUTPUT_DIR}'"))?;

    for mode in modes {
        let rgba = render_mode(mode, &dem_hi, pixel_size_hi, elev_min, elev_max)?;
        let out_path = Path::new(OUTPUT_DIR).join(format!("geolab-render-{}.png", mode.slug()));
        let png_bytes = encode_png(&rgba, dem_hi.width() as u32, dem_hi.height() as u32)
            .with_context(|| format!("PNG encoding failed for {}", mode.slug()))?;
        fs::write(&out_path, &png_bytes)
            .with_context(|| format!("failed to write '{}'", out_path.display()))?;
        println!(
            "  wrote {} ({}x{}, {} bytes)",
            out_path.display(),
            dem_hi.width(),
            dem_hi.height(),
            png_bytes.len()
        );
    }

    Ok(())
}
