//! PROJ string parser and builder.
//!
//! Supports parsing and constructing PROJ.4 / PROJ strings of the form:
//! `+proj=utm +zone=32 +datum=WGS84 +units=m +no_defs`

#[cfg(feature = "std")]
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error returned when parsing a PROJ string fails.
#[derive(Debug, Clone)]
pub struct ProjStringError(pub String);

impl std::fmt::Display for ProjStringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PROJ string error: {}", self.0)
    }
}

impl std::error::Error for ProjStringError {}

// ---------------------------------------------------------------------------
// ProjString
// ---------------------------------------------------------------------------

/// A parsed PROJ string, holding parameters as key→optional-value pairs.
///
/// Boolean flags (e.g. `+no_defs`, `+south`) are stored with `None` as the value.
#[cfg(feature = "std")]
#[derive(Debug, Clone, PartialEq)]
pub struct ProjString {
    /// All parsed parameters. Key is the param name (without `+`).
    /// Value is `Some(value_string)` for `+key=value`, `None` for bare flags.
    pub params: HashMap<String, Option<String>>,
}

#[cfg(feature = "std")]
impl ProjString {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Parse a PROJ string such as `"+proj=utm +zone=32 +datum=WGS84 +units=m +no_defs"`.
    ///
    /// Leading/trailing whitespace is ignored.  Each token must start with `+`.
    ///
    /// # Errors
    ///
    /// Returns `ProjStringError` if the string is empty or contains no `+`-prefixed tokens.
    pub fn parse(s: &str) -> Result<Self, ProjStringError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ProjStringError("input string is empty".to_string()));
        }

        let mut params: HashMap<String, Option<String>> = HashMap::new();
        let mut found_any = false;

        for token in s.split_whitespace() {
            if !token.starts_with('+') {
                // Skip tokens that don't start with '+' (e.g. leading non-proj text)
                continue;
            }
            found_any = true;
            let token = &token[1..]; // strip leading '+'
            if let Some(eq_pos) = token.find('=') {
                let key = token[..eq_pos].to_string();
                let val = token[eq_pos + 1..].to_string();
                params.insert(key, Some(val));
            } else {
                params.insert(token.to_string(), None);
            }
        }

        if !found_any {
            return Err(ProjStringError(
                "no PROJ parameters found (expected tokens starting with '+')".to_string(),
            ));
        }

        Ok(Self { params })
    }

    /// Build a `ProjString` directly from a list of key-value pairs.
    ///
    /// Flags (boolean parameters) should have `None` as the value.
    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (&'static str, Option<&'static str>)>,
    ) -> Self {
        let mut params = HashMap::new();
        for (k, v) in pairs {
            params.insert(k.to_string(), v.map(|s| s.to_string()));
        }
        Self { params }
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Get the value for `key`.
    ///
    /// - If the key exists with a value, returns `Some(value)`.
    /// - If the key exists as a boolean flag (no `=`), returns `Some("")`.
    /// - If the key is absent, returns `None`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(|opt| opt.as_deref().unwrap_or(""))
    }

    /// Returns `true` if the key is present (as either a value param or a flag).
    pub fn has(&self, key: &str) -> bool {
        self.params.contains_key(key)
    }

    /// Returns the value of `+proj`, if present.
    pub fn proj(&self) -> Option<&str> {
        self.params.get("proj").and_then(|v| v.as_deref())
    }

    /// Returns the value of `+datum`, if present.
    pub fn datum(&self) -> Option<&str> {
        self.params.get("datum").and_then(|v| v.as_deref())
    }

    /// Returns the parsed integer value of `+zone`, if present and valid.
    pub fn zone(&self) -> Option<i32> {
        self.params
            .get("zone")
            .and_then(|v| v.as_deref())
            .and_then(|s| s.parse::<i32>().ok())
    }

    /// Returns the value of `+units`, if present.
    pub fn units(&self) -> Option<&str> {
        self.params.get("units").and_then(|v| v.as_deref())
    }

    /// Returns the value of `+ellps`, if present.
    pub fn ellps(&self) -> Option<&str> {
        self.params.get("ellps").and_then(|v| v.as_deref())
    }

    /// Parse the `+towgs84` parameter as a 7-element Helmert transformation array.
    ///
    /// Expected format: `+towgs84=dx,dy,dz,rx,ry,rz,s`
    pub fn towgs84(&self) -> Option<[f64; 7]> {
        let raw = self.params.get("towgs84")?.as_deref()?;
        let parts: Vec<&str> = raw.split(',').collect();
        if parts.len() != 7 {
            return None;
        }
        let mut arr = [0.0f64; 7];
        for (i, p) in parts.iter().enumerate() {
            arr[i] = p.trim().parse::<f64>().ok()?;
        }
        Some(arr)
    }

    // -----------------------------------------------------------------------
    // Serialisation
    // -----------------------------------------------------------------------

    /// Render back to a PROJ string.  Parameters are emitted in sorted key order
    /// for deterministic output.  Boolean flags are emitted as `+flag`.
    pub fn to_proj_string(&self) -> String {
        let mut keys: Vec<&String> = self.params.keys().collect();
        keys.sort();

        let mut parts: Vec<String> = Vec::with_capacity(keys.len());
        for key in keys {
            match &self.params[key] {
                Some(val) => parts.push(format!("+{}={}", key, val)),
                None => parts.push(format!("+{}", key)),
            }
        }
        parts.join(" ")
    }

    // -----------------------------------------------------------------------
    // Common CRS factories (built directly to avoid unwrap/expect)
    // -----------------------------------------------------------------------

    /// Return the PROJ string for WGS84 geographic CRS.
    ///
    /// Equivalent to `+proj=longlat +datum=WGS84 +no_defs`.
    pub fn wgs84() -> Self {
        Self::from_pairs([
            ("proj", Some("longlat")),
            ("datum", Some("WGS84")),
            ("no_defs", None),
        ])
    }

    /// Return the PROJ string for Web Mercator (EPSG:3857).
    ///
    /// Equivalent to `+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +no_defs`.
    pub fn web_mercator() -> Self {
        Self::from_pairs([
            ("proj", Some("merc")),
            ("a", Some("6378137")),
            ("b", Some("6378137")),
            ("lat_ts", Some("0")),
            ("lon_0", Some("0")),
            ("x_0", Some("0")),
            ("y_0", Some("0")),
            ("k", Some("1")),
            ("units", Some("m")),
            ("nadgrids", Some("@null")),
            ("no_defs", None),
        ])
    }

    /// Return the PROJ string for a WGS84 UTM zone.
    ///
    /// `zone` must be 1–60. Set `south` to `true` for southern hemisphere.
    pub fn utm(zone: u8, south: bool) -> Self {
        // Build params directly (zone is dynamic, cannot be &'static str)
        let mut ps = Self::from_pairs([
            ("proj", Some("utm")),
            ("datum", Some("WGS84")),
            ("units", Some("m")),
            ("no_defs", None),
        ]);
        ps.params.insert("zone".to_string(), Some(zone.to_string()));
        if south {
            ps.params.insert("south".to_string(), None);
        }
        ps
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_utm() {
        let ps = ProjString::parse("+proj=utm +zone=32 +datum=WGS84 +units=m +no_defs")
            .expect("valid PROJ string");
        assert_eq!(ps.proj(), Some("utm"));
        assert_eq!(ps.zone(), Some(32));
        assert_eq!(ps.datum(), Some("WGS84"));
        assert_eq!(ps.units(), Some("m"));
        assert!(ps.has("no_defs"));
    }

    #[test]
    fn test_parse_error_empty() {
        assert!(ProjString::parse("").is_err());
    }

    #[test]
    fn test_wgs84_factory() {
        let ps = ProjString::wgs84();
        assert_eq!(ps.proj(), Some("longlat"));
        assert_eq!(ps.datum(), Some("WGS84"));
    }

    #[test]
    fn test_utm_factory_north() {
        let ps = ProjString::utm(32, false);
        assert_eq!(ps.proj(), Some("utm"));
        assert_eq!(ps.zone(), Some(32));
        assert!(!ps.has("south"));
    }

    #[test]
    fn test_utm_factory_south() {
        let ps = ProjString::utm(32, true);
        assert!(ps.has("south"));
    }

    #[test]
    fn test_to_string() {
        let ps = ProjString::parse("+proj=longlat +datum=WGS84 +no_defs").expect("valid");
        let s = ps.to_proj_string();
        assert!(s.contains("+proj=longlat"));
        assert!(s.contains("+datum=WGS84"));
        assert!(s.contains("+no_defs"));
    }

    #[test]
    fn test_towgs84() {
        let ps = ProjString::parse(
            "+proj=tmerc +towgs84=598.1,73.7,418.2,0.202,0.045,-2.455,6.7 +no_defs",
        )
        .expect("valid");
        let params = ps.towgs84().expect("has towgs84");
        assert!((params[0] - 598.1).abs() < 1e-9);
        assert!((params[6] - 6.7).abs() < 1e-9);
    }
}
