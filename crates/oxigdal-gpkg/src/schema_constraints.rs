//! GeoPackage `gpkg_data_column_constraints` extension parser + validator.
//!
//! Implements OGC GeoPackage Encoding Standard §F.5 — the Schema option of the
//! Schema extension — which lets producers attach declarative constraints
//! (`range`, `enum`, `glob`) to user-data columns via the `gpkg_schema`
//! extension.
//!
//! The system table `gpkg_data_column_constraints` carries the rule rows;
//! a separate `gpkg_data_columns` table (not handled here) binds constraint
//! names to user columns.  This module focuses on:
//!
//! * Parsing the constraint rows themselves from a [`GeoPackage`].
//! * Building a [`ConstraintValidator`] that can decide whether a given
//!   [`CellValue`] satisfies the rules registered under a name.
//!
//! The spec column layout of `gpkg_data_column_constraints` (0-based positions
//! in the B-tree record) is:
//!
//! | # | column              | SQL type   | semantics                       |
//! |---|---------------------|------------|---------------------------------|
//! | 0 | `constraint_name`   | TEXT       | NOT NULL — rule identifier      |
//! | 1 | `constraint_type`   | TEXT       | `"range"` \| `"enum"` \| `"glob"` |
//! | 2 | `value`             | TEXT       | enum literal / glob pattern     |
//! | 3 | `min`               | NUMERIC    | numeric lower bound (range)     |
//! | 4 | `min_is_inclusive`  | BOOLEAN    | inclusivity flag (range)        |
//! | 5 | `max`               | NUMERIC    | numeric upper bound (range)     |
//! | 6 | `max_is_inclusive`  | BOOLEAN    | inclusivity flag (range)        |
//! | 7 | `description`       | TEXT       | optional free-text annotation   |
//!
//! Multiple rows with the same `constraint_name` collectively express an
//! enumeration; a `range` or `glob` constraint is represented by a single row.

use std::collections::HashMap;
use std::str::FromStr;

use crate::btree::CellValue;
use crate::error::GpkgError;
use crate::gpkg::GeoPackage;

// ─────────────────────────────────────────────────────────────────────────────
// ConstraintType — the three kinds defined by OGC §F.5
// ─────────────────────────────────────────────────────────────────────────────

/// Discriminator for the three constraint kinds permitted by OGC §F.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintType {
    /// Numeric interval check using `min` / `max` columns with inclusivity flags.
    Range,
    /// Enumeration: the value must equal one of the registered `value` strings
    /// (multiple rows share the same `constraint_name`, one literal each).
    Enum,
    /// SQLite GLOB pattern match against `value`.
    Glob,
}

impl FromStr for ConstraintType {
    type Err = GpkgError;

    /// Parse the canonical strings (`"range"`, `"enum"`, `"glob"`),
    /// case-insensitively.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "range" => Ok(Self::Range),
            "enum" => Ok(Self::Enum),
            "glob" => Ok(Self::Glob),
            other => Err(GpkgError::ParseError(format!(
                "unknown constraint_type: {other}"
            ))),
        }
    }
}

impl ConstraintType {
    /// Return the canonical lower-case string used in the system table.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Range => "range",
            Self::Enum => "enum",
            Self::Glob => "glob",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DataColumnConstraint — one parsed row of gpkg_data_column_constraints
// ─────────────────────────────────────────────────────────────────────────────

/// A typed representation of one `gpkg_data_column_constraints` row.
///
/// Field optionality mirrors the SQL nullability of each column: every column
/// except `constraint_name` and `constraint_type` may be absent depending on
/// the [`ConstraintType`] of the row.
#[derive(Debug, Clone, PartialEq)]
pub struct DataColumnConstraint {
    /// Logical name of the rule (foreign key target from `gpkg_data_columns`).
    pub constraint_name: String,
    /// Discriminator for the kind of validation to perform.
    pub constraint_type: ConstraintType,
    /// Enum literal or GLOB pattern — required for `Enum`/`Glob`, ignored for `Range`.
    pub value: Option<String>,
    /// Range lower bound (NULL means unbounded below).
    pub min: Option<f64>,
    /// Whether `min` is inclusive (defaults to `true` when NULL per the spec).
    pub min_is_inclusive: Option<bool>,
    /// Range upper bound (NULL means unbounded above).
    pub max: Option<f64>,
    /// Whether `max` is inclusive (defaults to `true` when NULL per the spec).
    pub max_is_inclusive: Option<bool>,
    /// Optional human-readable description of the constraint.
    pub description: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// ConstraintViolation — returned by ConstraintValidator::validate
// ─────────────────────────────────────────────────────────────────────────────

/// Diagnostic produced when a [`CellValue`] fails a constraint check.
///
/// The struct is intentionally non-`Error`-typed to keep validation results
/// easy to aggregate; convert to [`GpkgError::ConstraintViolation`] at the
/// call site when bubbling up as an error.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintViolation {
    /// Name of the violated constraint.
    pub constraint_name: String,
    /// Kind of constraint that was violated.
    pub constraint_type: ConstraintType,
    /// String rendering of the offending value (for diagnostics).
    pub actual_value: String,
    /// Human-readable explanation, e.g. `"value 5 above max 4 (inclusive)"`.
    pub reason: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Loader — parse the gpkg_data_column_constraints system table
// ─────────────────────────────────────────────────────────────────────────────

/// Load every row from `gpkg_data_column_constraints`.
///
/// Returns an empty vector when the table is absent (the table is optional and
/// only present when the GeoPackage Schema extension is in use).
///
/// Rows whose `constraint_type` cannot be parsed are silently skipped — this
/// matches the defensive style used by [`GeoPackage::load_extensions`] and
/// [`GeoPackage::load_metadata`].  Rows with fewer than 8 value cells are also
/// skipped to defend against schema-version mismatches.
///
/// # Errors
/// Returns an error if the underlying `sqlite_master` scan or the B-tree
/// traversal of the constraints table fails for reasons other than a missing
/// table.
pub fn load_data_column_constraints(
    gpkg: &GeoPackage,
) -> Result<Vec<DataColumnConstraint>, GpkgError> {
    let rows = match gpkg.scan_table_by_name("gpkg_data_column_constraints")? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut out = Vec::with_capacity(rows.len());
    for (_rowid, values) in rows {
        if values.len() < 8 {
            // Schema mismatch — skip the row defensively rather than erroring.
            continue;
        }

        let constraint_name = cell_to_string(&values[0]);
        if constraint_name.is_empty() {
            // Per OGC the constraint_name column is NOT NULL; an empty value
            // indicates a malformed row that we silently ignore.
            continue;
        }

        let raw_type = cell_to_string(&values[1]);
        let Ok(constraint_type) = raw_type.parse::<ConstraintType>() else {
            // Skip rows with unknown constraint types rather than failing
            // the whole load — older producers may emit author-defined kinds.
            continue;
        };

        let value = cell_to_optional_string(&values[2]);
        let min = cell_to_optional_f64(&values[3]);
        let min_is_inclusive = cell_to_optional_bool(&values[4]);
        let max = cell_to_optional_f64(&values[5]);
        let max_is_inclusive = cell_to_optional_bool(&values[6]);
        let description = cell_to_optional_string(&values[7]);

        out.push(DataColumnConstraint {
            constraint_name,
            constraint_type,
            value,
            min,
            min_is_inclusive,
            max,
            max_is_inclusive,
            description,
        });
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// ConstraintValidator — group rules by name and validate values against them
// ─────────────────────────────────────────────────────────────────────────────

/// Validator that indexes [`DataColumnConstraint`] rows by `constraint_name`
/// and dispatches to the right rule kind when [`Self::validate`] is called.
///
/// Multiple rows with the same name form an enumeration; the validator handles
/// this by storing a `Vec<DataColumnConstraint>` per name.
#[derive(Debug, Clone)]
pub struct ConstraintValidator {
    constraints: HashMap<String, Vec<DataColumnConstraint>>,
}

impl ConstraintValidator {
    /// Build a validator from a flat list of rule rows.
    ///
    /// Rules with the same `constraint_name` are grouped together.
    #[must_use]
    pub fn new(constraints: Vec<DataColumnConstraint>) -> Self {
        let mut map: HashMap<String, Vec<DataColumnConstraint>> = HashMap::new();
        for c in constraints {
            map.entry(c.constraint_name.clone()).or_default().push(c);
        }
        Self { constraints: map }
    }

    /// Return the number of distinct constraint names registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.constraints.len()
    }

    /// Return `true` when no constraints are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }

    /// Return `true` when a constraint with the given name has been registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.constraints.contains_key(name)
    }

    /// Validate `value` against the constraint registered under `name`.
    ///
    /// Returns `Ok(())` when the value satisfies the rule (or no rule exists
    /// for `name` — unknown names are intentionally permissive).
    ///
    /// SQL `NULL` always passes — this matches the standard SQL CHECK semantics
    /// where a NULL operand makes the predicate unknown (treated as pass).
    ///
    /// # Errors
    /// Returns a [`ConstraintViolation`] describing the failure when the value
    /// does not satisfy the rule.
    pub fn validate(&self, name: &str, value: &CellValue) -> Result<(), ConstraintViolation> {
        let Some(rules) = self.constraints.get(name) else {
            return Ok(());
        };
        if rules.is_empty() {
            return Ok(());
        }
        let ctype = rules[0].constraint_type;
        match ctype {
            ConstraintType::Range => self.validate_range(name, value, &rules[0]),
            ConstraintType::Enum => self.validate_enum(name, value, rules),
            ConstraintType::Glob => self.validate_glob(name, value, &rules[0]),
        }
    }

    // ── Range ────────────────────────────────────────────────────────────────

    fn validate_range(
        &self,
        name: &str,
        value: &CellValue,
        rule: &DataColumnConstraint,
    ) -> Result<(), ConstraintViolation> {
        let num = match value {
            CellValue::Integer(i) => *i as f64,
            CellValue::Float(f) => *f,
            CellValue::Null => return Ok(()),
            other => {
                return Err(ConstraintViolation {
                    constraint_name: name.to_owned(),
                    constraint_type: ConstraintType::Range,
                    actual_value: render_cell(other),
                    reason: "non-numeric value for range constraint".to_owned(),
                });
            }
        };

        if let Some(min) = rule.min {
            let inclusive = rule.min_is_inclusive.unwrap_or(true);
            let ok = if inclusive { num >= min } else { num > min };
            if !ok {
                return Err(ConstraintViolation {
                    constraint_name: name.to_owned(),
                    constraint_type: ConstraintType::Range,
                    actual_value: num.to_string(),
                    reason: format!(
                        "value {num} below min {min} ({})",
                        if inclusive { "inclusive" } else { "exclusive" }
                    ),
                });
            }
        }

        if let Some(max) = rule.max {
            let inclusive = rule.max_is_inclusive.unwrap_or(true);
            let ok = if inclusive { num <= max } else { num < max };
            if !ok {
                return Err(ConstraintViolation {
                    constraint_name: name.to_owned(),
                    constraint_type: ConstraintType::Range,
                    actual_value: num.to_string(),
                    reason: format!(
                        "value {num} above max {max} ({})",
                        if inclusive { "inclusive" } else { "exclusive" }
                    ),
                });
            }
        }

        Ok(())
    }

    // ── Enum ─────────────────────────────────────────────────────────────────

    fn validate_enum(
        &self,
        name: &str,
        value: &CellValue,
        rules: &[DataColumnConstraint],
    ) -> Result<(), ConstraintViolation> {
        let text = match value {
            CellValue::Text(s) => s.clone(),
            CellValue::Integer(i) => i.to_string(),
            CellValue::Float(f) => f.to_string(),
            CellValue::Null => return Ok(()),
            other => {
                return Err(ConstraintViolation {
                    constraint_name: name.to_owned(),
                    constraint_type: ConstraintType::Enum,
                    actual_value: render_cell(other),
                    reason: "non-comparable value for enum constraint".to_owned(),
                });
            }
        };

        let allowed: Vec<&str> = rules.iter().filter_map(|r| r.value.as_deref()).collect();
        if allowed.iter().any(|v| *v == text) {
            Ok(())
        } else {
            let allowed_display = if allowed.is_empty() {
                "<empty>".to_owned()
            } else {
                allowed.join(", ")
            };
            Err(ConstraintViolation {
                constraint_name: name.to_owned(),
                constraint_type: ConstraintType::Enum,
                actual_value: text,
                reason: format!("value not in enum [{allowed_display}]"),
            })
        }
    }

    // ── Glob ─────────────────────────────────────────────────────────────────

    fn validate_glob(
        &self,
        name: &str,
        value: &CellValue,
        rule: &DataColumnConstraint,
    ) -> Result<(), ConstraintViolation> {
        let text = match value {
            CellValue::Text(s) => s.clone(),
            CellValue::Null => return Ok(()),
            other => {
                return Err(ConstraintViolation {
                    constraint_name: name.to_owned(),
                    constraint_type: ConstraintType::Glob,
                    actual_value: render_cell(other),
                    reason: "non-text value for glob constraint".to_owned(),
                });
            }
        };

        let Some(pattern) = rule.value.as_deref() else {
            return Err(ConstraintViolation {
                constraint_name: name.to_owned(),
                constraint_type: ConstraintType::Glob,
                actual_value: text,
                reason: "glob constraint has no pattern (value column is NULL)".to_owned(),
            });
        };

        if glob_matches(pattern, &text) {
            Ok(())
        } else {
            Err(ConstraintViolation {
                constraint_name: name.to_owned(),
                constraint_type: ConstraintType::Glob,
                actual_value: text,
                reason: format!("value does not match glob pattern '{pattern}'"),
            })
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SQLite GLOB pattern matching
// ─────────────────────────────────────────────────────────────────────────────

/// SQLite GLOB pattern matching.
///
/// Differs from SQL `LIKE` in important ways:
/// * `*` matches zero or more characters (SQL `LIKE` uses `%`).
/// * `?` matches exactly one character (SQL `LIKE` uses `_`).
/// * `[abc]` matches one of the listed characters (character class).
/// * `[^abc]` or `[!abc]` matches any character **not** in the class.
/// * `[a-z]` matches any character in the inclusive range.
/// * Inside a character class, `]` is taken literally when it is the first
///   character (otherwise it terminates the class).
/// * GLOB is **case-sensitive** in the default SQLite configuration; LIKE is
///   case-insensitive for ASCII.
/// * No backslash escape semantics — to match a literal `*`, `?`, or `[` they
///   must appear inside a character class (e.g. `[*]`).
///
/// Implementation strategy: a recursive byte-level matcher that mirrors the
/// SQLite C code (`patternCompare` in `func.c`).  Because patterns are user
/// supplied and typically small, recursion depth is bounded by the pattern
/// length and stack overflow is not a practical concern.
#[must_use]
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_bytes(pat: &[u8], s: &[u8]) -> bool {
    let mut p = 0usize;
    let mut t = 0usize;

    // Iterative outer loop with one recursive call to handle the `*` branch.
    while p < pat.len() {
        let c = pat[p];
        match c {
            b'*' => {
                // Collapse consecutive '*' wildcards.
                while p < pat.len() && pat[p] == b'*' {
                    p += 1;
                }
                if p == pat.len() {
                    // Trailing '*' matches the entire remainder.
                    return true;
                }
                // Try every split point of the remaining text.  Using the rest
                // of the pattern after the star, recursively check each suffix.
                let rest_pat = &pat[p..];
                for i in t..=s.len() {
                    if glob_match_bytes(rest_pat, &s[i..]) {
                        return true;
                    }
                }
                return false;
            }
            b'?' => {
                if t >= s.len() {
                    return false;
                }
                p += 1;
                t += 1;
            }
            b'[' => {
                if t >= s.len() {
                    return false;
                }
                let Some((matched, consumed)) = match_char_class(&pat[p..], s[t]) else {
                    // Malformed class — treat the literal `[` byte as itself,
                    // mirroring SQLite's lenient handling.
                    if s[t] != b'[' {
                        return false;
                    }
                    p += 1;
                    t += 1;
                    continue;
                };
                if !matched {
                    return false;
                }
                p += consumed;
                t += 1;
            }
            _ => {
                if t >= s.len() || s[t] != c {
                    return false;
                }
                p += 1;
                t += 1;
            }
        }
    }

    t == s.len()
}

/// Try to match `byte` against the character class that begins at `pat[0]`.
///
/// Returns `Some((matched, bytes_consumed_from_pattern))` on success, where
/// `bytes_consumed_from_pattern` includes the leading `[` and trailing `]`.
/// Returns `None` when the class is malformed (e.g. no closing `]`), allowing
/// the caller to fall back to literal handling.
fn match_char_class(pat: &[u8], byte: u8) -> Option<(bool, usize)> {
    // pat[0] is '['.  Determine negation, then scan to the closing ']'.
    if pat.is_empty() || pat[0] != b'[' {
        return None;
    }
    let mut i = 1usize;
    let mut negated = false;
    if i < pat.len() && (pat[i] == b'^' || pat[i] == b'!') {
        negated = true;
        i += 1;
    }

    let mut matched = false;
    let mut first_class_byte = true;
    let mut class_closed = false;

    while i < pat.len() {
        let ch = pat[i];

        // A `]` as the very first byte of the class is taken literally.
        if ch == b']' && !first_class_byte {
            class_closed = true;
            i += 1;
            break;
        }

        // Range: a-b (only if we have at least 3 more bytes including the `]`).
        if i + 2 < pat.len() && pat[i + 1] == b'-' && pat[i + 2] != b']' {
            let lo = ch;
            let hi = pat[i + 2];
            if (lo..=hi).contains(&byte) || (hi..=lo).contains(&byte) {
                matched = true;
            }
            i += 3;
            first_class_byte = false;
            continue;
        }

        if ch == byte {
            matched = true;
        }
        i += 1;
        first_class_byte = false;
    }

    if !class_closed {
        return None;
    }
    Some((matched ^ negated, i))
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal cell-value coercion helpers (private to this module)
// ─────────────────────────────────────────────────────────────────────────────

fn cell_to_string(v: &CellValue) -> String {
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

fn cell_to_optional_string(v: &CellValue) -> Option<String> {
    match v {
        CellValue::Null => None,
        CellValue::Text(s) if s.is_empty() => None,
        other => Some(cell_to_string(other)),
    }
}

fn cell_to_optional_f64(v: &CellValue) -> Option<f64> {
    match v {
        CellValue::Null => None,
        CellValue::Integer(i) => Some(*i as f64),
        CellValue::Float(f) => Some(*f),
        CellValue::Text(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Coerce a [`CellValue`] to `Option<bool>` using SQLite-style truthiness:
///
/// * `NULL` → `None`
/// * `Integer(0)` / `Float(0.0)` → `Some(false)`
/// * Any other integer / float → `Some(true)`
/// * `Text("true"|"false"|"1"|"0")` case-insensitive → `Some(..)`
/// * Anything else → `None`
fn cell_to_optional_bool(v: &CellValue) -> Option<bool> {
    match v {
        CellValue::Null => None,
        CellValue::Integer(i) => Some(*i != 0),
        CellValue::Float(f) => Some(*f != 0.0),
        CellValue::Text(s) => match s.to_ascii_lowercase().as_str() {
            "1" | "true" | "t" | "yes" | "y" => Some(true),
            "0" | "false" | "f" | "no" | "n" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Pretty-print a [`CellValue`] for diagnostic output in `ConstraintViolation`.
fn render_cell(v: &CellValue) -> String {
    match v {
        CellValue::Null => "NULL".to_owned(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Text(s) => s.clone(),
        CellValue::Blob(b) => format!("<blob {} bytes>", b.len()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (round-trip and corner-case coverage for glob_matches)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ConstraintType parsing ───────────────────────────────────────────────

    #[test]
    fn test_constraint_type_from_str_canonical() {
        assert_eq!(
            "range".parse::<ConstraintType>().expect("range"),
            ConstraintType::Range
        );
        assert_eq!(
            "enum".parse::<ConstraintType>().expect("enum"),
            ConstraintType::Enum
        );
        assert_eq!(
            "glob".parse::<ConstraintType>().expect("glob"),
            ConstraintType::Glob
        );
    }

    #[test]
    fn test_constraint_type_from_str_case_insensitive() {
        assert_eq!(
            "RANGE".parse::<ConstraintType>().expect("RANGE"),
            ConstraintType::Range
        );
        assert_eq!(
            "Enum".parse::<ConstraintType>().expect("Enum"),
            ConstraintType::Enum
        );
    }

    #[test]
    fn test_constraint_type_from_str_rejects_unknown() {
        let err = "regex"
            .parse::<ConstraintType>()
            .expect_err("must reject regex");
        assert!(matches!(err, GpkgError::ParseError(_)));
    }

    #[test]
    fn test_constraint_type_as_str_roundtrip() {
        for t in [
            ConstraintType::Range,
            ConstraintType::Enum,
            ConstraintType::Glob,
        ] {
            let s = t.as_str();
            let parsed = s.parse::<ConstraintType>().expect("round-trip parse");
            assert_eq!(parsed, t);
        }
    }

    // ── glob_matches — basic wildcards ───────────────────────────────────────

    #[test]
    fn test_glob_star_matches_anything() {
        assert!(glob_matches("*", ""));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("foo*", "foobar"));
        assert!(glob_matches("*bar", "foobar"));
        assert!(glob_matches("*oo*", "foobar"));
    }

    #[test]
    fn test_glob_question_matches_one() {
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "ac"));
        assert!(!glob_matches("a?c", "abbc"));
    }

    #[test]
    fn test_glob_literal_match() {
        assert!(glob_matches("exact", "exact"));
        assert!(!glob_matches("exact", "Exact")); // case-sensitive
        assert!(!glob_matches("exact", "exactly"));
    }

    #[test]
    fn test_glob_empty_pattern_matches_empty_only() {
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "x"));
    }

    // ── glob_matches — character classes ─────────────────────────────────────

    #[test]
    fn test_glob_char_class_simple() {
        assert!(glob_matches("[abc]", "a"));
        assert!(glob_matches("[abc]", "b"));
        assert!(!glob_matches("[abc]", "d"));
    }

    #[test]
    fn test_glob_char_class_range() {
        assert!(glob_matches("[a-z]", "m"));
        assert!(!glob_matches("[a-z]", "M"));
        assert!(glob_matches("[0-9]", "5"));
    }

    #[test]
    fn test_glob_char_class_negated_caret() {
        assert!(glob_matches("[^abc]", "d"));
        assert!(!glob_matches("[^abc]", "a"));
    }

    #[test]
    fn test_glob_char_class_negated_bang() {
        assert!(glob_matches("[!0-9]", "x"));
        assert!(!glob_matches("[!0-9]", "5"));
    }

    // ── ConstraintValidator — range ──────────────────────────────────────────

    #[test]
    fn test_validate_range_inclusive_min_accepts_edge() {
        let v = ConstraintValidator::new(vec![DataColumnConstraint {
            constraint_name: "r".into(),
            constraint_type: ConstraintType::Range,
            value: None,
            min: Some(0.0),
            min_is_inclusive: Some(true),
            max: Some(10.0),
            max_is_inclusive: Some(true),
            description: None,
        }]);
        assert!(v.validate("r", &CellValue::Integer(0)).is_ok());
        assert!(v.validate("r", &CellValue::Integer(10)).is_ok());
    }

    #[test]
    fn test_validate_range_exclusive_min_rejects_edge() {
        let v = ConstraintValidator::new(vec![DataColumnConstraint {
            constraint_name: "r".into(),
            constraint_type: ConstraintType::Range,
            value: None,
            min: Some(0.0),
            min_is_inclusive: Some(false),
            max: None,
            max_is_inclusive: None,
            description: None,
        }]);
        let err = v
            .validate("r", &CellValue::Integer(0))
            .expect_err("0 must fail");
        assert_eq!(err.constraint_type, ConstraintType::Range);
        assert!(err.reason.contains("below"));
    }

    #[test]
    fn test_validate_null_passes_any_rule() {
        let v = ConstraintValidator::new(vec![DataColumnConstraint {
            constraint_name: "r".into(),
            constraint_type: ConstraintType::Range,
            value: None,
            min: Some(100.0),
            min_is_inclusive: Some(true),
            max: Some(200.0),
            max_is_inclusive: Some(true),
            description: None,
        }]);
        assert!(v.validate("r", &CellValue::Null).is_ok());
    }

    #[test]
    fn test_validate_unknown_name_is_permissive() {
        let v = ConstraintValidator::new(Vec::new());
        assert!(
            v.validate("nothing_registered", &CellValue::Integer(42))
                .is_ok()
        );
    }

    #[test]
    fn test_validator_len_and_is_empty() {
        let empty = ConstraintValidator::new(Vec::new());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let one = ConstraintValidator::new(vec![DataColumnConstraint {
            constraint_name: "x".into(),
            constraint_type: ConstraintType::Enum,
            value: Some("a".into()),
            min: None,
            min_is_inclusive: None,
            max: None,
            max_is_inclusive: None,
            description: None,
        }]);
        assert!(!one.is_empty());
        assert_eq!(one.len(), 1);
        assert!(one.contains("x"));
        assert!(!one.contains("y"));
    }
}
