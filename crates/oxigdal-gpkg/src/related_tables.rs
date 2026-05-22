//! GeoPackage Related Tables Extension (GPKG-RTE).
//!
//! Implements parsing of the `gpkg_relations` table and its associated mapping
//! tables as defined in the OGC GeoPackage Related Tables Extension
//! specification (<https://www.geopackage.org/spec/related-tables/>).
//!
//! The extension describes many-to-many relationships between feature tables,
//! attribute tables, media resources, and tile pyramids by storing pairs of
//! foreign keys in intermediate "mapping tables".
//!
//! # Key types
//!
//! * [`RelationType`] — semantic kind of the relationship.
//! * [`GpkgRelation`] — one row of `gpkg_relations`.
//! * [`MappingRow`] — one `(base_id, related_id)` pair from a mapping table.

use std::str::FromStr;

// ─────────────────────────────────────────────────────────────────────────────
// RelationType
// ─────────────────────────────────────────────────────────────────────────────

/// Semantic relation type stored in the `relation_name` column of
/// `gpkg_relations` (OGC GPKG-RTE §2.4).
///
/// The OGC specification currently defines five standardised relation types.
/// Any extension-specific or author-defined name that does not match one of
/// the five canonical strings is represented by [`RelationType::Other`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationType {
    /// Binary media: tiles, images, documents.
    ///
    /// Stored as `"media"` in `gpkg_relations`.
    Media,
    /// Simple non-spatial attributes.
    ///
    /// Stored as `"simple_attributes"` in `gpkg_relations`.
    SimpleAttributes,
    /// Many-to-many relationship between two feature tables.
    ///
    /// Stored as `"related_features"` (or the legacy alias `"features"`)
    /// in `gpkg_relations`.
    RelatedFeatures,
    /// Tile pyramid tiles.
    ///
    /// Stored as `"tiles"` in `gpkg_relations`.
    Tiles,
    /// Generic attribute table.
    ///
    /// Stored as `"attributes"` in `gpkg_relations`.
    Attributes,
    /// Unknown or extension-specific relation type.
    ///
    /// The inner `String` preserves the raw value from the table so that
    /// callers can still inspect or forward it without data loss.
    Other(String),
}

impl RelationType {
    /// Return the canonical OGC string for this relation type.
    ///
    /// The returned value is suitable for direct insertion into the
    /// `relation_name` column of `gpkg_relations`.  For
    /// [`RelationType::Other`] the raw stored string is returned as-is.
    ///
    /// Note that `RelatedFeatures` serialises to `"related_features"` (not
    /// to the legacy alias `"features"`).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Media => "media",
            Self::SimpleAttributes => "simple_attributes",
            Self::RelatedFeatures => "related_features",
            Self::Tiles => "tiles",
            Self::Attributes => "attributes",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl FromStr for RelationType {
    /// Parsing is infallible: unknown strings map to [`RelationType::Other`].
    type Err = std::convert::Infallible;

    /// Parse the `relation_name` column value from `gpkg_relations`.
    ///
    /// The five OGC-defined canonical strings are recognised
    /// case-sensitively.  Any other input maps to [`RelationType::Other`].
    ///
    /// The legacy alias `"features"` for `RelatedFeatures` is also accepted
    /// to support older GeoPackage producers.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rt = match s {
            "media" => Self::Media,
            "simple_attributes" => Self::SimpleAttributes,
            "related_features" | "features" => Self::RelatedFeatures,
            "tiles" => Self::Tiles,
            "attributes" => Self::Attributes,
            other => Self::Other(other.to_owned()),
        };
        Ok(rt)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgRelation
// ─────────────────────────────────────────────────────────────────────────────

/// A row from the `gpkg_relations` table (OGC GPKG-RTE §2.4).
///
/// Each row describes a many-to-many relationship between two user tables.
/// The relationship is reified through an intermediate mapping table whose
/// name is stored in `mapping_table_name`.
///
/// Column layout (0-based within the B-tree row):
///
/// | # | Column                    | SQL type     | Default |
/// |---|---------------------------|--------------|---------|
/// | 0 | `id`                      | INTEGER PK   | auto    |
/// | 1 | `base_table_name`         | TEXT NOT NULL| —       |
/// | 2 | `base_primary_column`     | TEXT NOT NULL| `"id"`  |
/// | 3 | `related_table_name`      | TEXT NOT NULL| —       |
/// | 4 | `related_primary_column`  | TEXT NOT NULL| `"id"`  |
/// | 5 | `relation_name`           | TEXT NOT NULL| —       |
/// | 6 | `mapping_table_name`      | TEXT NOT NULL| —       |
#[derive(Debug, Clone, PartialEq)]
pub struct GpkgRelation {
    /// Primary-key value from the `id` column.
    pub id: i64,
    /// Name of the "left" (base) table in the relationship.
    pub base_table_name: String,
    /// Name of the primary-key column in the base table.
    ///
    /// Defaults to `"id"` per the specification.
    pub base_primary_column: String,
    /// Name of the "right" (related) table in the relationship.
    pub related_table_name: String,
    /// Name of the primary-key column in the related table.
    ///
    /// Defaults to `"id"` per the specification.
    pub related_primary_column: String,
    /// Semantic relation type.
    pub relation_name: RelationType,
    /// Name of the mapping table that holds `(base_id, related_id)` pairs
    /// linking rows in the base and related tables.
    pub mapping_table_name: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// MappingRow
// ─────────────────────────────────────────────────────────────────────────────

/// A row from a GPKG-RTE mapping table.
///
/// Mapping tables have the following schema:
///
/// ```sql
/// CREATE TABLE <mapping_table_name> (
///   id          INTEGER PRIMARY KEY AUTOINCREMENT,
///   base_id     INTEGER NOT NULL,
///   related_id  INTEGER NOT NULL
/// );
/// ```
///
/// Each `MappingRow` represents one directed edge in the many-to-many join
/// between the base and related tables referenced by a [`GpkgRelation`].
#[derive(Debug, Clone, PartialEq)]
pub struct MappingRow {
    /// Primary-key value of this mapping row.
    pub id: i64,
    /// Foreign key into the base table (references `base_primary_column`).
    pub base_id: i64,
    /// Foreign key into the related table (references `related_primary_column`).
    pub related_id: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for the types defined here (no I/O required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // FromStr::from_str is infallible for RelationType, so .unwrap() is safe here.
    fn parse(s: &str) -> RelationType {
        s.parse().unwrap()
    }

    #[test]
    fn test_relation_type_from_str_known_variants() {
        assert_eq!(parse("media"), RelationType::Media);
        assert_eq!(parse("simple_attributes"), RelationType::SimpleAttributes);
        assert_eq!(parse("related_features"), RelationType::RelatedFeatures);
        assert_eq!(parse("features"), RelationType::RelatedFeatures);
        assert_eq!(parse("tiles"), RelationType::Tiles);
        assert_eq!(parse("attributes"), RelationType::Attributes);
    }

    #[test]
    fn test_relation_type_from_str_other() {
        assert_eq!(
            parse("custom_relation"),
            RelationType::Other("custom_relation".to_owned())
        );
        assert_eq!(parse(""), RelationType::Other(String::new()));
    }

    #[test]
    fn test_relation_type_as_str_roundtrip() {
        let cases = [
            ("media", RelationType::Media),
            ("simple_attributes", RelationType::SimpleAttributes),
            ("related_features", RelationType::RelatedFeatures),
            ("tiles", RelationType::Tiles),
            ("attributes", RelationType::Attributes),
        ];
        for (expected, variant) in &cases {
            assert_eq!(
                variant.as_str(),
                *expected,
                "as_str() for {variant:?} should return {expected:?}"
            );
            assert_eq!(
                parse(expected),
                *variant,
                "parse({expected:?}) did not round-trip"
            );
        }
    }

    #[test]
    fn test_relation_type_other_preserves_raw_string() {
        let raw = "my_organisation_ext";
        let rt = parse(raw);
        assert_eq!(rt.as_str(), raw);
    }

    #[test]
    fn test_gpkg_relation_clone_and_eq() {
        let rel = GpkgRelation {
            id: 7,
            base_table_name: "features".to_owned(),
            base_primary_column: "id".to_owned(),
            related_table_name: "media".to_owned(),
            related_primary_column: "id".to_owned(),
            relation_name: RelationType::Media,
            mapping_table_name: "features_media".to_owned(),
        };
        let cloned = rel.clone();
        assert_eq!(rel, cloned);
    }

    #[test]
    fn test_mapping_row_clone_and_eq() {
        let row = MappingRow {
            id: 1,
            base_id: 42,
            related_id: 99,
        };
        let cloned = row.clone();
        assert_eq!(row, cloned);
    }
}
