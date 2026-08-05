//! Resolving the CRS strings a VRT carries into [`oxigeo_proj::Crs`].
//!
//! A warped VRT spells its `<SourceSRS>`/`<TargetSRS>` however the writing GDAL
//! felt like: a full WKT1 tree, an `EPSG:3857` shorthand, or a bare PROJ.4
//! string. This module normalises all three.
//!
//! The WKT path deliberately resolves through the CRS's **root** authority code
//! rather than through WKT-to-PROJ translation. Real GDAL WKT nests several
//! `AUTHORITY["EPSG",…]` nodes — the spheroid (7030), the datum (6326), the
//! prime meridian (8901), the unit (9122) — and only the outermost one names
//! the CRS itself. Scanning for the first match therefore yields the *spheroid*
//! code, and the crate's general WKT-to-PROJ conversion does not yet accept the
//! `EXTENSION[…]`/`AXIS[…]` nodes GDAL emits, so both of the obvious routes
//! resolve a real warped VRT to the wrong CRS or to no CRS at all
//! (cool-japan/oxigeo#15).

use crate::error::{Result, VrtError};
use oxigeo_proj::Crs;

/// Resolves an SRS string from a VRT into a CRS.
///
/// Accepts, in order of preference: an `EPSG:<code>` (or bare numeric)
/// shorthand, a PROJ.4 string, a WKT tree carrying a root `AUTHORITY`/`ID`
/// EPSG code, a WKT tree carrying an `EXTENSION["PROJ4", …]` node, and finally
/// the WKT itself.
///
/// # Errors
/// Returns an error when the string is empty or names no resolvable CRS.
pub fn resolve_crs(srs: &str) -> Result<Crs> {
    let trimmed = srs.trim();
    if trimmed.is_empty() {
        return Err(VrtError::invalid_structure("Empty SRS definition"));
    }

    if let Some(code) = parse_epsg_shorthand(trimmed) {
        return Crs::from_epsg(code).map_err(|e| srs_error(trimmed, e));
    }

    if trimmed.starts_with('+') {
        return Crs::from_proj(trimmed).map_err(|e| srs_error(trimmed, e));
    }

    if let Some(code) = root_authority_epsg(trimmed)
        && let Ok(crs) = Crs::from_epsg(code)
    {
        return Ok(crs);
    }

    if let Some(proj4) = extension_proj4(trimmed)
        && let Ok(crs) = Crs::from_proj(&proj4)
    {
        return Ok(crs);
    }

    Crs::from_wkt(trimmed).map_err(|e| srs_error(trimmed, e))
}

fn srs_error(srs: &str, err: oxigeo_proj::Error) -> VrtError {
    let head: String = srs.chars().take(80).collect();
    VrtError::invalid_structure(format!("Unresolvable SRS '{}…': {}", head, err))
}

/// `EPSG:4326`, `epsg:4326`, or a bare `4326`.
fn parse_epsg_shorthand(s: &str) -> Option<u32> {
    let rest = if let Some(stripped) = s.strip_prefix("EPSG:").or_else(|| s.strip_prefix("epsg:")) {
        stripped
    } else if s.chars().all(|c| c.is_ascii_digit()) {
        s
    } else {
        return None;
    };
    rest.trim().parse::<u32>().ok()
}

/// The EPSG code of a WKT tree's **root** node.
///
/// Bracket depth is tracked so only an `AUTHORITY[`/`ID[` that is a direct
/// child of the outermost node counts; quoted strings are skipped so a bracket
/// inside a name or a PROJ.4 extension cannot desynchronise the depth.
fn root_authority_epsg(wkt: &str) -> Option<u32> {
    let bytes = wkt.as_bytes();
    let mut depth = 0usize;
    let mut in_quotes = false;
    let mut found = None;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];

        if in_quotes {
            if b == b'"' {
                in_quotes = false;
            }
            i += 1;
            continue;
        }

        match b {
            b'"' => in_quotes = true,
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth = depth.saturating_sub(1),
            _ => {
                // A keyword can only start at depth 1 (a direct child of the
                // root node) and must be preceded by a separator.
                if depth == 1
                    && (i == 0 || matches!(bytes[i - 1], b'[' | b',' | b' ' | b'('))
                    && let Some(code) = authority_at(wkt, i)
                {
                    found = Some(code);
                }
            }
        }

        i += 1;
    }

    found
}

/// Parses `AUTHORITY["EPSG","<code>"]` or `ID["EPSG",<code>]` starting at
/// `offset`, if one begins there.
fn authority_at(wkt: &str, offset: usize) -> Option<u32> {
    let rest = wkt.get(offset..)?;

    let body = rest
        .strip_prefix("AUTHORITY[")
        .or_else(|| rest.strip_prefix("ID["))?;

    let body = body.trim_start();
    let body = body
        .strip_prefix("\"EPSG\"")
        .or_else(|| body.strip_prefix("\"epsg\""))?;
    let body = body.trim_start().strip_prefix(',')?.trim_start();
    let body = body.strip_prefix('"').unwrap_or(body);

    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// The PROJ.4 string of a WKT `EXTENSION["PROJ4","…"]` node, if present.
fn extension_proj4(wkt: &str) -> Option<String> {
    let idx = wkt.find("EXTENSION[\"PROJ4\",\"")?;
    let after = &wkt[idx + "EXTENSION[\"PROJ4\",\"".len()..];
    let end = after.find('"')?;
    let proj4 = after[..end].trim();
    if proj4.is_empty() {
        None
    } else {
        Some(proj4.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact strings from the VRT attached to cool-japan/oxigeo#15.
    const ISSUE_15_SOURCE_SRS: &str = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AXIS["Latitude",NORTH],AXIS["Longitude",EAST],AUTHORITY["EPSG","4326"]]"#;

    const ISSUE_15_TARGET_SRS: &str = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],EXTENSION["PROJ4","+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs"],AUTHORITY["EPSG","3857"]]"#;

    #[test]
    fn test_root_authority_skips_nested_codes() {
        // The first AUTHORITY in each string is the spheroid's (7030); the CRS
        // code is the outermost one.
        assert_eq!(root_authority_epsg(ISSUE_15_SOURCE_SRS), Some(4326));
        assert_eq!(root_authority_epsg(ISSUE_15_TARGET_SRS), Some(3857));
    }

    #[test]
    fn test_resolve_issue_15_wkt() {
        let src = resolve_crs(ISSUE_15_SOURCE_SRS).expect("source SRS");
        let dst = resolve_crs(ISSUE_15_TARGET_SRS).expect("target SRS");
        assert_eq!(src.epsg_code(), Some(4326));
        assert_eq!(dst.epsg_code(), Some(3857));
    }

    #[test]
    fn test_resolve_shorthand_and_proj() {
        assert_eq!(
            resolve_crs("EPSG:4326").expect("epsg").epsg_code(),
            Some(4326)
        );
        assert_eq!(resolve_crs("3857").expect("bare").epsg_code(), Some(3857));
        assert!(resolve_crs("+proj=merc +datum=WGS84 +units=m +no_defs").is_ok());
    }

    #[test]
    fn test_resolve_rejects_empty() {
        assert!(resolve_crs("   ").is_err());
    }

    #[test]
    fn test_extension_proj4() {
        let proj4 = extension_proj4(ISSUE_15_TARGET_SRS).expect("proj4 extension");
        assert!(proj4.starts_with("+proj=merc"));
        assert!(extension_proj4(ISSUE_15_SOURCE_SRS).is_none());
    }

    #[test]
    fn test_wkt2_id_form() {
        let wkt = r#"PROJCRS["Some CRS",BASEGEOGCRS["WGS 84",ID["EPSG",4326]],ID["EPSG",32633]]"#;
        assert_eq!(root_authority_epsg(wkt), Some(32633));
    }
}
