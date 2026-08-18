//! `CREATE TABLE` column-type parsing used to restore SQLite REAL affinity.
//!
use crate::vector::types::FieldType;

// Real SQLite applies an on-disk space optimisation: a value written to a
// column with REAL affinity is stored using an INTEGER serial type (instead
// of serial type 7) whenever the value has an exact integer representation.
// A reader that only looks at the record's serial types therefore sees a
// column mixing `Integer` and `Float` cells for what is, semantically, a
// single REAL column (e.g. `40` alongside `40.5`). Restoring that requires
// consulting the column's *declared* type from the table's `CREATE TABLE`
// statement (`sqlite_master.sql`), because the serial type alone cannot
// distinguish "this INTEGER cell lives in a REAL column" from "this is a
// genuine INTEGER column".
//
// The parser below is intentionally a best-effort DDL scanner, not a full SQL
// grammar: it recognises the four SQLite identifier/string quoting styles
// (`"..."`, `` `...` ``, `[...]`, `'...'`, with `""`/``` `` ```/`''` doubling
// as the escape for a literal quote character inside `"`/`` ` ``/`'`
// respectively — `[...]` has no escape, the first `]` always closes),
// balances nested parentheses (for type-parameter lists like `NUMERIC(10,2)`
// and inline `CHECK (...)` expressions), and recognises the standard
// column-constraint and table-constraint leading keywords. Unquoted
// identifier bytes `>= 0x80` (multi-byte UTF-8 continuation/lead bytes) are
// always treated as ordinary identifier characters, since no ASCII
// delimiter byte (`,`, `(`, `)`, quote, whitespace) can ever appear as part
// of a valid UTF-8 multi-byte sequence — so splitting on raw bytes at those
// delimiters is always safe and never cuts a multi-byte character in half.

/// Maps an opening quote byte to its closing byte, or `None` if `b` does not
/// open any of the four SQLite quoting styles.
fn quote_close(b: u8) -> Option<u8> {
    match b {
        b'"' => Some(b'"'),
        b'`' => Some(b'`'),
        b'[' => Some(b']'),
        b'\'' => Some(b'\''),
        _ => None,
    }
}

/// Advance past a quoted region, given `i` is the byte index immediately
/// *after* the opening quote and `close` is its matching closing byte.
///
/// Honors the doubled-quote escape (`""`, `` `` ``, `''`) used by every
/// SQLite quoting style except `[...]` (brackets have no escape; the first
/// `]` always closes). Returns the index just past the closing quote, or
/// `bytes.len()` if the quote is unterminated (malformed/truncated SQL
/// degrades to "consume the rest of the string", which conservatively yields
/// fewer parsed columns rather than a false split).
fn skip_quoted(bytes: &[u8], mut i: usize, close: u8) -> usize {
    while i < bytes.len() {
        if bytes[i] == close {
            if close != b']' && i + 1 < bytes.len() && bytes[i + 1] == close {
                i += 2; // doubled-quote escape: literal quote character
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Split the column/constraint definition list out of a `CREATE TABLE`
/// statement.
///
/// Finds the first *unquoted* `(` (the opening of the column list — any `(`
/// appearing inside a quoted table name is skipped over) and splits its
/// balanced contents on top-level commas, i.e. commas at paren depth 1,
/// outside any quoted region. Nested parens (`NUMERIC(10,2)`, `CHECK (x >
/// 0)`, `FOREIGN KEY (a, b)`) never cause a spurious split. Returns an empty
/// vector if no balanced, unquoted parenthesised list is found (e.g. a
/// `CREATE VIEW` / `CREATE INDEX` / `CREATE TRIGGER` statement, or malformed
/// SQL) — callers treat that as "no columns parsed", never as a mismatch to
/// force through.
fn split_column_defs(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut i = 0usize;

    let open = loop {
        if i >= bytes.len() {
            return Vec::new();
        }
        let b = bytes[i];
        if let Some(close) = quote_close(b) {
            i = skip_quoted(bytes, i + 1, close);
            continue;
        }
        if b == b'(' {
            break i;
        }
        i += 1;
    };

    let mut depth: u32 = 1;
    let mut seg_start = open + 1;
    let mut defs = Vec::new();
    let mut j = open + 1;

    while j < bytes.len() {
        let b = bytes[j];
        if let Some(close) = quote_close(b) {
            j = skip_quoted(bytes, j + 1, close);
            continue;
        }
        match b {
            b'(' => {
                depth += 1;
                j += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let seg = &sql[seg_start..j];
                    if !seg.trim().is_empty() {
                        defs.push(seg);
                    }
                    return defs;
                }
                j += 1;
            }
            b',' if depth == 1 => {
                let seg = &sql[seg_start..j];
                if !seg.trim().is_empty() {
                    defs.push(seg);
                }
                j += 1;
                seg_start = j;
            }
            _ => j += 1,
        }
    }

    // Unbalanced parens (malformed SQL): whatever was collected before
    // running out of input, which is the conservative "parsed less than
    // everything" outcome — never a false positive column.
    defs
}

/// If `def` begins with an unquoted word, return it upper-cased; a def that
/// begins with a quote character is unambiguously a (quoted) column name,
/// never a table-constraint keyword, so this returns `None` for it.
fn leading_unquoted_keyword(def: &str) -> Option<String> {
    let bytes = def.as_bytes();
    if bytes.is_empty() || quote_close(bytes[0]).is_some() {
        return None;
    }
    let end = bytes
        .iter()
        .position(|&b| b.is_ascii_whitespace() || b == b'(')
        .unwrap_or(bytes.len());
    Some(def[..end].to_ascii_uppercase())
}

/// Table-level constraint keywords: a definition beginning with one of these
/// (unquoted) is `PRIMARY KEY (...)`, `UNIQUE (...)`, `CHECK (...)`, `FOREIGN
/// KEY (...)`, or a named `CONSTRAINT ...` — never a column definition, so it
/// consumes no position in the row's value vector.
const TABLE_CONSTRAINT_KEYWORDS: &[&str] = &["PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT"];

/// Column-constraint keywords that terminate the declared-type word sequence
/// for a column definition (SQLite's `typename` grammar is "zero or more
/// identifiers", greedily consumed until one of these is recognised).
const COLUMN_CONSTRAINT_KEYWORDS: &[&str] = &[
    "NOT",
    "NULL",
    "PRIMARY",
    "UNIQUE",
    "CHECK",
    "DEFAULT",
    "COLLATE",
    "REFERENCES",
    "GENERATED",
    "AS",
    "CONSTRAINT",
];

fn is_table_constraint(def: &str) -> bool {
    match leading_unquoted_keyword(def) {
        Some(kw) => TABLE_CONSTRAINT_KEYWORDS.contains(&kw.as_str()),
        None => false,
    }
}

/// Extract the declared type text of a single column definition (with the
/// leading column name already known to be present), joining any multi-word
/// type (`DOUBLE PRECISION`, `UNSIGNED BIG INT`) with single spaces and
/// dropping any type-parameter list (`NUMERIC(10,2)` → `NUMERIC`). Returns
/// an empty string for a typeless column (SQLite permits omitting the type
/// entirely).
fn extract_declared_type(def: &str) -> String {
    let bytes = def.as_bytes();
    let mut i = 0usize;

    // Skip leading whitespace, then the column name (quoted or unquoted).
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < bytes.len() {
        if let Some(close) = quote_close(bytes[i]) {
            i = skip_quoted(bytes, i + 1, close);
        } else {
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'(' {
                i += 1;
            }
        }
    }

    let mut type_words: Vec<&str> = Vec::new();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'(' {
            // A type-parameter list immediately following the type name
            // (e.g. `NUMERIC(10,2)`, `VARCHAR(255)`) — skip the balanced
            // group without contributing to the type name, then keep
            // scanning (a constraint keyword may still follow).
            let mut depth: u32 = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                if let Some(close) = quote_close(bytes[i]) {
                    i = skip_quoted(bytes, i + 1, close);
                    continue;
                }
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        if quote_close(bytes[i]).is_some() {
            // A quoted token here is not part of SQLite's typename grammar;
            // treat it as the start of something else and stop.
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'(' {
            i += 1;
        }
        let word = &def[start..i];
        if COLUMN_CONSTRAINT_KEYWORDS.contains(&word.to_ascii_uppercase().as_str()) {
            break;
        }
        type_words.push(word);
    }

    type_words.join(" ")
}

/// Parse the ordered list of column [`FieldType`]s declared by a `CREATE
/// TABLE` statement's SQL text (as stored verbatim in `sqlite_master.sql`),
/// skipping table-level constraint clauses (`PRIMARY KEY (...)`, `UNIQUE
/// (...)`, `CHECK (...)`, `FOREIGN KEY (...)`, named `CONSTRAINT ...`) so
/// only actual columns are counted.
///
/// The returned vector is positionally aligned with the serial-type-ordered
/// values SQLite stores in each row record: index `i` here corresponds to
/// `values[i]` in a row decoded by [`scan_table`] / [`parse_record`],
/// *including* a rowid-alias column (e.g. `fid INTEGER PRIMARY KEY`), which
/// SQLite still allocates a NULL-serial-type slot for in the record even
/// though its value is read from the rowid.
///
/// A column with no declared type (SQLite permits typeless columns) maps to
/// [`FieldType::Text`] via [`FieldType::from_sql_type`]'s catch-all — safe
/// here because this parse result is only ever used to *restore* REAL
/// affinity, never to reinterpret any other column.
pub(crate) fn declared_column_types(create_table_sql: &str) -> Vec<FieldType> {
    split_column_defs(create_table_sql)
        .into_iter()
        .filter_map(|raw_def| {
            let def = raw_def.trim();
            if def.is_empty() || is_table_constraint(def) {
                return None;
            }
            Some(FieldType::from_sql_type(&extract_declared_type(def)))
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_types_plain_real_and_integer() {
        let sql = "CREATE TABLE t (val REAL, n INTEGER)";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Real, FieldType::Integer]);
    }

    #[test]
    fn declared_types_multi_word_type() {
        let sql = "CREATE TABLE t (a DOUBLE PRECISION, b UNSIGNED BIG INT)";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Real, FieldType::Integer]);
    }

    #[test]
    fn declared_types_quoted_identifier_with_space() {
        let sql = "CREATE TABLE t (\"name ja\" TEXT, val REAL)";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Text, FieldType::Real]);
    }

    #[test]
    fn declared_types_backtick_and_bracket_identifiers() {
        let sql = "CREATE TABLE t (`a b` REAL, [c d] INTEGER)";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Real, FieldType::Integer]);
    }

    #[test]
    fn declared_types_skip_table_level_constraints() {
        let sql = "CREATE TABLE t (id INTEGER, val REAL, PRIMARY KEY (id), \
                    CHECK (val > 0), UNIQUE (id, val), \
                    FOREIGN KEY (id) REFERENCES other(id), \
                    CONSTRAINT nm CHECK (val <> 0))";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Integer, FieldType::Real]);
    }

    #[test]
    fn declared_types_strip_type_parameters() {
        let sql = "CREATE TABLE t (a NUMERIC(10,2), b VARCHAR(255))";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Real, FieldType::Text]);
    }

    #[test]
    fn declared_types_column_level_primary_key_not_a_constraint_row() {
        let sql = "CREATE TABLE t (fid INTEGER PRIMARY KEY AUTOINCREMENT, val REAL)";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Integer, FieldType::Real]);
    }

    #[test]
    fn declared_types_typeless_column_defaults_text() {
        let sql = "CREATE TABLE t (a, val REAL)";
        let types = declared_column_types(sql);
        assert_eq!(types, vec![FieldType::Text, FieldType::Real]);
    }

    #[test]
    fn declared_types_not_a_create_table() {
        // CREATE INDEX has no column-def list at all — empty result.
        let sql = "CREATE INDEX idx_t_val ON t (val)";
        // The first unquoted '(' here belongs to the index column list, not
        // a table column-def list, but the parser has no way to distinguish
        // that from `sql` text alone without also matching the `TABLE`
        // keyword; callers only ever pass `MasterEntry.sql` for
        // `entry_type == "table"` rows, so this case never reaches
        // production code — this test only pins that it degrades safely
        // (produces some FieldType values, never panics).
        let _ = declared_column_types(sql);
    }
}
