//! Self-contained UTM ↔ WGS84 conversion (Krüger series, order 6).
//!
//! Covers `UtmZone::from_epsg` for EPSG 326xx/327xx with the standard
//! UTM parameters (k0 = 0.9996, FE = 500 000 m, FN = 10 000 000 m south).
//! Deliberately independent of `oxigdal-proj` to keep the wasm bundle
//! free of the bundled EPSG database.
//!
//! The transverse-Mercator core is the Gauss–Krüger *n*-series carried to
//! order 6 (≈ n⁶), the same formulation used by Karney (2011) and the
//! reference derivation on the "Transverse Mercator projection" article.
//! Accuracy is sub-millimetre anywhere inside a nominal ±3° UTM zone and
//! stays well below the parity tolerance out to several degrees of the
//! central meridian.
//!
//! Implemented by WP A2 (GeoSentinel lane); stub registered by WP W0.

/// WGS84 semi-major axis (metres).
const WGS84_A: f64 = 6_378_137.0;
/// WGS84 inverse flattening.
const WGS84_INV_F: f64 = 298.257_223_563;
/// UTM scale factor on the central meridian.
const K0: f64 = 0.9996;
/// UTM false easting (metres).
const FALSE_EASTING: f64 = 500_000.0;
/// UTM false northing for southern-hemisphere zones (metres).
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

/// A UTM zone: numeric zone `1..=60` plus hemisphere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtmZone {
    /// Zone number, `1..=60`.
    pub zone: u8,
    /// `true` for a northern-hemisphere zone (EPSG 326xx),
    /// `false` for a southern-hemisphere zone (EPSG 327xx).
    pub north: bool,
}

impl UtmZone {
    /// Build a zone from an EPSG code.
    ///
    /// Accepts `32601..=32660` (northern, WGS84 / UTM zone N) and
    /// `32701..=32760` (southern, WGS84 / UTM zone S). Any other code —
    /// including zone `00` or `61` — yields `None`.
    #[must_use]
    pub fn from_epsg(epsg: u32) -> Option<Self> {
        let (north, zone) = match epsg {
            32601..=32660 => (true, (epsg - 32600) as u8),
            32701..=32760 => (false, (epsg - 32700) as u8),
            _ => return None,
        };
        Some(Self { zone, north })
    }

    /// The EPSG code corresponding to this zone.
    #[must_use]
    pub fn epsg(&self) -> u32 {
        let base = if self.north { 32600 } else { 32700 };
        base + u32::from(self.zone)
    }

    /// Central-meridian longitude of the zone, in degrees.
    #[must_use]
    pub fn central_meridian_deg(&self) -> f64 {
        // Zone 1 → −177°, each zone is 6° wide.
        -183.0 + 6.0 * f64::from(self.zone)
    }

    /// False northing appropriate to the hemisphere (metres).
    #[must_use]
    fn false_northing(&self) -> f64 {
        if self.north {
            0.0
        } else {
            FALSE_NORTHING_SOUTH
        }
    }
}

/// Pre-computed ellipsoid/series constants for the Krüger order-6 expansion.
///
/// All of these depend only on the WGS84 flattening, so they could be `const`
/// in a `const fn` world; computing them once per call is negligible next to
/// the six `sin`/`cosh` evaluations that follow.
struct KrugerConstants {
    /// First eccentricity.
    e: f64,
    /// Rectifying radius `A` (metres) — the mean meridional radius.
    rectifying_a: f64,
    /// Forward coefficients α₁..α₆ (geographic → grid), indexed 0..5.
    alpha: [f64; 6],
    /// Inverse coefficients β₁..β₆ (grid → conformal), indexed 0..5.
    beta: [f64; 6],
    /// Conformal → geographic latitude coefficients δ₁..δ₆, indexed 0..5.
    delta: [f64; 6],
}

impl KrugerConstants {
    fn wgs84() -> Self {
        let f = 1.0 / WGS84_INV_F;
        let e = (f * (2.0 - f)).sqrt();
        let n = f / (2.0 - f);
        let n2 = n * n;
        let n3 = n2 * n;
        let n4 = n3 * n;
        let n5 = n4 * n;
        let n6 = n5 * n;

        // Rectifying radius A = a/(1+n) · (1 + n²/4 + n⁴/64 + n⁶/256 + …).
        let rectifying_a = WGS84_A / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);

        let alpha = [
            n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
                + 7891.0 * n6 / 37800.0,
            13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
                - 1_983_433.0 * n6 / 1_935_360.0,
            61.0 * n3 / 240.0 - 103.0 * n4 / 140.0
                + 15_061.0 * n5 / 26_880.0
                + 167_603.0 * n6 / 181_440.0,
            49_561.0 * n4 / 161_280.0 - 179.0 * n5 / 168.0 + 6_601_661.0 * n6 / 7_257_600.0,
            34_729.0 * n5 / 80_640.0 - 3_418_889.0 * n6 / 1_995_840.0,
            212_378_941.0 * n6 / 319_334_400.0,
        ];

        let beta = [
            n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0 - 81.0 * n5 / 512.0
                + 96_199.0 * n6 / 604_800.0,
            n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0 + 46.0 * n5 / 105.0
                - 1_118_711.0 * n6 / 3_870_720.0,
            17.0 * n3 / 480.0 - 37.0 * n4 / 840.0 - 209.0 * n5 / 4480.0 + 5569.0 * n6 / 90_720.0,
            4397.0 * n4 / 161_280.0 - 11.0 * n5 / 504.0 - 830_251.0 * n6 / 7_257_600.0,
            4583.0 * n5 / 161_280.0 - 108_847.0 * n6 / 3_991_680.0,
            20_648_693.0 * n6 / 638_668_800.0,
        ];

        let delta = [
            2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0 + 26.0 * n5 / 45.0
                - 2854.0 * n6 / 675.0,
            7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0
                + 2704.0 * n5 / 315.0
                + 2323.0 * n6 / 945.0,
            56.0 * n3 / 15.0 - 136.0 * n4 / 35.0 - 1262.0 * n5 / 105.0 + 73_814.0 * n6 / 2835.0,
            4279.0 * n4 / 630.0 - 332.0 * n5 / 35.0 - 399_572.0 * n6 / 14_175.0,
            4174.0 * n5 / 315.0 - 144_838.0 * n6 / 6237.0,
            601_676.0 * n6 / 22_275.0,
        ];

        Self {
            e,
            rectifying_a,
            alpha,
            beta,
            delta,
        }
    }
}

/// Forward transverse-Mercator: geographic (`lon`, `lat` degrees) → UTM.
///
/// Returns `(easting, northing)` in metres for the given `zone`.
#[must_use]
pub fn wgs84_to_utm(zone: &UtmZone, lon: f64, lat: f64) -> (f64, f64) {
    let k = KrugerConstants::wgs84();
    let lat_rad = lat.to_radians();
    let dlon = (lon - zone.central_meridian_deg()).to_radians();

    let sin_phi = lat_rad.sin();
    // Isometric latitude ψ; t = tan(conformal latitude) = sinh ψ.
    let psi = sin_phi.atanh() - k.e * (k.e * sin_phi).atanh();
    let t = psi.sinh();

    let xi_prime = t.atan2(dlon.cos());
    let eta_prime = (dlon.sin() / (1.0 + t * t).sqrt()).atanh();

    let mut xi = xi_prime;
    let mut eta = eta_prime;
    for (j, &a) in k.alpha.iter().enumerate() {
        let m = 2.0 * (j as f64 + 1.0);
        xi += a * (m * xi_prime).sin() * (m * eta_prime).cosh();
        eta += a * (m * xi_prime).cos() * (m * eta_prime).sinh();
    }

    let easting = FALSE_EASTING + K0 * k.rectifying_a * eta;
    let northing = zone.false_northing() + K0 * k.rectifying_a * xi;
    (easting, northing)
}

/// Inverse transverse-Mercator: UTM (`easting`, `northing` metres) → geographic.
///
/// Returns `(lon, lat)` in degrees.
#[must_use]
pub fn utm_to_wgs84(zone: &UtmZone, easting: f64, northing: f64) -> (f64, f64) {
    let k = KrugerConstants::wgs84();
    let denom = K0 * k.rectifying_a;

    let xi = (northing - zone.false_northing()) / denom;
    let eta = (easting - FALSE_EASTING) / denom;

    let mut xi_prime = xi;
    let mut eta_prime = eta;
    for (j, &b) in k.beta.iter().enumerate() {
        let m = 2.0 * (j as f64 + 1.0);
        xi_prime -= b * (m * xi).sin() * (m * eta).cosh();
        eta_prime -= b * (m * xi).cos() * (m * eta).sinh();
    }

    // Conformal latitude χ, then Krüger δ-series to geographic latitude.
    let chi = (xi_prime.sin() / eta_prime.cosh()).clamp(-1.0, 1.0).asin();
    let mut lat_rad = chi;
    for (j, &d) in k.delta.iter().enumerate() {
        let m = 2.0 * (j as f64 + 1.0);
        lat_rad += d * (m * chi).sin();
    }

    let dlon = eta_prime.sinh().atan2(xi_prime.cos());
    let lon = zone.central_meridian_deg() + dlon.to_degrees();
    let lat = lat_rad.to_degrees();
    (normalize_lon(lon), lat)
}

/// Wrap a longitude into `[-180, 180)`.
fn normalize_lon(lon: f64) -> f64 {
    let mut v = (lon + 180.0) % 360.0;
    if v < 0.0 {
        v += 360.0;
    }
    v - 180.0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Reference easting/northing produced by PROJ 9.7.0
    /// (`cs2cs EPSG:4326 EPSG:326xx/327xx`, lat/lon input) for a point in
    /// each named zone. These are the parity anchors required by the A2
    /// contract: 54N Tokyo, 20S Rondônia, 4Q (zone 4N) Hawaii, 53N Noto.
    struct ParityCase {
        name: &'static str,
        epsg: u32,
        lat: f64,
        lon: f64,
        easting: f64,
        northing: f64,
    }

    const PARITY: &[ParityCase] = &[
        ParityCase {
            name: "Tokyo 54N",
            epsg: 32654,
            lat: 35.681236,
            lon: 139.767125,
            easting: 388_435.687137,
            northing: 3_949_293.978149,
        },
        ParityCase {
            name: "Rondonia 20S",
            epsg: 32720,
            lat: -8.9,
            lon: -63.2,
            easting: 478_011.155970,
            northing: 9_016_197.576696,
        },
        ParityCase {
            name: "Hawaii 4Q (4N)",
            epsg: 32604,
            lat: 20.87,
            lon: -156.65,
            easting: 744_495.061056,
            northing: 2_309_547.224292,
        },
        ParityCase {
            name: "Noto 53N",
            epsg: 32653,
            lat: 37.4,
            lon: 136.9,
            easting: 668_173.007075,
            northing: 4_140_941.141124,
        },
    ];

    #[test]
    fn from_epsg_maps_hemisphere_and_zone() {
        let north = UtmZone::from_epsg(32654).expect("valid north code");
        assert_eq!(
            north,
            UtmZone {
                zone: 54,
                north: true
            }
        );
        assert_eq!(north.central_meridian_deg(), 141.0);
        assert_eq!(north.epsg(), 32654);

        let south = UtmZone::from_epsg(32720).expect("valid south code");
        assert_eq!(
            south,
            UtmZone {
                zone: 20,
                north: false
            }
        );
        assert_eq!(south.central_meridian_deg(), -63.0);
        assert_eq!(south.epsg(), 32720);

        // Zone-1 boundaries.
        assert_eq!(
            UtmZone::from_epsg(32601).map(|z| z.central_meridian_deg()),
            Some(-177.0)
        );
        assert_eq!(
            UtmZone::from_epsg(32760).map(|z| z.central_meridian_deg()),
            Some(177.0)
        );
    }

    #[test]
    fn from_epsg_rejects_out_of_range() {
        assert_eq!(UtmZone::from_epsg(32600), None); // zone 00
        assert_eq!(UtmZone::from_epsg(32661), None); // zone 61 north
        assert_eq!(UtmZone::from_epsg(32700), None); // zone 00 south
        assert_eq!(UtmZone::from_epsg(32761), None); // zone 61 south
        assert_eq!(UtmZone::from_epsg(4326), None); // not UTM
        assert_eq!(UtmZone::from_epsg(3857), None);
    }

    #[test]
    fn forward_matches_proj_reference() {
        for case in PARITY {
            let zone = UtmZone::from_epsg(case.epsg).expect("known epsg");
            let (e, n) = wgs84_to_utm(&zone, case.lon, case.lat);
            assert!(
                (e - case.easting).abs() < 1e-3,
                "{}: easting {e} vs {} (Δ {})",
                case.name,
                case.easting,
                e - case.easting
            );
            assert!(
                (n - case.northing).abs() < 1e-3,
                "{}: northing {n} vs {} (Δ {})",
                case.name,
                case.northing,
                n - case.northing
            );
        }
    }

    #[test]
    fn inverse_matches_proj_reference() {
        for case in PARITY {
            let zone = UtmZone::from_epsg(case.epsg).expect("known epsg");
            let (lon, lat) = utm_to_wgs84(&zone, case.easting, case.northing);
            assert!(
                (lon - case.lon).abs() < 1e-8,
                "{}: lon {lon} vs {} (Δ {})",
                case.name,
                case.lon,
                lon - case.lon
            );
            assert!(
                (lat - case.lat).abs() < 1e-8,
                "{}: lat {lat} vs {} (Δ {})",
                case.name,
                case.lat,
                lat - case.lat
            );
        }
    }

    #[test]
    fn round_trip_forward_inverse() {
        // A spread of latitudes/longitudes across several zones, including
        // points a few degrees off the central meridian.
        let samples = [
            (32654, 139.767125, 35.681236),
            (32720, -63.2, -8.9),
            (32604, -156.65, 20.87),
            (32653, 136.9, 37.4),
            (32631, 2.35, 48.85),    // Paris, near CM 3°
            (32633, 13.4, 52.52),    // Berlin
            (32718, -77.03, -12.05), // Lima, southern
            (32643, 55.27, 25.2),    // Dubai
        ];
        for (epsg, lon, lat) in samples {
            let zone = UtmZone::from_epsg(epsg).expect("known epsg");
            let (e, n) = wgs84_to_utm(&zone, lon, lat);
            let (lon2, lat2) = utm_to_wgs84(&zone, e, n);
            assert!(
                (lon2 - lon).abs() < 1e-9,
                "epsg {epsg}: lon round-trip {lon} -> {lon2}"
            );
            assert!(
                (lat2 - lat).abs() < 1e-9,
                "epsg {epsg}: lat round-trip {lat} -> {lat2}"
            );
        }
    }

    #[test]
    fn round_trip_at_zone_extremes() {
        // Push toward the ±3° zone edge and high/low latitudes where the
        // series is most stressed; round-trip must still close to < 1e-9°.
        let zone = UtmZone::from_epsg(32633).expect("zone 33N");
        let cm = zone.central_meridian_deg();
        for &dlon in &[-3.0, -1.5, 0.0, 1.5, 3.0] {
            for &lat in &[0.0, 15.0, 45.0, 70.0, 83.0] {
                let lon = cm + dlon;
                let (e, n) = wgs84_to_utm(&zone, lon, lat);
                let (lon2, lat2) = utm_to_wgs84(&zone, e, n);
                assert!(
                    (lon2 - lon).abs() < 1e-9 && (lat2 - lat).abs() < 1e-9,
                    "dlon {dlon}, lat {lat}: ({lon},{lat}) -> ({lon2},{lat2})"
                );
            }
        }
    }

    #[test]
    fn central_meridian_has_false_easting() {
        // On the central meridian easting is exactly the false easting.
        let zone = UtmZone::from_epsg(32633).expect("zone 33N");
        let (e, _n) = wgs84_to_utm(&zone, zone.central_meridian_deg(), 40.0);
        assert!((e - FALSE_EASTING).abs() < 1e-6, "easting on CM = {e}");
    }

    #[test]
    fn equator_northing_is_zero_north_and_ten_million_south() {
        let north = UtmZone::from_epsg(32633).expect("north");
        let (_e, n) = wgs84_to_utm(&north, north.central_meridian_deg(), 0.0);
        assert!(n.abs() < 1e-6, "north equator northing = {n}");

        let south = UtmZone::from_epsg(32733).expect("south");
        let (_e, ns) = wgs84_to_utm(&south, south.central_meridian_deg(), 0.0);
        assert!(
            (ns - FALSE_NORTHING_SOUTH).abs() < 1e-6,
            "south equator northing = {ns}"
        );
    }

    #[test]
    fn normalize_lon_wraps() {
        assert!((normalize_lon(181.0) - (-179.0)).abs() < 1e-12);
        assert!((normalize_lon(-181.0) - 179.0).abs() < 1e-12);
        assert!((normalize_lon(0.0)).abs() < 1e-12);
        assert!((normalize_lon(360.0) - 0.0).abs() < 1e-12);
    }
}
