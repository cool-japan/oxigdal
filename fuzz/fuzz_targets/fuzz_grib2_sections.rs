//! Fuzz target: GRIB2 section parser.
//!
//! Tests that GRIB2 message parsing never panics on arbitrary input.
//! Tries several discipline values to exercise more code paths.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Try several GRIB2 discipline values (WMO Table 0.0):
    // 0=Meteorological, 1=Hydrological, 2=Land Surface, 10=Oceanographic, 20=Climate
    for disc in [0u8, 1, 2, 10, 20] {
        let _ = oxigdal_grib::grib2::Grib2Message::from_bytes(data, disc);
    }
});
