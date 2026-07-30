//! Virtual Raster (VRT) construction from multiple source datasets.
//!
//! This module provides `build_vrt`, which generates a GDAL-compatible VRT
//! XML file describing a mosaic of multiple raster datasets. The resulting
//! file can be opened with [`crate::Dataset::open`] or any GDAL-compatible
//! reader.
//!
//! # Design
//!
//! VRT files are pure XML — no pixel data is copied. Each source dataset
//! contributes a `<VRTRasterBand>` → `<SimpleSource>` stanza that tells
//! readers where to fetch pixel data at runtime.
//!
//! # Examples
//!
//! ```rust,no_run
//! use std::path::Path;
//! use oxigeo::vrt_builder::{build_vrt, VrtOptions, VrtResolution};
//!
//! # fn main() -> oxigeo::Result<()> {
//! let sources = [Path::new("tile_a.tif"), Path::new("tile_b.tif")];
//! let output = Path::new("mosaic.vrt");
//! let options = VrtOptions::default();
//! let ds = build_vrt(&sources, output, options)?;
//! println!("VRT extent: {}×{}", ds.width(), ds.height());
//! # Ok(())
//! # }
//! ```

use std::path::Path;

use crate::{Dataset, DatasetFormat, DatasetInfo, GeoTransform, OxiGeoError, Result};

// ─── Public types ─────────────────────────────────────────────────────────────

/// Resolution rule for combining multiple source rasters.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum VrtResolution {
    /// Use the arithmetic average of all source pixel sizes (default).
    #[default]
    Average,
    /// Use the finest (smallest pixel) resolution.
    Highest,
    /// Use the coarsest (largest pixel) resolution.
    Lowest,
    /// Use a user-specified pixel size.
    ///
    /// The value is the pixel width (and height, for square pixels).
    User(f64),
}

/// Options controlling [`build_vrt`].
#[derive(Debug, Clone, Default)]
pub struct VrtOptions {
    /// Resolution rule for the output VRT.
    pub resolution: VrtResolution,
    /// NoData value for the output bands (applied to all sources).
    pub no_data: Option<f64>,
    /// When `true`, each source file contributes a *separate* band rather than
    /// overlapping bands being composited by first-come-first-served order.
    pub separate_bands: bool,
    /// Source NoData value to mask before compositing.
    pub srcnodata: Option<f64>,
}

// ─── Main entry point ─────────────────────────────────────────────────────────

/// Build a GDAL-compatible VRT mosaic from a list of source raster files.
///
/// The function:
/// 1. Opens each source (or extension fallback) to read width, height, and geotransform.
///    fallback) to read width, height, and geotransform.
/// 2. Computes the union bounding box and output pixel dimensions.
/// 3. Writes GDAL VRT XML to `output_path`.
/// 4. Returns a [`Dataset`] opened from the newly written VRT.
///
/// # Errors
///
/// - [`OxiGeoError::InvalidParameter`] — `sources` is empty or any source
///   cannot be opened.
/// - [`OxiGeoError::Io`] — cannot write the output VRT file.
pub fn build_vrt(sources: &[&Path], output_path: &Path, options: VrtOptions) -> Result<Dataset> {
    if sources.is_empty() {
        return Err(OxiGeoError::InvalidParameter {
            parameter: "sources",
            message: "at least one source file is required to build a VRT".to_string(),
        });
    }

    // Gather metadata for each source.
    let source_metas: Vec<SourceMeta> = sources
        .iter()
        .enumerate()
        .map(|(idx, &src)| {
            read_source_meta(src).map_err(|e| OxiGeoError::InvalidParameter {
                parameter: "sources",
                message: format!(
                    "failed to read metadata for source[{}] '{}': {e}",
                    idx,
                    src.display()
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Compute output resolution.
    let pixel_size = resolve_pixel_size(&source_metas, &options.resolution);

    // Compute union bounding box.
    let union_bbox = compute_union_bbox(&source_metas)?;

    // Output dimensions.
    let total_width = ((union_bbox.max_x - union_bbox.min_x) / pixel_size).ceil() as u32;
    let total_height = ((union_bbox.max_y - union_bbox.min_y) / pixel_size).ceil() as u32;

    // Output band count depends on the compositing mode:
    // - separate_bands: every source contributes its own distinct output bands,
    //   so the total is the *sum* of per-source band counts.
    // - overlapping (default): sources are stacked, so the output has as many
    //   bands as the source with the most bands (the MAX).
    let band_count: u32 = if options.separate_bands {
        source_metas
            .iter()
            .map(|m| m.band_count)
            .sum::<u32>()
            .max(1)
    } else {
        source_metas.iter().map(|m| m.band_count).max().unwrap_or(1)
    };

    // Write the VRT XML.
    let xml = generate_vrt_xml(
        &source_metas,
        sources,
        &union_bbox,
        total_width,
        total_height,
        pixel_size,
        band_count,
        &options,
    );

    let output_str = output_path
        .to_str()
        .ok_or_else(|| OxiGeoError::InvalidParameter {
            parameter: "output_path",
            message: "output path contains non-UTF-8 characters".to_string(),
        })?;

    std::fs::write(output_path, &xml).map_err(|e| {
        OxiGeoError::Io(oxigeo_core::error::IoError::Write {
            message: format!("failed to write VRT file '{}': {e}", output_str),
        })
    })?;

    // Build a Dataset metadata descriptor for the newly written VRT.
    let vrt_gt = GeoTransform::north_up(union_bbox.min_x, union_bbox.max_y, pixel_size, pixel_size);

    let info = DatasetInfo {
        format: DatasetFormat::Vrt,
        path: Some(output_str.to_string()),
        width: Some(total_width),
        height: Some(total_height),
        band_count,
        crs: source_metas.first().and_then(|m| m.crs.clone()),
        geotransform: Some(vrt_gt),
        // Every source is validated to share one pixel type by the caller, so
        // the VRT inherits the first source's element type.
        data_type: source_metas.first().and_then(|m| m.data_type),
        ..DatasetInfo::default()
    };

    Ok(Dataset::from_info(output_str.to_string(), info))
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Lightweight metadata extracted from a source file for VRT construction.
#[derive(Debug, Clone)]
struct SourceMeta {
    /// Pixel width.
    pixel_width: f64,
    /// Pixel height (absolute value, always positive).
    pixel_height: f64,
    /// Raster width in pixels.
    width: u32,
    /// Raster height in pixels.
    height: u32,
    /// Number of bands.
    band_count: u32,
    /// Geotransform origin X (top-left corner).
    origin_x: f64,
    /// Geotransform origin Y (top-left corner).
    origin_y: f64,
    /// Pixel element type parsed from the source header, when known.
    data_type: Option<oxigeo_core::types::RasterDataType>,
    /// GDAL data type string (e.g. "Float32").
    data_type_str: String,
    /// CRS string (optional).
    crs: Option<String>,
}

impl SourceMeta {
    /// Left-most X coordinate.
    fn min_x(&self) -> f64 {
        self.origin_x
    }

    /// Right-most X coordinate.
    fn max_x(&self) -> f64 {
        self.origin_x + self.width as f64 * self.pixel_width
    }

    /// Bottom-most Y coordinate (north-up: origin_y − height × pixel_height).
    fn min_y(&self) -> f64 {
        self.origin_y - self.height as f64 * self.pixel_height
    }

    /// Top-most Y coordinate.
    fn max_y(&self) -> f64 {
        self.origin_y
    }
}

/// Bounding box used internally for VRT construction.
#[derive(Debug, Clone)]
struct Bbox {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

/// Read lightweight raster metadata from a source file.
fn read_source_meta(path: &Path) -> Result<SourceMeta> {
    // Attempt header-level parsing for GeoTIFF; anything else is unsupported.
    match crate::open::extract_tiff_info(path) {
        Ok(info) => {
            let gt = info
                .geotransform
                .unwrap_or_else(|| GeoTransform::north_up(0.0, 0.0, 1.0, 1.0));
            let width = info.width.unwrap_or(1);
            let height = info.height.unwrap_or(1);
            let pixel_height = gt.pixel_height.abs();
            // Use the real pixel type (BitsPerSample / SampleFormat) rather
            // than assuming Float32 — UInt8/UInt16 imagery would otherwise be
            // misinterpreted as 4-byte floats by VRT readers.
            let data_type_str = info
                .data_type
                .map(|dt| dt.name().to_string())
                .unwrap_or_else(|| "Float32".to_string());
            Ok(SourceMeta {
                pixel_width: gt.pixel_width,
                pixel_height,
                width,
                height,
                band_count: info.band_count.max(1),
                origin_x: gt.origin_x,
                origin_y: gt.origin_y,
                data_type: info.data_type,
                data_type_str,
                crs: info.crs,
            })
        }
        Err(e) => Err(OxiGeoError::InvalidParameter {
            parameter: "source",
            message: format!(
                "cannot read raster metadata from '{}' — only GeoTIFF sources are supported ({e})",
                path.display()
            ),
        }),
    }
}

/// Compute the output pixel size from source metadata and the resolution rule.
fn resolve_pixel_size(metas: &[SourceMeta], rule: &VrtResolution) -> f64 {
    match rule {
        VrtResolution::User(px) => *px,
        VrtResolution::Average => {
            let sum: f64 = metas.iter().map(|m| m.pixel_width).sum();
            sum / metas.len() as f64
        }
        VrtResolution::Highest => metas
            .iter()
            .map(|m| m.pixel_width)
            .fold(f64::INFINITY, f64::min),
        VrtResolution::Lowest => metas.iter().map(|m| m.pixel_width).fold(0.0f64, f64::max),
    }
}

/// Compute the union bounding box over all sources.
fn compute_union_bbox(metas: &[SourceMeta]) -> Result<Bbox> {
    let first = metas.first().ok_or_else(|| OxiGeoError::InvalidParameter {
        parameter: "sources",
        message: "no sources to compute bbox from".to_string(),
    })?;

    let mut min_x = first.min_x();
    let mut min_y = first.min_y();
    let mut max_x = first.max_x();
    let mut max_y = first.max_y();

    for m in metas.iter().skip(1) {
        if m.min_x() < min_x {
            min_x = m.min_x();
        }
        if m.min_y() < min_y {
            min_y = m.min_y();
        }
        if m.max_x() > max_x {
            max_x = m.max_x();
        }
        if m.max_y() > max_y {
            max_y = m.max_y();
        }
    }

    Ok(Bbox {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

/// Map a GDAL data type string to the VRT-compatible token.
fn gdal_dtype_str(dt_str: &str) -> &str {
    match dt_str {
        "UInt8" => "Byte",
        "UInt16" => "UInt16",
        "Int16" => "Int16",
        "UInt32" => "UInt32",
        "Int32" => "Int32",
        "Float32" => "Float32",
        "Float64" => "Float64",
        other => other,
    }
}

/// Generate the GDAL VRT XML string for the mosaic.
#[allow(clippy::too_many_arguments)]
fn generate_vrt_xml(
    metas: &[SourceMeta],
    sources: &[&Path],
    bbox: &Bbox,
    total_width: u32,
    total_height: u32,
    pixel_size: f64,
    band_count: u32,
    options: &VrtOptions,
) -> String {
    let mut xml = String::with_capacity(4096);

    // VRT header
    xml.push_str("<VRTDataset rasterXSize=\"");
    xml.push_str(&total_width.to_string());
    xml.push_str("\" rasterYSize=\"");
    xml.push_str(&total_height.to_string());
    xml.push_str("\">\n");

    // GeoTransform: origin_x, pixel_width, 0, origin_y, 0, -pixel_height
    xml.push_str("  <GeoTransform>");
    xml.push_str(&format!(
        "{:.10}, {:.10}, 0.0, {:.10}, 0.0, -{:.10}",
        bbox.min_x, pixel_size, bbox.max_y, pixel_size
    ));
    xml.push_str("</GeoTransform>\n");

    // SRS from first source
    if let Some(crs) = metas.first().and_then(|m| m.crs.as_ref()) {
        xml.push_str("  <SRS>");
        xml.push_str(crs);
        xml.push_str("</SRS>\n");
    }

    // Assign, per output band, the list of `(source_index, source_local_band)`
    // pairs that feed it.
    //
    // - separate_bands: each output band is fed by exactly ONE (source, local
    //   band) — the sources' bands are concatenated, never composited.
    // - overlapping (default): output band N is composited from every source
    //   that actually *has* a band N.  Sources with fewer than N bands are
    //   skipped, so we never emit a `<SourceBand>` index a source does not
    //   possess (which would produce an unreadable VRT).
    let assignments: Vec<Vec<(usize, u32)>> = if options.separate_bands {
        let mut per_band = Vec::new();
        for (si, meta) in metas.iter().enumerate() {
            for local_band in 1..=meta.band_count {
                per_band.push(vec![(si, local_band)]);
            }
        }
        if per_band.is_empty() {
            per_band.push(Vec::new());
        }
        per_band
    } else {
        (1..=band_count)
            .map(|b| {
                metas
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| b <= m.band_count)
                    .map(|(si, _)| (si, b))
                    .collect()
            })
            .collect()
    };

    // Emit VRTRasterBand elements
    for (band0, sources_for_band) in assignments.iter().enumerate() {
        let band_idx = (band0 as u32) + 1;

        // Data type: for separate bands use the contributing source's type;
        // otherwise fall back to the first source (overlapping bands share type).
        let dt_str = sources_for_band
            .first()
            .and_then(|&(si, _)| metas.get(si))
            .or_else(|| metas.first())
            .map(|m| gdal_dtype_str(&m.data_type_str))
            .unwrap_or("Float32");

        xml.push_str("  <VRTRasterBand dataType=\"");
        xml.push_str(dt_str);
        xml.push_str("\" band=\"");
        xml.push_str(&band_idx.to_string());
        xml.push_str("\">\n");

        // NoData
        if let Some(nd) = options.no_data {
            xml.push_str("    <NoDataValue>");
            xml.push_str(&nd.to_string());
            xml.push_str("</NoDataValue>\n");
        }

        // Simple sources feeding this output band.
        for &(src_idx, source_band) in sources_for_band {
            let (meta, src_path) = match (metas.get(src_idx), sources.get(src_idx)) {
                (Some(m), Some(p)) => (m, p),
                _ => continue,
            };
            // Offset in output pixels for this source.
            let dst_off_x = ((meta.origin_x - bbox.min_x) / pixel_size).round() as i64;
            let dst_off_y = ((bbox.max_y - meta.max_y()) / pixel_size).round() as i64;
            let dst_w = (meta.width as f64 * meta.pixel_width / pixel_size).ceil() as u32;
            let dst_h = (meta.height as f64 * meta.pixel_height / pixel_size).ceil() as u32;

            let src_path_str = src_path.to_string_lossy();

            xml.push_str("    <SimpleSource>\n");
            xml.push_str("      <SourceFilename relativeToVRT=\"1\">");
            xml.push_str(&src_path_str);
            xml.push_str("</SourceFilename>\n");
            xml.push_str("      <SourceBand>");
            xml.push_str(&source_band.to_string());
            xml.push_str("</SourceBand>\n");

            // SrcRect: full source extent
            xml.push_str("      <SrcRect xOff=\"0\" yOff=\"0\" xSize=\"");
            xml.push_str(&meta.width.to_string());
            xml.push_str("\" ySize=\"");
            xml.push_str(&meta.height.to_string());
            xml.push_str("\"/>\n");

            // DstRect: where this source maps in the output
            xml.push_str("      <DstRect xOff=\"");
            xml.push_str(&dst_off_x.to_string());
            xml.push_str("\" yOff=\"");
            xml.push_str(&dst_off_y.to_string());
            xml.push_str("\" xSize=\"");
            xml.push_str(&dst_w.to_string());
            xml.push_str("\" ySize=\"");
            xml.push_str(&dst_h.to_string());
            xml.push_str("\"/>\n");

            if let Some(nd) = options.srcnodata {
                xml.push_str("      <NODATA>");
                xml.push_str(&nd.to_string());
                xml.push_str("</NODATA>\n");
            }

            xml.push_str("    </SimpleSource>\n");
        }

        xml.push_str("  </VRTRasterBand>\n");
    }

    xml.push_str("</VRTDataset>\n");
    xml
}

// ─── Dataset convenience wrapper ─────────────────────────────────────────────

impl Dataset {
    /// Build a GDAL-compatible Virtual Raster (VRT) mosaic from multiple source files.
    ///
    /// Delegates to [`build_vrt`].  See its documentation for full details.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::InvalidParameter`] — `sources` is empty or a source cannot be opened.
    /// - [`OxiGeoError::Io`] — cannot write the output VRT file.
    pub fn build_vrt(
        sources: &[&Path],
        output_path: &Path,
        options: VrtOptions,
    ) -> Result<Dataset> {
        build_vrt(sources, output_path, options)
    }
}

#[cfg(all(test, feature = "geotiff"))]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::builder::{DatasetCreateBuilder, OutputFormat};
    use oxigeo_core::types::RasterDataType;

    fn write_nband_geotiff(path: &Path, bands: u32) {
        let mut writer = DatasetCreateBuilder::new(path, OutputFormat::GeoTiff)
            .create()
            .expect("create writer");
        writer.set_dimensions(2, 2, bands).expect("dims");
        writer.set_data_type(RasterDataType::UInt8);
        writer.set_geo_transform(GeoTransform::north_up(0.0, 2.0, 1.0, 1.0));
        let data: Vec<u8> = vec![0u8; (2 * 2 * bands) as usize];
        writer.write_all_bands(&data).expect("write bands");
        writer.finalize().expect("finalize");
    }

    #[test]
    fn test_vrt_overlap_skips_missing_bands() {
        let dir = std::env::temp_dir();
        let src_rgb = dir.join("vrt_overlap_rgb.tif");
        let src_gray = dir.join("vrt_overlap_gray.tif");
        let out = dir.join("vrt_overlap_out.vrt");
        write_nband_geotiff(&src_rgb, 3);
        write_nband_geotiff(&src_gray, 1);

        let sources: Vec<&Path> = vec![src_rgb.as_path(), src_gray.as_path()];
        let ds = build_vrt(&sources, &out, VrtOptions::default()).expect("build vrt");
        // Overlap mode: output band count is the MAX (3).
        assert_eq!(ds.band_count(), 3);

        let xml = std::fs::read_to_string(&out).expect("read vrt");
        // The 1-band grayscale source must appear in exactly ONE SimpleSource
        // (feeding band 1 only) — never referenced for bands 2/3 which it lacks.
        let gray_name = src_gray.file_name().and_then(|n| n.to_str()).expect("name");
        let gray_refs = xml.matches(gray_name).count();
        assert_eq!(
            gray_refs, 1,
            "1-band source must feed only band 1, got {gray_refs} references"
        );
    }

    #[test]
    fn test_vrt_separate_bands_sums_band_count() {
        let dir = std::env::temp_dir();
        let src_rgb = dir.join("vrt_sep_rgb.tif");
        let src_gray = dir.join("vrt_sep_gray.tif");
        let out = dir.join("vrt_sep_out.vrt");
        write_nband_geotiff(&src_rgb, 3);
        write_nband_geotiff(&src_gray, 1);

        let sources: Vec<&Path> = vec![src_rgb.as_path(), src_gray.as_path()];
        let options = VrtOptions {
            separate_bands: true,
            ..VrtOptions::default()
        };
        let ds = build_vrt(&sources, &out, options).expect("build vrt");
        // Separate mode: output band count is the SUM (3 + 1 = 4).
        assert_eq!(ds.band_count(), 4);

        let xml = std::fs::read_to_string(&out).expect("read vrt");
        assert_eq!(
            xml.matches("<VRTRasterBand").count(),
            4,
            "separate_bands should emit one VRTRasterBand per source band"
        );
    }
}
