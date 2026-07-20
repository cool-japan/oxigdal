//! GeoPackage extensions registry (OGC 12-128r19 §F.4).
//!
//! The `gpkg_extensions` table is mandatory when a GeoPackage uses features
//! beyond the core standard, such as RTree spatial indexing, WebP tiles, or
//! any author-defined extension.  This module provides a typed representation
//! of each row and helpers to parse the scope string.

use std::str::FromStr;

// ── Extension scope ───────────────────────────────────────────────────────────

/// Extension scope as defined in OGC 12-128r19 §F.4.
///
/// The `scope` column of `gpkg_extensions` constrains whether callers that do
/// not understand a given extension can safely read the file.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtensionScope {
    /// The extension must be honoured for both reading **and** writing.
    ///
    /// A reader that does not implement this extension may produce incorrect
    /// results or corrupt data.
    ReadWrite,
    /// The extension only affects write operations; safe to read without
    /// implementing it.
    WriteOnly,
}

impl FromStr for ExtensionScope {
    type Err = ();

    /// Parse the canonical scope strings from the GeoPackage spec.
    ///
    /// Per OGC 12-128r19 §F.4 the only defined values are `"read-write"` and
    /// `"write-only"`.  Any unrecognised string defaults to
    /// [`ExtensionScope::ReadWrite`] (the safer assumption).
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "read-write" => Ok(ExtensionScope::ReadWrite),
            "write-only" => Ok(ExtensionScope::WriteOnly),
            _ => Ok(ExtensionScope::ReadWrite), // conservative default for unknown
        }
    }
}

// ── GpkgExtension row ────────────────────────────────────────────────────────

/// A single row from the `gpkg_extensions` table (OGC 12-128r19 §F.4).
///
/// Column layout:
///
/// | # | column           | SQL type         |
/// |---|------------------|------------------|
/// | 0 | `table_name`     | TEXT (nullable)  |
/// | 1 | `column_name`    | TEXT (nullable)  |
/// | 2 | `extension_name` | TEXT NOT NULL    |
/// | 3 | `definition`     | TEXT NOT NULL    |
/// | 4 | `scope`          | TEXT NOT NULL    |
#[derive(Debug, Clone, PartialEq)]
pub struct GpkgExtension {
    /// Name of the table the extension applies to, if table-specific.
    ///
    /// `None` indicates the extension applies to the GeoPackage as a whole.
    pub table_name: Option<String>,
    /// Name of the column the extension applies to, if column-specific.
    ///
    /// Requires `table_name` to be `Some` per the spec.
    pub column_name: Option<String>,
    /// Unique extension name, e.g. `"gpkg_rtree_index"`, `"gpkg_webp"`.
    ///
    /// The prefix `gpkg_` is reserved for OGC-registered extensions.
    pub extension_name: String,
    /// URI or human-readable description of the extension definition.
    pub definition: String,
    /// Whether the extension must be honoured for reads, writes, or both.
    pub scope: ExtensionScope,
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_scope_from_str_read_write() {
        let scope = "read-write"
            .parse::<ExtensionScope>()
            .expect("should parse read-write");
        assert_eq!(scope, ExtensionScope::ReadWrite);
    }

    #[test]
    fn test_extension_scope_from_str_write_only() {
        let scope = "write-only"
            .parse::<ExtensionScope>()
            .expect("should parse write-only");
        assert_eq!(scope, ExtensionScope::WriteOnly);
    }

    #[test]
    fn test_extension_scope_from_str_unknown_defaults_to_read_write() {
        let scope = "something-unrecognised"
            .parse::<ExtensionScope>()
            .expect("unknown scope should fall back to ReadWrite");
        assert_eq!(
            scope,
            ExtensionScope::ReadWrite,
            "unknown scope string must default to the conservative ReadWrite variant"
        );
    }

    #[test]
    fn test_gpkg_extension_debug_format() {
        let ext = GpkgExtension {
            table_name: Some("features_table".to_string()),
            column_name: Some("geom".to_string()),
            extension_name: "gpkg_rtree_index".to_string(),
            definition: "http://www.geopackage.org/spec/#extension_rtree".to_string(),
            scope: ExtensionScope::ReadWrite,
        };
        let debug_str = format!("{ext:?}");
        assert!(debug_str.contains("gpkg_rtree_index"));
        assert!(debug_str.contains("ReadWrite"));
        assert!(debug_str.contains("features_table"));
    }

    #[test]
    fn test_gpkg_extension_clone_equality() {
        let ext = GpkgExtension {
            table_name: None,
            column_name: None,
            extension_name: "gpkg_webp".to_string(),
            definition: "http://www.geopackage.org/spec/#extension_webp".to_string(),
            scope: ExtensionScope::ReadWrite,
        };
        let cloned = ext.clone();
        assert_eq!(ext, cloned, "cloned GpkgExtension must equal the original");
    }

    #[test]
    fn test_gpkg_extension_write_only_scope() {
        let ext = GpkgExtension {
            table_name: Some("metadata_table".to_string()),
            column_name: None,
            extension_name: "gpkg_metadata".to_string(),
            definition: "http://www.geopackage.org/spec/#extension_metadata".to_string(),
            scope: ExtensionScope::WriteOnly,
        };
        assert_eq!(ext.scope, ExtensionScope::WriteOnly);
        assert!(ext.table_name.is_some());
        assert!(ext.column_name.is_none());
    }

    #[test]
    fn test_extension_scope_exhaustive_known_values() {
        // Ensure both spec-defined string representations round-trip correctly.
        let cases: &[(&str, ExtensionScope)] = &[
            ("read-write", ExtensionScope::ReadWrite),
            ("write-only", ExtensionScope::WriteOnly),
        ];
        for (s, expected) in cases {
            let parsed = s
                .parse::<ExtensionScope>()
                .expect("known scope string must parse without error");
            assert_eq!(
                &parsed, expected,
                "scope string '{s}' did not round-trip correctly"
            );
        }
    }
}
