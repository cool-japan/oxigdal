//! Internal SQL identifier / literal escaping helpers.
//!
//! These mirror the protection provided by `oxigeo-postgis`'s `SqlIdentifier`
//! and exist so that the MySQL and TimescaleDB connectors never splice
//! caller-supplied table/column/property names — or interval/offset literals —
//! into raw SQL without validation. Values are always bound as parameters; only
//! *identifiers* and DDL-only *literals* (which cannot be parameter-bound) go
//! through these helpers.

#[allow(unused_imports)]
use crate::error::{Error, Result};

/// Quote an identifier for MySQL/MariaDB.
///
/// The identifier is wrapped in backticks with any embedded backtick doubled
/// (`` ` `` → ```` `` ````), which is the MySQL-defined way to render an
/// arbitrary identifier safely. Inputs containing a NUL byte (illegal in MySQL
/// identifiers) or exceeding the 64-character limit — the only inputs
/// backtick-quoting cannot make safe — are rejected.
#[cfg(feature = "mysql")]
pub fn quote_mysql_ident(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(Error::Query(
            "invalid SQL identifier: empty identifier".to_string(),
        ));
    }
    if name.contains('\0') {
        return Err(Error::Query(format!(
            "invalid SQL identifier (contains NUL byte): {name:?}"
        )));
    }
    if name.chars().count() > 64 {
        return Err(Error::Query(format!(
            "invalid SQL identifier (exceeds 64 characters): {name:?}"
        )));
    }
    Ok(format!("`{}`", name.replace('`', "``")))
}

/// Quote a (optionally schema-qualified) identifier for PostgreSQL/TimescaleDB.
///
/// Each dot-separated part is wrapped in double quotes with embedded double
/// quotes doubled (`"` → `""`), the PostgreSQL-defined safe quoting. A NUL byte
/// or an empty part is rejected (neither can appear in a valid identifier).
#[cfg(feature = "postgres")]
pub fn quote_pg_ident(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(Error::Query(
            "invalid SQL identifier: empty identifier".to_string(),
        ));
    }
    if name.contains('\0') {
        return Err(Error::Query(format!(
            "invalid SQL identifier (contains NUL byte): {name:?}"
        )));
    }
    let mut parts = Vec::new();
    for part in name.split('.') {
        if part.is_empty() {
            return Err(Error::Query(format!(
                "invalid SQL identifier (empty component): {name:?}"
            )));
        }
        parts.push(format!("\"{}\"", part.replace('"', "\"\"")));
    }
    Ok(parts.join("."))
}

/// Validate a bare (unquoted) SQL identifier against a strict allow-list.
///
/// Accepts only `[A-Za-z_][A-Za-z0-9_]*`. Used where an identifier must be
/// embedded *unquoted* inside a single-quoted literal (e.g. a `segment_by`
/// column list in a TimescaleDB reloption string), so that no quote/semicolon
/// can appear at all. Returns the identifier unchanged on success.
#[cfg(feature = "postgres")]
pub fn validate_bare_ident(name: &str) -> Result<&str> {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if !valid_start || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::Query(format!(
            "invalid SQL identifier (must match [A-Za-z_][A-Za-z0-9_]*): {name:?}"
        )));
    }
    Ok(name)
}

/// Escape a string for safe embedding inside a single-quoted PostgreSQL string
/// literal.
///
/// Doubles embedded single quotes (`'` → `''`) — correct under the default
/// `standard_conforming_strings = on`. Backslashes are rejected so the result
/// is also safe if a session unexpectedly enables backslash escapes, and NUL
/// bytes (which cannot appear in a text value) are rejected. The returned
/// string does **not** include the surrounding quotes; the caller supplies
/// those.
#[cfg(feature = "postgres")]
pub fn escape_pg_literal(s: &str) -> Result<String> {
    if s.contains('\0') {
        return Err(Error::Query(
            "invalid SQL literal (contains NUL byte)".to_string(),
        ));
    }
    if s.contains('\\') {
        return Err(Error::Query(format!(
            "invalid SQL literal (contains backslash): {s:?}"
        )));
    }
    Ok(s.replace('\'', "''"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    #[cfg(feature = "mysql")]
    #[test]
    fn test_quote_mysql_ident() {
        use super::quote_mysql_ident;
        assert_eq!(quote_mysql_ident("name").unwrap(), "`name`");
        // Backticks are doubled, neutralising the injection.
        assert_eq!(
            quote_mysql_ident("a`, secret) VALUES (1); DROP TABLE users; --").unwrap(),
            "`a``, secret) VALUES (1); DROP TABLE users; --`"
        );
        assert!(quote_mysql_ident("").is_err());
        assert!(quote_mysql_ident("has\0nul").is_err());
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn test_quote_pg_ident() {
        use super::quote_pg_ident;
        assert_eq!(quote_pg_ident("tbl").unwrap(), "\"tbl\"");
        assert_eq!(quote_pg_ident("public.tbl").unwrap(), "\"public\".\"tbl\"");
        // A double quote is doubled; the injected `; DROP` stays inside the
        // single quoted identifier.
        assert_eq!(
            quote_pg_ident("t\"; DROP TABLE x; --").unwrap(),
            "\"t\"\"; DROP TABLE x; --\""
        );
        assert!(quote_pg_ident("a..b").is_err());
        assert!(quote_pg_ident("").is_err());
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn test_validate_bare_ident() {
        use super::validate_bare_ident;
        assert_eq!(validate_bare_ident("device_id").unwrap(), "device_id");
        assert!(validate_bare_ident("has space").is_err());
        assert!(validate_bare_ident("x'; DROP").is_err());
        assert!(validate_bare_ident("1col").is_err());
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn test_escape_pg_literal() {
        use super::escape_pg_literal;
        assert_eq!(escape_pg_literal("1 day").unwrap(), "1 day");
        assert_eq!(escape_pg_literal("1 day'; DROP").unwrap(), "1 day''; DROP");
        assert!(escape_pg_literal("back\\slash").is_err());
    }
}
