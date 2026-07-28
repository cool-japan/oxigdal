//! Known published Bursa-Wolf datum-shift presets used by
//! [`crate::transform::Transformer::transform_3d`]'s general (non-compound,
//! non-ITRF) 3-D fallback to compute a height-consistent horizontal datum
//! shift for a small set of named datums.

use crate::datum_transform::{BursaWolfParams, Ellipsoid};

/// A known 7-parameter Helmert datum-shift preset, together with the
/// source/target reference ellipsoids the preset's ECEF rotation and
/// translation were fitted against.
pub(crate) struct HorizontalDatumShift {
    pub(crate) params: BursaWolfParams,
    pub(crate) source_ellipsoid: Ellipsoid,
    pub(crate) target_ellipsoid: Ellipsoid,
}

/// Looks up a known, published Bursa-Wolf datum-shift preset for the given
/// `(source_datum, target_datum)` name pair (case-insensitive), matching
/// either the forward ("named datum → WGS84") or inverse ("WGS84 → named
/// datum") direction.
///
/// This intentionally covers only the small set of named datums this crate
/// already ships hard-coded EPSG transformation parameters for
/// ([`BursaWolfParams::nad27_to_wgs84_conus`], `ed50_to_wgs84`,
/// `tokyo_to_wgs84`, `osgb36_to_wgs84`). Unknown or unrecognised datum name
/// pairs return `None`, in which case
/// [`crate::transform::Transformer::transform_3d`] falls back to a documented
/// height-preserving passthrough.
pub(crate) fn known_horizontal_datum_shift(
    source_datum: &str,
    target_datum: &str,
) -> Option<HorizontalDatumShift> {
    /// `(non-WGS84 datum name as it appears in the EPSG registry `datum` field,
    /// that datum's reference ellipsoid, the "named datum → WGS84" preset
    /// constructor)`.
    type DatumPreset = (&'static str, Ellipsoid, fn() -> BursaWolfParams);

    let src = source_datum.to_ascii_uppercase();
    let tgt = target_datum.to_ascii_uppercase();

    let presets: [DatumPreset; 4] = [
        (
            "NAD27",
            Ellipsoid::CLARKE1866,
            BursaWolfParams::nad27_to_wgs84_conus,
        ),
        (
            "ED50",
            Ellipsoid::INTERNATIONAL,
            BursaWolfParams::ed50_to_wgs84,
        ),
        ("TOKYO", Ellipsoid::BESSEL, BursaWolfParams::tokyo_to_wgs84),
        ("OSGB36", Ellipsoid::AIRY, BursaWolfParams::osgb36_to_wgs84),
    ];

    for (name, ellipsoid, preset_fn) in presets {
        if src == name && tgt == "WGS84" {
            return Some(HorizontalDatumShift {
                params: preset_fn(),
                source_ellipsoid: ellipsoid,
                target_ellipsoid: Ellipsoid::WGS84,
            });
        }
        if tgt == name && src == "WGS84" {
            return Some(HorizontalDatumShift {
                params: preset_fn().inverse(),
                source_ellipsoid: Ellipsoid::WGS84,
                target_ellipsoid: ellipsoid,
            });
        }
    }

    None
}
