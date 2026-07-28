//! Fuzz target: NTv2 (`.gsb`) binary datum grid-shift parser and bilinear
//! interpolator.
//!
//! `NtV2Grid::from_bytes` reads the 176-byte OREC overview header, then per
//! sub-grid a 176-byte SREC header followed by `GS_COUNT` 16-byte shift
//! records - all attacker-controlled binary fields, including counts that
//! directly size `Vec` allocations. Any `Err` is acceptable; panics are not.
//!
//! `NtV2Grid::transform` is then exercised at a handful of coordinates
//! (including ones derived from the file's own declared bounds) to reach
//! the sub-grid-selection and bilinear-interpolation code without needing a
//! real-world coordinate as a seed.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_proj::NtV2Grid;

fuzz_target!(|data: &[u8]| {
    if let Ok(grid) = NtV2Grid::from_bytes(data) {
        for &(lon, lat) in &[
            (0.0_f64, 0.0_f64),
            (-123.0, 49.0),
            (151.0, -33.0),
            (f64::MIN_POSITIVE, f64::MIN_POSITIVE),
        ] {
            let _ = grid.transform(lon, lat);
        }

        // Coordinates derived from the first sub-grid's own declared bounds
        // (converted from positive-west arc-seconds back to positive-east
        // decimal degrees) are far more likely to land inside the grid and
        // exercise the bilinear-interpolation path than the fixed probes
        // above.
        if let Some(sg) = grid.sub_grids.first() {
            let lon_deg = -((sg.east_lon + sg.west_lon) / 2.0) / 3600.0;
            let lat_deg = (sg.south_lat + sg.north_lat) / 2.0 / 3600.0;
            let _ = grid.transform(lon_deg, lat_deg);
        }
    }
});
