//! WKT → PROJ-string conversion.
//!
//! Translates an OGC/ESRI WKT1 (or basic WKT2) CRS definition into a PROJ
//! string that the pure-Rust `proj4rs` backend can evaluate. This is what makes
//! a `Crs::from_wkt(..)` CRS — e.g. the `.prj` shipped with a shapefile — usable
//! by [`crate::transform::Transformer`].
//!
//! The conversion is best-effort but covers the common real-world cases:
//! geographic CRS (`GEOGCS`/`GEOGCRS`) and projected CRS (`PROJCS`/`PROJCRS`)
//! over the standard projection methods (Transverse Mercator, Lambert Conformal
//! Conic, Mercator, Albers, the azimuthal family, …), mapping the projection
//! parameters, ellipsoid, datum/`TOWGS84` shift, and linear unit.

use crate::error::{Error, Result};
use crate::wkt::{WktNode, parse_wkt};

/// Convert a WKT CRS definition to a PROJ string.
///
/// # Errors
///
/// Returns an error if the WKT cannot be parsed or describes a CRS kind that
/// has no PROJ-string equivalent here (e.g. a bare vertical or compound CRS).
pub fn wkt_to_proj_string(wkt: &str) -> Result<String> {
    let root = parse_wkt(wkt).map_err(|e| Error::invalid_wkt(format!("WKT parse failed: {e}")))?;
    let kind = root.node_type.to_uppercase();

    match kind.as_str() {
        "PROJCS" | "PROJCRS" => projected_to_proj(&root),
        "GEOGCS" | "GEOGCRS" | "GEODCRS" | "BASEGEOGCRS" => Ok(geographic_to_proj(&root)),
        other => Err(Error::unsupported_crs(format!(
            "WKT root '{other}' has no PROJ-string conversion (expected PROJCS/GEOGCS)"
        ))),
    }
}

/// Build the `+proj=longlat ...` string for a geographic CRS node.
fn geographic_to_proj(node: &WktNode) -> String {
    let mut parts = vec!["+proj=longlat".to_string()];
    push_datum_and_ellipsoid(node, &mut parts);
    parts.push("+no_defs".to_string());
    parts.join(" ")
}

/// Build the projected `+proj=<method> ...` string for a PROJCS/PROJCRS node.
fn projected_to_proj(root: &WktNode) -> Result<String> {
    // Projection method: WKT1 `PROJECTION["..."]`, WKT2 `METHOD["..."]`.
    let method = find_descendant(root, "PROJECTION")
        .or_else(|| find_descendant(root, "METHOD"))
        .and_then(|n| n.value.clone())
        .ok_or_else(|| Error::unsupported_crs("PROJCS without a PROJECTION/METHOD node"))?;

    let proj = proj_name_for_method(&method).ok_or_else(|| {
        Error::unsupported_crs(format!("Unsupported WKT projection method: '{method}'"))
    })?;

    // Collect every PARAMETER["name", value] into a case-insensitive lookup.
    let mut params = Vec::new();
    collect_descendants(root, "PARAMETER", &mut params);
    let lookup = |names: &[&str]| -> Option<f64> {
        for p in &params {
            if let Some(name) = &p.value {
                if names.iter().any(|n| name.eq_ignore_ascii_case(n)) {
                    return ordered_numbers(p).first().copied();
                }
            }
        }
        None
    };

    let mut parts = vec![format!("+proj={proj}")];

    // Latitude / longitude of origin.
    if let Some(v) = lookup(&[
        "latitude_of_origin",
        "latitude_of_center",
        "latitude_of_natural_origin",
        "latitude_of_false_origin",
        "central_parallel",
    ]) {
        parts.push(format!("+lat_0={}", fmt_num(v)));
    }
    if let Some(v) = lookup(&[
        "central_meridian",
        "longitude_of_center",
        "longitude_of_origin",
        "longitude_of_natural_origin",
        "longitude_of_false_origin",
    ]) {
        parts.push(format!("+lon_0={}", fmt_num(v)));
    }

    // Standard parallels (conics).
    if let Some(v) = lookup(&[
        "standard_parallel_1",
        "standard_parallel1",
        "latitude_of_1st_standard_parallel",
    ]) {
        parts.push(format!("+lat_1={}", fmt_num(v)));
    }
    if let Some(v) = lookup(&[
        "standard_parallel_2",
        "standard_parallel2",
        "latitude_of_2nd_standard_parallel",
    ]) {
        parts.push(format!("+lat_2={}", fmt_num(v)));
    }

    // Oblique-mercator angles.
    if let Some(v) = lookup(&["azimuth", "azimuth_of_initial_line"]) {
        parts.push(format!("+alpha={}", fmt_num(v)));
    }
    if let Some(v) = lookup(&["rectified_grid_angle"]) {
        parts.push(format!("+gamma={}", fmt_num(v)));
    }

    // Scale factor.
    if let Some(v) = lookup(&[
        "scale_factor",
        "scale_factor_at_natural_origin",
        "scale_factor_at_center",
    ]) {
        parts.push(format!("+k_0={}", fmt_num(v)));
    }

    // False easting / northing.
    if let Some(v) = lookup(&[
        "false_easting",
        "easting_at_false_origin",
        "easting_at_projection_centre",
    ]) {
        parts.push(format!("+x_0={}", fmt_num(v)));
    }
    if let Some(v) = lookup(&[
        "false_northing",
        "northing_at_false_origin",
        "northing_at_projection_centre",
    ]) {
        parts.push(format!("+y_0={}", fmt_num(v)));
    }

    push_datum_and_ellipsoid(root, &mut parts);
    push_linear_unit(root, &mut parts);
    parts.push("+no_defs".to_string());
    Ok(parts.join(" "))
}

/// Append ellipsoid + datum/`TOWGS84` tokens shared by geographic and projected CRS.
fn push_datum_and_ellipsoid(node: &WktNode, parts: &mut Vec<String>) {
    let spheroid = find_descendant(node, "SPHEROID").or_else(|| find_descendant(node, "ELLIPSOID"));
    let datum_name = find_descendant(node, "DATUM")
        .and_then(|n| n.value.clone())
        .unwrap_or_default();

    // An explicit TOWGS84 always wins.
    if let Some(towgs) = find_descendant(node, "TOWGS84") {
        let nums = ordered_numbers(towgs);
        if !nums.is_empty() {
            let joined = nums
                .iter()
                .map(|n| fmt_num(*n))
                .collect::<Vec<_>>()
                .join(",");
            push_ellipsoid(spheroid, parts);
            parts.push(format!("+towgs84={joined}"));
            return;
        }
    }

    // No TOWGS84: if this is plainly WGS 84, use the +datum shortcut (which also
    // sets the ellipsoid), otherwise emit the ellipsoid plus a null shift so the
    // backend can still build a transform.
    let ellps_name = spheroid.and_then(|n| n.value.clone()).unwrap_or_default();
    if is_wgs84(&datum_name) || is_wgs84(&ellps_name) {
        parts.push("+datum=WGS84".to_string());
    } else {
        push_ellipsoid(spheroid, parts);
        parts.push("+towgs84=0,0,0".to_string());
    }
}

/// Append `+ellps=` (known name) or explicit `+a=`/`+rf=`/`+b=` for a SPHEROID node.
fn push_ellipsoid(spheroid: Option<&WktNode>, parts: &mut Vec<String>) {
    let Some(node) = spheroid else {
        parts.push("+ellps=WGS84".to_string());
        return;
    };
    let name = node.value.clone().unwrap_or_default();
    if let Some(ellps) = proj_ellps_for_name(&name) {
        parts.push(format!("+ellps={ellps}"));
        return;
    }
    // Fall back to the numeric definition: [semi-major axis, inverse flattening].
    let nums = ordered_numbers(node);
    match (nums.first(), nums.get(1)) {
        (Some(&a), Some(&rf)) if rf > 0.0 => {
            parts.push(format!("+a={}", fmt_num(a)));
            parts.push(format!("+rf={}", fmt_num(rf)));
        }
        (Some(&a), _) => {
            // rf == 0 (or missing) denotes a sphere.
            parts.push(format!("+a={}", fmt_num(a)));
            parts.push(format!("+b={}", fmt_num(a)));
        }
        _ => parts.push("+ellps=WGS84".to_string()),
    }
}

/// Append `+units=`/`+to_meter=` for the linear unit of a projected CRS.
fn push_linear_unit(root: &WktNode, parts: &mut Vec<String>) {
    // The linear unit is a direct child UNIT/LENGTHUNIT of the PROJCS root.
    let unit = root.children.iter().find(|c| {
        c.node_type.eq_ignore_ascii_case("UNIT") || c.node_type.eq_ignore_ascii_case("LENGTHUNIT")
    });
    let Some(unit) = unit else {
        parts.push("+units=m".to_string());
        return;
    };
    let name = unit.value.clone().unwrap_or_default().to_lowercase();
    let factor = ordered_numbers(unit).first().copied().unwrap_or(1.0);

    if (factor - 1.0).abs() < 1e-9 {
        parts.push("+units=m".to_string());
    } else if name.contains("us") && name.contains("foot") {
        parts.push("+units=us-ft".to_string());
    } else if name.contains("foot") || name.contains("feet") {
        parts.push("+units=ft".to_string());
    } else {
        parts.push(format!("+to_meter={}", fmt_num(factor)));
    }
}

/// Map an OGC/ESRI projection-method name to its PROJ `+proj=` token.
fn proj_name_for_method(method: &str) -> Option<&'static str> {
    let key: String = method
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let name = match key.as_str() {
        "transversemercator"
        | "gausskruger"
        | "gaussboaga"
        | "transversemercatorsouthorientated" => "tmerc",
        "mercator"
        | "mercator1sp"
        | "mercator2sp"
        | "mercatorvarianta"
        | "mercatorvariantb"
        | "popularvisualisationpseudomercator" => "merc",
        "lambertconformalconic"
        | "lambertconformalconic1sp"
        | "lambertconformalconic2sp"
        | "lambertconicconformal1sp"
        | "lambertconicconformal2sp" => "lcc",
        "albers" | "albersconicequalarea" | "albersequalarea" => "aea",
        "lambertazimuthalequalarea" => "laea",
        "azimuthalequidistant" => "aeqd",
        "stereographic" | "obliquestereographic" | "doublestereographic" => "sterea",
        "polarstereographic" | "polarstereographicvarianta" | "polarstereographicvariantb" => {
            "stere"
        }
        "cassinisoldner" | "cassini" => "cass",
        "sinusoidal" => "sinu",
        "mollweide" => "moll",
        "robinson" => "robin",
        "orthographic" => "ortho",
        "gnomonic" => "gnom",
        "equirectangular" | "equidistantcylindrical" | "platecarree" => "eqc",
        "newzealandmapgrid" => "nzmg",
        "hotineobliquemercator"
        | "obliquemercator"
        | "hotineobliquemercatorvarianta"
        | "hotineobliquemercatorvariantb"
        | "rectifiedskewedorthomorphic" => "omerc",
        "vandergrinten" | "vandergrinteni" => "vandg",
        "polyconic" | "americanpolyconic" => "poly",
        "krovak" => "krovak",
        "eckertiv" => "eck4",
        "eckertvi" => "eck6",
        "miller" | "millercylindrical" => "mill",
        _ => return None,
    };
    Some(name)
}

/// Map a WKT spheroid/ellipsoid name to a PROJ `+ellps=` token, when well known.
fn proj_ellps_for_name(name: &str) -> Option<&'static str> {
    let key: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let ellps = match key.as_str() {
        "wgs84" | "wgs1984" => "WGS84",
        "grs1980" | "grs80" => "GRS80",
        "grs1967" | "grs1967modified" => "GRS67",
        "krasovsky1940" | "krassovsky1940" | "krassowsky1940" | "krasovsky" | "krass" => "krass",
        "bessel1841" | "bessel" => "bessel",
        "clarke1866" => "clrk66",
        "clarke1880" | "clarke1880rgs" | "clarke1880arc" => "clrk80",
        "clarke1880ign" => "clrk80ign",
        "international1924" | "internationalhayford" | "hayford1909" | "intl1924" => "intl",
        "airy1830" | "airy" => "airy",
        "airymodified1849" | "modifiedairy" => "mod_airy",
        "everest1830" | "everest18301937adjustment" => "evrst30",
        "gem10c" => "gem10c",
        "helmert1906" => "helmert",
        "australiannational" | "australiannationalspheroid" => "aust_SA",
        "war office" | "waroffice" => "WGS84",
        _ => return None,
    };
    Some(ellps)
}

/// Does this datum/ellipsoid name denote WGS 84?
fn is_wgs84(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("wgs") && (lower.contains("84") || lower.contains("1984"))
}

/// Recursively find the first descendant node of `node_type` (case-insensitive),
/// including `node` itself.
fn find_descendant<'a>(node: &'a WktNode, node_type: &str) -> Option<&'a WktNode> {
    if node.node_type.eq_ignore_ascii_case(node_type) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_descendant(child, node_type) {
            return Some(found);
        }
    }
    None
}

/// Recursively collect every descendant node of `node_type` (case-insensitive).
fn collect_descendants<'a>(node: &'a WktNode, node_type: &str, out: &mut Vec<&'a WktNode>) {
    if node.node_type.eq_ignore_ascii_case(node_type) {
        out.push(node);
    }
    for child in &node.children {
        collect_descendants(child, node_type, out);
    }
}

/// Extract the node's unnamed numeric arguments in source order.
///
/// The WKT parser stores positional (unnamed) values under `param_<byte-offset>`
/// keys, so sorting by that offset recovers the original argument order — e.g.
/// `SPHEROID["Krasovsky_1940",6378245.0,298.3]` → `[6378245.0, 298.3]`.
fn ordered_numbers(node: &WktNode) -> Vec<f64> {
    let mut items: Vec<(usize, f64)> = node
        .parameters
        .iter()
        .filter_map(|(k, v)| {
            let pos = k.strip_prefix("param_")?.parse::<usize>().ok()?;
            let num = v.parse::<f64>().ok()?;
            Some((pos, num))
        })
        .collect();
    items.sort_by_key(|(pos, _)| *pos);
    items.into_iter().map(|(_, n)| n).collect()
}

/// Format a number without a trailing `.0` for whole values (cleaner PROJ strings).
fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const LAO97_UTM48: &str = r#"PROJCS["Lao97_UTM_zone_48",GEOGCS["GCS_Lao_1997",DATUM["D_Lao_National_Datum_1997",SPHEROID["Krasovsky_1940",6378245.0,298.3]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",105.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#;

    #[test]
    fn lao97_utm48_converts() {
        let s = wkt_to_proj_string(LAO97_UTM48).unwrap();
        assert!(s.contains("+proj=tmerc"), "{s}");
        assert!(s.contains("+lon_0=105"), "{s}");
        assert!(s.contains("+lat_0=0"), "{s}");
        assert!(s.contains("+k_0=0.9996"), "{s}");
        assert!(s.contains("+x_0=500000"), "{s}");
        assert!(s.contains("+y_0=0"), "{s}");
        assert!(s.contains("+ellps=krass"), "{s}");
        assert!(s.contains("+units=m"), "{s}");
    }

    #[test]
    fn wgs84_utm_uses_datum_shortcut() {
        let wkt = r#"PROJCS["WGS 84 / UTM zone 48N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",105],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],UNIT["metre",1]]"#;
        let s = wkt_to_proj_string(wkt).unwrap();
        assert!(s.contains("+proj=tmerc"), "{s}");
        assert!(s.contains("+datum=WGS84"), "{s}");
    }

    #[test]
    fn lambert_conformal_conic_2sp() {
        let wkt = r#"PROJCS["test_lcc",GEOGCS["GCS_North_American_1983",DATUM["D_North_American_1983",SPHEROID["GRS_1980",6378137,298.257222101]],PRIMEM["Greenwich",0]],PROJECTION["Lambert_Conformal_Conic"],PARAMETER["standard_parallel_1",33],PARAMETER["standard_parallel_2",45],PARAMETER["latitude_of_origin",23],PARAMETER["central_meridian",-96],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["Meter",1]]"#;
        let s = wkt_to_proj_string(wkt).unwrap();
        assert!(s.contains("+proj=lcc"), "{s}");
        assert!(s.contains("+lat_1=33"), "{s}");
        assert!(s.contains("+lat_2=45"), "{s}");
        assert!(s.contains("+lon_0=-96"), "{s}");
        assert!(s.contains("+ellps=GRS80"), "{s}");
    }

    #[test]
    fn geographic_crs_longlat() {
        let wkt = r#"GEOGCS["GCS_Lao_1997",DATUM["D_Lao_National_Datum_1997",SPHEROID["Krasovsky_1940",6378245.0,298.3]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#;
        let s = wkt_to_proj_string(wkt).unwrap();
        assert!(s.contains("+proj=longlat"), "{s}");
        assert!(s.contains("+ellps=krass"), "{s}");
    }

    #[test]
    fn unknown_projection_errors() {
        let wkt = r#"PROJCS["x",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]],PROJECTION["Totally_Made_Up"],UNIT["Meter",1]]"#;
        assert!(wkt_to_proj_string(wkt).is_err());
    }
}
