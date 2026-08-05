//! Warped VRT support — the `<GDALWarpOptions>` element of a
//! `VRTDataset subClass="VRTWarpedDataset"`.
//!
//! A warped VRT does not carry per-band `<SimpleSource>`/`<ComplexSource>`
//! elements. Instead a single `<GDALWarpOptions>` block names one source
//! dataset plus the transformation that maps the warped (destination) pixel
//! grid onto it, and every band is materialised by resampling that source.
//! This module models that block; `warped` executes it.
//!
//! The transformer chain GDAL writes is
//! `Transformer → ApproxTransformer → BaseTransformer → GenImgProjTransformer
//! → ReprojectTransformer → ReprojectionTransformer`, with the `Approx*`
//! wrapper present only when a spline approximation was requested. The parser
//! below walks the subtree by element name rather than by position, so both the
//! wrapped and unwrapped forms — and GDAL's habit of adding elements between
//! releases — parse identically.

use crate::error::{Result, VrtError};
use crate::source::SourceFilename;
use oxigeo_core::types::{GeoTransform, RasterDataType};
use serde::{Deserialize, Serialize};

/// Resampling algorithm named by `<ResampleAlg>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WarpResampleAlg {
    /// Nearest neighbour — GDAL's default when `<ResampleAlg>` is absent.
    #[default]
    NearestNeighbour,
    /// Bilinear interpolation over the four neighbouring samples.
    Bilinear,
    /// Cubic convolution.
    Cubic,
    /// B-spline cubic.
    CubicSpline,
    /// Lanczos windowed sinc.
    Lanczos,
    /// Average of all contributing source pixels.
    Average,
    /// Most frequently occurring value.
    Mode,
}

/// The interpolation kernel a warped read actually applies.
///
/// [`WarpResampleAlg`] records what the VRT asked for; this records what is
/// implemented. Anything wider than bilinear currently degrades to
/// [`WarpKernel::Bilinear`] — see [`WarpResampleAlg::kernel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarpKernel {
    /// Take the value of the source pixel containing the sample point.
    Nearest,
    /// Weight the four samples surrounding the sample point by area.
    Bilinear,
}

impl WarpResampleAlg {
    /// Parses a GDAL `<ResampleAlg>` spelling. Unknown values fall back to
    /// [`Self::NearestNeighbour`], matching GDAL's own default.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "bilinear" => Self::Bilinear,
            "cubic" => Self::Cubic,
            "cubicspline" => Self::CubicSpline,
            "lanczos" => Self::Lanczos,
            "average" => Self::Average,
            "mode" => Self::Mode,
            _ => Self::NearestNeighbour,
        }
    }

    /// The GDAL spelling of this algorithm.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NearestNeighbour => "NearestNeighbour",
            Self::Bilinear => "Bilinear",
            Self::Cubic => "Cubic",
            Self::CubicSpline => "CubicSpline",
            Self::Lanczos => "Lanczos",
            Self::Average => "Average",
            Self::Mode => "Mode",
        }
    }

    /// The kernel a warped read applies for this algorithm.
    ///
    /// Only nearest-neighbour and bilinear have kernels of their own here.
    /// `Cubic`, `CubicSpline`, `Lanczos`, `Average` and `Mode` parse and are
    /// preserved on round-trip, but resample **bilinearly**; they are reported
    /// honestly by [`Self::is_kernel_exact`] so a caller can tell whether the
    /// pixels it got match the algorithm the VRT asked for.
    #[must_use]
    pub fn kernel(self) -> WarpKernel {
        match self {
            Self::NearestNeighbour => WarpKernel::Nearest,
            _ => WarpKernel::Bilinear,
        }
    }

    /// Whether [`Self::kernel`] implements this algorithm exactly.
    #[must_use]
    pub fn is_kernel_exact(self) -> bool {
        matches!(self, Self::NearestNeighbour | Self::Bilinear)
    }
}

/// How the destination buffer is initialised before warping, from the
/// `INIT_DEST` warp option.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InitDest {
    /// No initialisation requested — the buffer stays zero-filled.
    None,
    /// Initialise to each band's destination NoData value.
    NoData,
    /// Initialise to a fixed value.
    Value(f64),
}

/// The `<ReprojectionTransformer>` leaf: the CRS pair the warp reprojects
/// between.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReprojectionTransformer {
    /// Source CRS, as WKT / `EPSG:code` / PROJ string.
    pub source_srs: Option<String>,
    /// Target CRS, as WKT / `EPSG:code` / PROJ string.
    pub target_srs: Option<String>,
    /// `<Options><Option key="…">` entries (e.g. `CENTER_LONG`).
    pub options: Vec<(String, String)>,
}

impl ReprojectionTransformer {
    /// Looks up a reprojection option by key (case-insensitive).
    #[must_use]
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// The `<GenImgProjTransformer>` node: source and destination geotransforms
/// plus the optional CRS reprojection between them.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GenImgProjTransformer {
    /// Source image geotransform (source pixel grid → source CRS).
    pub src_geo_transform: Option<GeoTransform>,
    /// Destination image geotransform (warped pixel grid → target CRS).
    pub dst_geo_transform: Option<GeoTransform>,
    /// CRS reprojection applied between the two grids.
    pub reprojection: Option<ReprojectionTransformer>,
    /// `<MaxError>` of an enclosing `<ApproxTransformer>`, in destination
    /// pixels.
    ///
    /// Recorded for fidelity only. Warped reads transform **every** pixel
    /// exactly instead of building the approximating spline this bounds, which
    /// is slower but never less accurate.
    pub max_error: Option<f64>,
}

/// One `<BandMapping>` entry of the `<BandList>`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WarpBandMapping {
    /// Source band (1-based).
    pub src: usize,
    /// Destination band (1-based).
    pub dst: usize,
    /// Real part of the source NoData value.
    pub src_nodata_real: Option<f64>,
    /// Real part of the destination NoData value.
    pub dst_nodata_real: Option<f64>,
}

/// A parsed `<GDALWarpOptions>` block.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WarpOptions {
    /// `<SourceDataset>` — the dataset being warped.
    pub source_dataset: Option<SourceFilename>,
    /// `<ResampleAlg>`.
    pub resample_alg: WarpResampleAlg,
    /// `<WorkingDataType>` — the type the warper computes in.
    pub working_data_type: Option<RasterDataType>,
    /// `<WarpMemoryLimit>` in bytes.
    pub warp_memory_limit: Option<f64>,
    /// `<Option name="…">` entries.
    pub options: Vec<(String, String)>,
    /// The transformer chain, flattened to the parameters that matter.
    pub transformer: Option<GenImgProjTransformer>,
    /// `<BandList>` band mappings.
    pub band_mappings: Vec<WarpBandMapping>,
}

impl WarpOptions {
    /// Looks up a warp option by name (case-insensitive).
    #[must_use]
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The `INIT_DEST` policy for the destination buffer.
    #[must_use]
    pub fn init_dest(&self) -> InitDest {
        match self.option("INIT_DEST") {
            None => InitDest::None,
            Some(v) if v.eq_ignore_ascii_case("NO_DATA") => InitDest::NoData,
            Some(v) => v
                .trim()
                .parse::<f64>()
                .map_or(InitDest::None, InitDest::Value),
        }
    }

    /// The source band feeding destination band `dst` (both 1-based).
    ///
    /// Falls back to the identity mapping when the `<BandList>` is absent,
    /// which is what GDAL does for a warp that does not reorder bands.
    #[must_use]
    pub fn source_band_for(&self, dst: usize) -> usize {
        self.band_mappings
            .iter()
            .find(|m| m.dst == dst)
            .map_or(dst, |m| m.src)
    }

    /// The mapping entry for destination band `dst` (1-based), if any.
    #[must_use]
    pub fn mapping_for(&self, dst: usize) -> Option<&WarpBandMapping> {
        self.band_mappings.iter().find(|m| m.dst == dst)
    }

    /// Validates that the block carries enough information to warp.
    ///
    /// # Errors
    /// Returns an error when no source dataset is named.
    pub fn validate(&self) -> Result<()> {
        if self.source_dataset.is_none() {
            return Err(VrtError::invalid_structure(
                "GDALWarpOptions has no <SourceDataset>",
            ));
        }
        Ok(())
    }
}

/// Parses a GDAL data-type name, tolerating the ones a warped VRT may name in
/// `<WorkingDataType>` but that no band can declare.
pub(crate) fn parse_working_data_type(s: &str) -> Option<RasterDataType> {
    match s.trim() {
        "Byte" => Some(RasterDataType::UInt8),
        "Int8" => Some(RasterDataType::Int8),
        "UInt16" => Some(RasterDataType::UInt16),
        "Int16" => Some(RasterDataType::Int16),
        "UInt32" => Some(RasterDataType::UInt32),
        "Int32" => Some(RasterDataType::Int32),
        "Float32" => Some(RasterDataType::Float32),
        "Float64" => Some(RasterDataType::Float64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_alg_parse() {
        assert_eq!(
            WarpResampleAlg::parse("Bilinear"),
            WarpResampleAlg::Bilinear
        );
        assert_eq!(
            WarpResampleAlg::parse("bilinear"),
            WarpResampleAlg::Bilinear
        );
        assert_eq!(WarpResampleAlg::parse("Cubic"), WarpResampleAlg::Cubic);
        assert_eq!(
            WarpResampleAlg::parse("NearestNeighbour"),
            WarpResampleAlg::NearestNeighbour
        );
        // Unknown spellings fall back to GDAL's own default.
        assert_eq!(
            WarpResampleAlg::parse("Nonesuch"),
            WarpResampleAlg::NearestNeighbour
        );
    }

    #[test]
    fn test_kernel_honesty() {
        assert_eq!(WarpResampleAlg::Bilinear.kernel(), WarpKernel::Bilinear);
        assert_eq!(
            WarpResampleAlg::NearestNeighbour.kernel(),
            WarpKernel::Nearest
        );
        assert!(WarpResampleAlg::Bilinear.is_kernel_exact());
        assert!(WarpResampleAlg::NearestNeighbour.is_kernel_exact());
        // Wider kernels are approximated, and say so.
        assert_eq!(WarpResampleAlg::Lanczos.kernel(), WarpKernel::Bilinear);
        assert!(!WarpResampleAlg::Lanczos.is_kernel_exact());
        assert!(!WarpResampleAlg::Cubic.is_kernel_exact());
    }

    #[test]
    fn test_init_dest() {
        let mut opts = WarpOptions::default();
        assert_eq!(opts.init_dest(), InitDest::None);

        opts.options
            .push(("INIT_DEST".to_string(), "NO_DATA".to_string()));
        assert_eq!(opts.init_dest(), InitDest::NoData);

        opts.options.clear();
        opts.options
            .push(("INIT_DEST".to_string(), "12.5".to_string()));
        assert_eq!(opts.init_dest(), InitDest::Value(12.5));
    }

    #[test]
    fn test_source_band_fallback() {
        let mut opts = WarpOptions::default();
        // No BandList: identity mapping.
        assert_eq!(opts.source_band_for(2), 2);

        opts.band_mappings.push(WarpBandMapping {
            src: 3,
            dst: 1,
            src_nodata_real: Some(0.0),
            dst_nodata_real: Some(0.0),
        });
        assert_eq!(opts.source_band_for(1), 3);
        assert_eq!(opts.source_band_for(2), 2);
    }

    #[test]
    fn test_validate_requires_source() {
        let opts = WarpOptions::default();
        assert!(opts.validate().is_err());
    }
}
