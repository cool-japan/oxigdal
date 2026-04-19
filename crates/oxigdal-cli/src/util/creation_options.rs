//! Creation options parsing for GDAL-compatible KEY=VALUE output flags

/// Parse a KEY=VALUE string into a (key, value) tuple.
/// Used as a clap `value_parser`.
pub fn parse_key_value(s: &str) -> std::result::Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=VALUE: no '=' in '{s}'"))?;
    let key = s[..pos].to_string();
    let value = s[pos + 1..].to_string();
    if key.is_empty() {
        return Err(format!("empty key in '{s}'"));
    }
    Ok((key, value))
}

/// Mapped creation options after interpreting well-known GDAL keys.
pub struct MappedOptions {
    /// LZW compression requested (`COMPRESS=LZW`).
    pub compress_lzw: bool,
    /// Deflate compression requested (`COMPRESS=DEFLATE`).
    pub compress_deflate: bool,
    /// Tiled output requested (`TILED=YES`).
    pub tiled: bool,
    /// BigTIFF output requested (`BIGTIFF=YES`).
    pub bigtiff: bool,
    /// Key=value pairs that were not recognised.
    pub unknown_pairs: Vec<(String, String)>,
}

/// Interpret a slice of (key, value) pairs into structured options.
/// Unknown keys are forwarded in `unknown_pairs` and warned about.
pub fn map_creation_options(pairs: &[(String, String)]) -> MappedOptions {
    let mut out = MappedOptions {
        compress_lzw: false,
        compress_deflate: false,
        tiled: false,
        bigtiff: false,
        unknown_pairs: Vec::new(),
    };
    for (k, v) in pairs {
        match k.to_uppercase().as_str() {
            "COMPRESS" => match v.to_uppercase().as_str() {
                "LZW" => out.compress_lzw = true,
                "DEFLATE" => out.compress_deflate = true,
                _ => {
                    tracing::warn!("Unknown COMPRESS value: {v}");
                    out.unknown_pairs.push((k.clone(), v.clone()));
                }
            },
            "TILED" if v.to_uppercase() == "YES" => out.tiled = true,
            "BIGTIFF" if v.to_uppercase() == "YES" => out.bigtiff = true,
            _ => {
                tracing::warn!("Unknown creation option: {k}={v}");
                out.unknown_pairs.push((k.clone(), v.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_co_multiple_flags() {
        let result = parse_key_value("COMPRESS=LZW");
        assert!(result.is_ok());
        let (k, v) = result.expect("parse should succeed");
        assert_eq!(k, "COMPRESS");
        assert_eq!(v, "LZW");
    }

    #[test]
    fn test_co_invalid_no_equals_errors() {
        let result = parse_key_value("BADFLAG");
        assert!(result.is_err());
    }

    #[test]
    fn test_co_empty_key_errors() {
        let result = parse_key_value("=VALUE");
        assert!(result.is_err());
    }

    #[test]
    fn test_map_compress_lzw() {
        let pairs = vec![("COMPRESS".to_string(), "LZW".to_string())];
        let mapped = map_creation_options(&pairs);
        assert!(mapped.compress_lzw);
        assert!(!mapped.compress_deflate);
        assert!(mapped.unknown_pairs.is_empty());
    }

    #[test]
    fn test_map_compress_deflate() {
        let pairs = vec![("COMPRESS".to_string(), "DEFLATE".to_string())];
        let mapped = map_creation_options(&pairs);
        assert!(!mapped.compress_lzw);
        assert!(mapped.compress_deflate);
    }

    #[test]
    fn test_map_tiled_yes() {
        let pairs = vec![("TILED".to_string(), "YES".to_string())];
        let mapped = map_creation_options(&pairs);
        assert!(mapped.tiled);
    }

    #[test]
    fn test_map_bigtiff_yes() {
        let pairs = vec![("BIGTIFF".to_string(), "YES".to_string())];
        let mapped = map_creation_options(&pairs);
        assert!(mapped.bigtiff);
    }

    #[test]
    fn test_map_unknown_forwarded() {
        let pairs = vec![("UNKNOWN_KEY".to_string(), "some_value".to_string())];
        let mapped = map_creation_options(&pairs);
        assert_eq!(mapped.unknown_pairs.len(), 1);
    }
}
