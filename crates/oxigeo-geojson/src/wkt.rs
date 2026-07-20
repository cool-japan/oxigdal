//! WKT (Well-Known Text) serialization and deserialization for [`GeoJsonGeometry`].
//!
//! Implements OGC Simple Features geometry text encoding for PostGIS / GeoPackage
//! interoperability.  Supports all 14 [`GeoJsonGeometry`] variants including the
//! ISO WKT `Z` dimensional qualifier (`POINT Z (x y z)`).
//!
//! # Round-trip guarantee
//!
//! Any geometry produced by [`geometry_to_wkt`] is accepted by
//! [`geometry_from_wkt`] and returns an `==` geometry.
//!
//! # Examples
//!
//! ```rust
//! use oxigeo_geojson_stream::{GeoJsonGeometry, geometry_to_wkt, geometry_from_wkt};
//!
//! let g = GeoJsonGeometry::Point([1.5, 2.5]);
//! let wkt = geometry_to_wkt(&g);
//! assert_eq!(wkt, "POINT(1.5 2.5)");
//!
//! let back = geometry_from_wkt(&wkt).unwrap();
//! assert_eq!(back, g);
//! ```

use crate::{GeoJsonError, GeoJsonGeometry};

// ─────────────────────────────────────────────────────────────────────────────
// Serializer
// ─────────────────────────────────────────────────────────────────────────────

/// Serialize a [`GeoJsonGeometry`] to an OGC WKT string.
///
/// The output conforms to ISO 19125 Simple Features WKT with the ISO `Z` tag
/// for 3-D geometries (`POINT Z (x y z)`, etc.).  Multi-point sub-geometries
/// use the modern parenthesised form (`MULTIPOINT((x y), (x y))`).
/// `Null` maps to `GEOMETRYCOLLECTION EMPTY`.
#[must_use]
pub fn geometry_to_wkt(g: &GeoJsonGeometry) -> String {
    let mut out = String::with_capacity(128);
    write_geometry(g, &mut out);
    out
}

fn write_geometry(g: &GeoJsonGeometry, out: &mut String) {
    match g {
        GeoJsonGeometry::Point([x, y]) => {
            out.push_str("POINT(");
            write_coord_2d(*x, *y, out);
            out.push(')');
        }
        GeoJsonGeometry::PointZ([x, y, z]) => {
            out.push_str("POINT Z (");
            write_coord_3d(*x, *y, *z, out);
            out.push(')');
        }
        GeoJsonGeometry::LineString(pts) => {
            out.push_str("LINESTRING");
            if pts.is_empty() {
                out.push_str(" EMPTY");
            } else {
                out.push('(');
                write_coords_2d(pts, out);
                out.push(')');
            }
        }
        GeoJsonGeometry::LineStringZ(pts) => {
            out.push_str("LINESTRING Z ");
            if pts.is_empty() {
                out.push_str("EMPTY");
            } else {
                out.push('(');
                write_coords_3d(pts, out);
                out.push(')');
            }
        }
        GeoJsonGeometry::Polygon(rings) => {
            out.push_str("POLYGON");
            if rings.is_empty() {
                out.push_str(" EMPTY");
            } else {
                out.push('(');
                for (i, ring) in rings.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    write_coords_2d(ring, out);
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::PolygonZ(rings) => {
            out.push_str("POLYGON Z ");
            if rings.is_empty() {
                out.push_str("EMPTY");
            } else {
                out.push('(');
                for (i, ring) in rings.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    write_coords_3d(ring, out);
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::MultiPoint(pts) => {
            out.push_str("MULTIPOINT");
            if pts.is_empty() {
                out.push_str(" EMPTY");
            } else {
                out.push('(');
                for (i, [x, y]) in pts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    write_coord_2d(*x, *y, out);
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::MultiPointZ(pts) => {
            out.push_str("MULTIPOINT Z ");
            if pts.is_empty() {
                out.push_str("EMPTY");
            } else {
                out.push('(');
                for (i, [x, y, z]) in pts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    write_coord_3d(*x, *y, *z, out);
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::MultiLineString(lines) => {
            out.push_str("MULTILINESTRING");
            if lines.is_empty() {
                out.push_str(" EMPTY");
            } else {
                out.push('(');
                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    write_coords_2d(line, out);
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::MultiLineStringZ(lines) => {
            out.push_str("MULTILINESTRING Z ");
            if lines.is_empty() {
                out.push_str("EMPTY");
            } else {
                out.push('(');
                for (i, line) in lines.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    write_coords_3d(line, out);
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::MultiPolygon(polys) => {
            out.push_str("MULTIPOLYGON");
            if polys.is_empty() {
                out.push_str(" EMPTY");
            } else {
                out.push('(');
                for (i, poly) in polys.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    for (j, ring) in poly.iter().enumerate() {
                        if j > 0 {
                            out.push_str(", ");
                        }
                        out.push('(');
                        write_coords_2d(ring, out);
                        out.push(')');
                    }
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::MultiPolygonZ(polys) => {
            out.push_str("MULTIPOLYGON Z ");
            if polys.is_empty() {
                out.push_str("EMPTY");
            } else {
                out.push('(');
                for (i, poly) in polys.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('(');
                    for (j, ring) in poly.iter().enumerate() {
                        if j > 0 {
                            out.push_str(", ");
                        }
                        out.push('(');
                        write_coords_3d(ring, out);
                        out.push(')');
                    }
                    out.push(')');
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::GeometryCollection(geoms) => {
            out.push_str("GEOMETRYCOLLECTION");
            if geoms.is_empty() {
                out.push_str(" EMPTY");
            } else {
                out.push('(');
                for (i, g) in geoms.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_geometry(g, out);
                }
                out.push(')');
            }
        }
        GeoJsonGeometry::Null => {
            out.push_str("GEOMETRYCOLLECTION EMPTY");
        }
    }
}

/// Write a single 2-D coordinate `x y`.
#[inline]
fn write_coord_2d(x: f64, y: f64, out: &mut String) {
    out.push_str(&format!("{x} {y}"));
}

/// Write a single 3-D coordinate `x y z`.
#[inline]
fn write_coord_3d(x: f64, y: f64, z: f64, out: &mut String) {
    out.push_str(&format!("{x} {y} {z}"));
}

/// Write a comma-separated sequence of 2-D coordinates.
fn write_coords_2d(pts: &[[f64; 2]], out: &mut String) {
    for (i, [x, y]) in pts.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_coord_2d(*x, *y, out);
    }
}

/// Write a comma-separated sequence of 3-D coordinates.
fn write_coords_3d(pts: &[[f64; 3]], out: &mut String) {
    for (i, [x, y, z]) in pts.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_coord_3d(*x, *y, *z, out);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tokenizer
// ─────────────────────────────────────────────────────────────────────────────

/// A single WKT token.
#[derive(Debug, PartialEq, Clone)]
enum Token<'a> {
    /// Identifier/keyword: `POINT`, `LINESTRING`, `Z`, `EMPTY`, …
    Word(&'a str),
    /// Floating-point number.
    Number(f64),
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
}

/// Lazy tokenizer over a borrowed WKT string.
struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    /// Return the next token without consuming it, or `None` at end-of-input.
    fn peek(&mut self) -> Result<Option<Token<'a>>, GeoJsonError> {
        let saved = self.pos;
        let tok = self.next_token()?;
        self.pos = saved;
        Ok(tok)
    }

    /// Consume and return the next token, or `None` at end-of-input.
    fn next_token(&mut self) -> Result<Option<Token<'a>>, GeoJsonError> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Ok(None);
        }
        let ch = self.current_byte();
        match ch {
            b'(' => {
                self.pos += 1;
                Ok(Some(Token::LParen))
            }
            b')' => {
                self.pos += 1;
                Ok(Some(Token::RParen))
            }
            b',' => {
                self.pos += 1;
                Ok(Some(Token::Comma))
            }
            b'-' | b'0'..=b'9' => self.scan_number(),
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => Ok(Some(self.scan_word())),
            other => Err(GeoJsonError::WktParseError(format!(
                "Unexpected character '{}' at position {}",
                other as char, self.pos
            ))),
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len()
            && matches!(self.current_byte(), b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    #[inline]
    fn current_byte(&self) -> u8 {
        self.input.as_bytes()[self.pos]
    }

    /// Scan a floating-point number token.  Handles sign, digits, decimal
    /// point, and exponent notation.
    fn scan_number(&mut self) -> Result<Option<Token<'a>>, GeoJsonError> {
        let start = self.pos;
        // Optional leading minus sign
        if self.pos < self.input.len() && self.current_byte() == b'-' {
            self.pos += 1;
        }
        // Integer part
        while self.pos < self.input.len() && self.current_byte().is_ascii_digit() {
            self.pos += 1;
        }
        // Optional decimal fraction
        if self.pos < self.input.len() && self.current_byte() == b'.' {
            self.pos += 1;
            while self.pos < self.input.len() && self.current_byte().is_ascii_digit() {
                self.pos += 1;
            }
        }
        // Optional exponent
        if self.pos < self.input.len() && matches!(self.current_byte(), b'e' | b'E') {
            self.pos += 1;
            if self.pos < self.input.len() && matches!(self.current_byte(), b'+' | b'-') {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.current_byte().is_ascii_digit() {
                self.pos += 1;
            }
        }
        let raw = &self.input[start..self.pos];
        let value: f64 = raw
            .parse()
            .map_err(|_| GeoJsonError::WktParseError(format!("Cannot parse number: '{raw}'")))?;
        // Reject non-finite values
        if !value.is_finite() {
            return Err(GeoJsonError::WktParseError(format!(
                "Non-finite coordinate value: '{raw}'"
            )));
        }
        Ok(Some(Token::Number(value)))
    }

    /// Scan an identifier/keyword (letters, digits, underscore).
    fn scan_word(&mut self) -> Token<'a> {
        let start = self.pos;
        while self.pos < self.input.len() {
            let b = self.current_byte();
            if b.is_ascii_alphabetic() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        Token::Word(&self.input[start..self.pos])
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Parser
// ─────────────────────────────────────────────────────────────────────────────

/// Deserialize a WKT string into a [`GeoJsonGeometry`].
///
/// Supports both ISO WKT `Z` qualifier and legacy bare coordinates.  Both the
/// modern `MULTIPOINT((x y),(x y))` and legacy `MULTIPOINT(x y, x y)` forms
/// are accepted.
///
/// Returns [`GeoJsonError::WktParseError`] for any syntactic or semantic error,
/// including non-finite coordinate values.
pub fn geometry_from_wkt(s: &str) -> Result<GeoJsonGeometry, GeoJsonError> {
    let mut tok = Tokenizer::new(s);
    let g = parse_geometry(&mut tok)?;
    // Ensure no trailing garbage
    tok.skip_whitespace();
    if let Some(extra) = tok.next_token()? {
        return Err(GeoJsonError::WktParseError(format!(
            "Unexpected trailing token: {extra:?}"
        )));
    }
    Ok(g)
}

/// Parse a single geometry (may be called recursively for collections).
fn parse_geometry(tok: &mut Tokenizer<'_>) -> Result<GeoJsonGeometry, GeoJsonError> {
    let type_word = expect_word(tok, "geometry type keyword")?;
    let type_upper = type_word.to_ascii_uppercase();

    // ISO WKT allows an optional "Z" qualifier after the geometry type.
    let is_3d = match tok.peek()? {
        Some(Token::Word(w)) if w.eq_ignore_ascii_case("Z") => {
            tok.next_token()?; // consume the "Z"
            true
        }
        _ => false,
    };

    // Check for EMPTY
    if let Some(Token::Word(w)) = tok.peek()?
        && w.eq_ignore_ascii_case("EMPTY")
    {
        tok.next_token()?; // consume "EMPTY"
        return empty_for_type(&type_upper, is_3d);
    }

    // Dispatch on geometry type
    match type_upper.as_str() {
        "POINT" => parse_point(tok, is_3d),
        "LINESTRING" => parse_linestring(tok, is_3d),
        "POLYGON" => parse_polygon(tok, is_3d),
        "MULTIPOINT" => parse_multipoint(tok, is_3d),
        "MULTILINESTRING" => parse_multilinestring(tok, is_3d),
        "MULTIPOLYGON" => parse_multipolygon(tok, is_3d),
        "GEOMETRYCOLLECTION" => parse_geometrycollection(tok),
        other => Err(GeoJsonError::WktParseError(format!(
            "Unknown geometry type: '{other}'"
        ))),
    }
}

/// Return the appropriate empty geometry for a given type keyword.
fn empty_for_type(type_upper: &str, is_3d: bool) -> Result<GeoJsonGeometry, GeoJsonError> {
    match type_upper {
        "POINT" => {
            // WKT "POINT EMPTY" has no standard representation in RFC 7946;
            // we map it to an empty collection to avoid data loss.
            Ok(GeoJsonGeometry::GeometryCollection(vec![]))
        }
        "LINESTRING" => {
            if is_3d {
                Ok(GeoJsonGeometry::LineStringZ(vec![]))
            } else {
                Ok(GeoJsonGeometry::LineString(vec![]))
            }
        }
        "POLYGON" => {
            if is_3d {
                Ok(GeoJsonGeometry::PolygonZ(vec![]))
            } else {
                Ok(GeoJsonGeometry::Polygon(vec![]))
            }
        }
        "MULTIPOINT" => {
            if is_3d {
                Ok(GeoJsonGeometry::MultiPointZ(vec![]))
            } else {
                Ok(GeoJsonGeometry::MultiPoint(vec![]))
            }
        }
        "MULTILINESTRING" => {
            if is_3d {
                Ok(GeoJsonGeometry::MultiLineStringZ(vec![]))
            } else {
                Ok(GeoJsonGeometry::MultiLineString(vec![]))
            }
        }
        "MULTIPOLYGON" => {
            if is_3d {
                Ok(GeoJsonGeometry::MultiPolygonZ(vec![]))
            } else {
                Ok(GeoJsonGeometry::MultiPolygon(vec![]))
            }
        }
        "GEOMETRYCOLLECTION" => Ok(GeoJsonGeometry::GeometryCollection(vec![])),
        other => Err(GeoJsonError::WktParseError(format!(
            "Unknown geometry type for EMPTY: '{other}'"
        ))),
    }
}

// ─── Point ───────────────────────────────────────────────────────────────────

fn parse_point(tok: &mut Tokenizer<'_>, is_3d: bool) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;
    if is_3d {
        let coord = parse_coord_3d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::PointZ(coord))
    } else {
        let coord = parse_coord_2d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::Point(coord))
    }
}

// ─── LineString ───────────────────────────────────────────────────────────────

fn parse_linestring(tok: &mut Tokenizer<'_>, is_3d: bool) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;
    if is_3d {
        let pts = parse_coord_list_3d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::LineStringZ(pts))
    } else {
        let pts = parse_coord_list_2d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::LineString(pts))
    }
}

// ─── Polygon ─────────────────────────────────────────────────────────────────

fn parse_polygon(tok: &mut Tokenizer<'_>, is_3d: bool) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;
    if is_3d {
        let rings = parse_ring_list_3d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::PolygonZ(rings))
    } else {
        let rings = parse_ring_list_2d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::Polygon(rings))
    }
}

/// Parse one or more rings, each enclosed in `(...)`, separated by commas.
fn parse_ring_list_2d(tok: &mut Tokenizer<'_>) -> Result<Vec<Vec<[f64; 2]>>, GeoJsonError> {
    let mut rings = vec![parse_ring_2d(tok)?];
    while let Some(Token::Comma) = tok.peek()? {
        tok.next_token()?;
        rings.push(parse_ring_2d(tok)?);
    }
    Ok(rings)
}

fn parse_ring_list_3d(tok: &mut Tokenizer<'_>) -> Result<Vec<Vec<[f64; 3]>>, GeoJsonError> {
    let mut rings = vec![parse_ring_3d(tok)?];
    while let Some(Token::Comma) = tok.peek()? {
        tok.next_token()?;
        rings.push(parse_ring_3d(tok)?);
    }
    Ok(rings)
}

/// Parse a parenthesised ring: `(x y, x y, ...)`.
fn parse_ring_2d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 2]>, GeoJsonError> {
    expect_lparen(tok)?;
    let pts = parse_coord_list_2d(tok)?;
    expect_rparen(tok)?;
    Ok(pts)
}

fn parse_ring_3d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 3]>, GeoJsonError> {
    expect_lparen(tok)?;
    let pts = parse_coord_list_3d(tok)?;
    expect_rparen(tok)?;
    Ok(pts)
}

// ─── MultiPoint ───────────────────────────────────────────────────────────────

/// Parse MULTIPOINT — supports both modern `(x y),(x y)` and legacy `x y, x y` forms.
fn parse_multipoint(tok: &mut Tokenizer<'_>, is_3d: bool) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;

    // Peek at the first token after `(` to determine form:
    //   - `(` → modern parenthesised sub-points
    //   - Number → legacy bare coordinates
    let modern_form = matches!(tok.peek()?, Some(Token::LParen));

    if is_3d {
        let pts = if modern_form {
            parse_multipoint_modern_3d(tok)?
        } else {
            parse_multipoint_legacy_3d(tok)?
        };
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::MultiPointZ(pts))
    } else {
        let pts = if modern_form {
            parse_multipoint_modern_2d(tok)?
        } else {
            parse_multipoint_legacy_2d(tok)?
        };
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::MultiPoint(pts))
    }
}

/// Modern form: each sub-point wrapped in parentheses.
fn parse_multipoint_modern_2d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 2]>, GeoJsonError> {
    let mut pts = Vec::new();
    loop {
        match tok.peek()? {
            Some(Token::LParen) => {
                tok.next_token()?; // consume `(`
                pts.push(parse_coord_2d(tok)?);
                expect_rparen(tok)?;
            }
            Some(Token::RParen) => break, // end of MULTIPOINT
            Some(Token::Comma) => {
                tok.next_token()?; // consume `,`
            }
            other => {
                return Err(GeoJsonError::WktParseError(format!(
                    "Expected '(' or ')' in MULTIPOINT modern form, got {other:?}"
                )));
            }
        }
    }
    Ok(pts)
}

fn parse_multipoint_modern_3d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 3]>, GeoJsonError> {
    let mut pts = Vec::new();
    loop {
        match tok.peek()? {
            Some(Token::LParen) => {
                tok.next_token()?;
                pts.push(parse_coord_3d(tok)?);
                expect_rparen(tok)?;
            }
            Some(Token::RParen) => break,
            Some(Token::Comma) => {
                tok.next_token()?;
            }
            other => {
                return Err(GeoJsonError::WktParseError(format!(
                    "Expected '(' or ')' in MULTIPOINT Z modern form, got {other:?}"
                )));
            }
        }
    }
    Ok(pts)
}

/// Legacy form: bare `x y` coordinates separated by commas.
fn parse_multipoint_legacy_2d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 2]>, GeoJsonError> {
    parse_coord_list_2d(tok)
}

fn parse_multipoint_legacy_3d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 3]>, GeoJsonError> {
    parse_coord_list_3d(tok)
}

// ─── MultiLineString ──────────────────────────────────────────────────────────

fn parse_multilinestring(
    tok: &mut Tokenizer<'_>,
    is_3d: bool,
) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;
    if is_3d {
        let lines = parse_ring_list_3d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::MultiLineStringZ(lines))
    } else {
        let lines = parse_ring_list_2d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::MultiLineString(lines))
    }
}

// ─── MultiPolygon ─────────────────────────────────────────────────────────────

fn parse_multipolygon(
    tok: &mut Tokenizer<'_>,
    is_3d: bool,
) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;
    if is_3d {
        let polys = parse_multipolygon_inner_3d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::MultiPolygonZ(polys))
    } else {
        let polys = parse_multipolygon_inner_2d(tok)?;
        expect_rparen(tok)?;
        Ok(GeoJsonGeometry::MultiPolygon(polys))
    }
}

/// Parse a list of polygon ring-groups: `((ring), (ring)), ((ring))`.
fn parse_multipolygon_inner_2d(
    tok: &mut Tokenizer<'_>,
) -> Result<Vec<Vec<Vec<[f64; 2]>>>, GeoJsonError> {
    let mut polys = Vec::new();
    // Each polygon begins with `(`
    loop {
        match tok.peek()? {
            Some(Token::LParen) => {
                tok.next_token()?; // consume opening `(` of polygon group
                let rings = parse_ring_list_2d(tok)?;
                expect_rparen(tok)?;
                polys.push(rings);
            }
            Some(Token::RParen) => break,
            Some(Token::Comma) => {
                tok.next_token()?;
            }
            other => {
                return Err(GeoJsonError::WktParseError(format!(
                    "Expected '(' or ')' in MULTIPOLYGON, got {other:?}"
                )));
            }
        }
    }
    Ok(polys)
}

fn parse_multipolygon_inner_3d(
    tok: &mut Tokenizer<'_>,
) -> Result<Vec<Vec<Vec<[f64; 3]>>>, GeoJsonError> {
    let mut polys = Vec::new();
    loop {
        match tok.peek()? {
            Some(Token::LParen) => {
                tok.next_token()?;
                let rings = parse_ring_list_3d(tok)?;
                expect_rparen(tok)?;
                polys.push(rings);
            }
            Some(Token::RParen) => break,
            Some(Token::Comma) => {
                tok.next_token()?;
            }
            other => {
                return Err(GeoJsonError::WktParseError(format!(
                    "Expected '(' or ')' in MULTIPOLYGON Z, got {other:?}"
                )));
            }
        }
    }
    Ok(polys)
}

// ─── GeometryCollection ───────────────────────────────────────────────────────

fn parse_geometrycollection(tok: &mut Tokenizer<'_>) -> Result<GeoJsonGeometry, GeoJsonError> {
    expect_lparen(tok)?;
    let mut geoms = Vec::new();
    loop {
        match tok.peek()? {
            Some(Token::RParen) => {
                tok.next_token()?;
                break;
            }
            Some(Token::Comma) => {
                tok.next_token()?;
            }
            Some(_) => {
                geoms.push(parse_geometry(tok)?);
            }
            None => {
                return Err(GeoJsonError::WktParseError(
                    "Unexpected end of input inside GEOMETRYCOLLECTION".to_string(),
                ));
            }
        }
    }
    Ok(GeoJsonGeometry::GeometryCollection(geoms))
}

// ─────────────────────────────────────────────────────────────────────────────
// Coordinate list parsers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a comma-separated list of 2-D coordinates, stopping before `)`.
fn parse_coord_list_2d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 2]>, GeoJsonError> {
    let mut coords = Vec::new();
    loop {
        // Stop if we see `)` or end of input
        match tok.peek()? {
            Some(Token::RParen) | None => break,
            Some(Token::Comma) => {
                tok.next_token()?;
            }
            _ => {
                coords.push(parse_coord_2d(tok)?);
            }
        }
    }
    Ok(coords)
}

fn parse_coord_list_3d(tok: &mut Tokenizer<'_>) -> Result<Vec<[f64; 3]>, GeoJsonError> {
    let mut coords = Vec::new();
    loop {
        match tok.peek()? {
            Some(Token::RParen) | None => break,
            Some(Token::Comma) => {
                tok.next_token()?;
            }
            _ => {
                coords.push(parse_coord_3d(tok)?);
            }
        }
    }
    Ok(coords)
}

// ─────────────────────────────────────────────────────────────────────────────
// Individual coordinate parsers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a 2-D coordinate pair: two consecutive number tokens.
fn parse_coord_2d(tok: &mut Tokenizer<'_>) -> Result<[f64; 2], GeoJsonError> {
    let x = expect_number(tok, "x coordinate")?;
    let y = expect_number(tok, "y coordinate")?;
    Ok([x, y])
}

/// Parse a 3-D coordinate triple: three consecutive number tokens.
fn parse_coord_3d(tok: &mut Tokenizer<'_>) -> Result<[f64; 3], GeoJsonError> {
    let x = expect_number(tok, "x coordinate")?;
    let y = expect_number(tok, "y coordinate")?;
    let z = expect_number(tok, "z coordinate")?;
    Ok([x, y, z])
}

// ─────────────────────────────────────────────────────────────────────────────
// Token assertion helpers
// ─────────────────────────────────────────────────────────────────────────────

fn expect_word<'a>(tok: &mut Tokenizer<'a>, context: &str) -> Result<&'a str, GeoJsonError> {
    match tok.next_token()? {
        Some(Token::Word(w)) => Ok(w),
        other => Err(GeoJsonError::WktParseError(format!(
            "Expected {context}, got {other:?}"
        ))),
    }
}

fn expect_number(tok: &mut Tokenizer<'_>, context: &str) -> Result<f64, GeoJsonError> {
    match tok.next_token()? {
        Some(Token::Number(n)) => Ok(n),
        other => Err(GeoJsonError::WktParseError(format!(
            "Expected {context}, got {other:?}"
        ))),
    }
}

fn expect_lparen(tok: &mut Tokenizer<'_>) -> Result<(), GeoJsonError> {
    match tok.next_token()? {
        Some(Token::LParen) => Ok(()),
        other => Err(GeoJsonError::WktParseError(format!(
            "Expected '(', got {other:?}"
        ))),
    }
}

fn expect_rparen(tok: &mut Tokenizer<'_>) -> Result<(), GeoJsonError> {
    match tok.next_token()? {
        Some(Token::RParen) => Ok(()),
        other => Err(GeoJsonError::WktParseError(format!(
            "Expected ')', got {other:?}"
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Round-trip helpers ────────────────────────────────────────────────────

    fn rt(g: &GeoJsonGeometry) -> GeoJsonGeometry {
        let wkt = geometry_to_wkt(g);
        geometry_from_wkt(&wkt).expect("round-trip parse must succeed")
    }

    // ── Basic geometry round-trips ────────────────────────────────────────────

    #[test]
    fn test_wkt_point_roundtrip() {
        let g = GeoJsonGeometry::Point([1.0, 2.0]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "POINT(1 2)");
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_point_z_roundtrip() {
        let g = GeoJsonGeometry::PointZ([1.0, 2.0, 3.0]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "POINT Z (1 2 3)");
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_linestring_roundtrip() {
        let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.5, 2.5], [3.0, 4.0]]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "LINESTRING(0 0, 1.5 2.5, 3 4)");
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_linestring_empty_roundtrip() {
        let g = GeoJsonGeometry::LineString(vec![]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "LINESTRING EMPTY");
        // Parse back — must yield the same empty LineString
        let back = geometry_from_wkt(&wkt).expect("parse LINESTRING EMPTY");
        assert_eq!(back, g);
    }

    #[test]
    fn test_wkt_polygon_with_holes_roundtrip() {
        let outer = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let hole = vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0], [2.0, 2.0]];
        let g = GeoJsonGeometry::Polygon(vec![outer, hole]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(
            wkt,
            "POLYGON((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 8 2, 8 8, 2 8, 2 2))"
        );
        assert_eq!(rt(&g), g);
    }

    // ── MultiPoint ────────────────────────────────────────────────────────────

    #[test]
    fn test_wkt_multipoint_modern_form_parses() {
        // Modern form — both our serializer and a third-party WKT source
        let wkt = "MULTIPOINT((1 2),(3 4))";
        let g = geometry_from_wkt(wkt).expect("parse MULTIPOINT modern form");
        assert_eq!(g, GeoJsonGeometry::MultiPoint(vec![[1.0, 2.0], [3.0, 4.0]]));
        // Round-trip back through our serializer stays consistent
        let wkt2 = geometry_to_wkt(&g);
        let g2 = geometry_from_wkt(&wkt2).expect("re-parse");
        assert_eq!(g, g2);
    }

    #[test]
    fn test_wkt_multipoint_legacy_form_parses() {
        // Legacy form without per-point parentheses
        let wkt = "MULTIPOINT(1 2, 3 4)";
        let g = geometry_from_wkt(wkt).expect("parse MULTIPOINT legacy form");
        assert_eq!(g, GeoJsonGeometry::MultiPoint(vec![[1.0, 2.0], [3.0, 4.0]]));
    }

    // ── MultiPolygon Z ────────────────────────────────────────────────────────

    #[test]
    fn test_wkt_multipolygon_z_roundtrip() {
        let poly_a = vec![vec![
            [0.0, 0.0, 10.0],
            [5.0, 0.0, 10.0],
            [5.0, 5.0, 10.0],
            [0.0, 5.0, 10.0],
            [0.0, 0.0, 10.0],
        ]];
        let poly_b = vec![vec![
            [10.0, 10.0, 20.0],
            [20.0, 10.0, 20.0],
            [20.0, 20.0, 20.0],
            [10.0, 20.0, 20.0],
            [10.0, 10.0, 20.0],
        ]];
        let g = GeoJsonGeometry::MultiPolygonZ(vec![poly_a, poly_b]);
        assert_eq!(rt(&g), g);
    }

    // ── GeometryCollection ────────────────────────────────────────────────────

    #[test]
    fn test_wkt_geometrycollection_mixed_types_roundtrip() {
        let g = GeoJsonGeometry::GeometryCollection(vec![
            GeoJsonGeometry::Point([1.0, 2.0]),
            GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]),
            GeoJsonGeometry::PointZ([3.0, 4.0, 5.0]),
        ]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(
            wkt,
            "GEOMETRYCOLLECTION(POINT(1 2), LINESTRING(0 0, 1 1), POINT Z (3 4 5))"
        );
        assert_eq!(rt(&g), g);
    }

    // ── Null mapping ──────────────────────────────────────────────────────────

    #[test]
    fn test_wkt_null_maps_to_geometrycollection_empty() {
        let g = GeoJsonGeometry::Null;
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "GEOMETRYCOLLECTION EMPTY");
        // Parsing GEOMETRYCOLLECTION EMPTY gives an empty collection, not Null
        let back = geometry_from_wkt(&wkt).expect("parse GEOMETRYCOLLECTION EMPTY");
        assert_eq!(back, GeoJsonGeometry::GeometryCollection(vec![]));
    }

    // ── Error paths ───────────────────────────────────────────────────────────

    #[test]
    fn test_wkt_invalid_tokens_returns_parse_error() {
        // "abc" is not a valid float — the tokenizer produces a Word token
        // where a Number is expected, so it must return an Err.
        let result = geometry_from_wkt("POINT(abc def)");
        assert!(result.is_err(), "Should fail on non-numeric coordinates");
        // Verify the error message mentions WKT by inspecting it via Display
        match result {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("WKT parse error"),
                    "Error should mention WKT: {msg}"
                );
            }
            Ok(_) => unreachable!("expected error"),
        }
    }

    #[test]
    fn test_wkt_unmatched_parens_returns_parse_error() {
        // Missing closing `)`
        let result = geometry_from_wkt("POINT(1 2");
        assert!(result.is_err(), "Should fail on unclosed parenthesis");
    }

    #[test]
    fn test_wkt_nan_rejected() {
        // NaN spelled out would be parsed as a Word, not a Number — so the
        // error message differs from "non-finite" but must still be an error.
        let result = geometry_from_wkt("POINT(NaN 0)");
        assert!(result.is_err(), "Should reject NaN as coordinate");
    }

    // ── Additional coverage ───────────────────────────────────────────────────

    #[test]
    fn test_wkt_linestring_z_roundtrip() {
        let g = GeoJsonGeometry::LineStringZ(vec![
            [0.0, 0.0, 100.0],
            [1.0, 2.0, 200.0],
            [3.0, 4.0, 300.0],
        ]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "LINESTRING Z (0 0 100, 1 2 200, 3 4 300)");
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_multilinestring_roundtrip() {
        let g = GeoJsonGeometry::MultiLineString(vec![
            vec![[0.0, 0.0], [1.0, 1.0]],
            vec![[2.0, 2.0], [3.0, 3.0]],
        ]);
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_multipoint_z_roundtrip() {
        let g = GeoJsonGeometry::MultiPointZ(vec![[1.0, 2.0, 10.0], [3.0, 4.0, 20.0]]);
        let wkt = geometry_to_wkt(&g);
        assert_eq!(wkt, "MULTIPOINT Z ((1 2 10), (3 4 20))");
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_polygon_z_roundtrip() {
        let g = GeoJsonGeometry::PolygonZ(vec![vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 5.0],
            [10.0, 10.0, 5.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 0.0],
        ]]);
        assert_eq!(rt(&g), g);
    }

    #[test]
    fn test_wkt_exponent_notation_parsed() {
        // Scientific notation in WKT coordinates must be handled
        let g = geometry_from_wkt("POINT(1e2 2.5e-1)").expect("parse exponent notation");
        assert_eq!(g, GeoJsonGeometry::Point([100.0, 0.25]));
    }

    #[test]
    fn test_wkt_negative_coords() {
        let g = GeoJsonGeometry::Point([-73.9857, 40.7484]);
        let wkt = geometry_to_wkt(&g);
        let back = geometry_from_wkt(&wkt).expect("parse negative coords");
        assert_eq!(back, g);
    }

    #[test]
    fn test_wkt_unknown_type_returns_error() {
        let result = geometry_from_wkt("TRIANGLE(0 0, 1 0, 0 1)");
        assert!(result.is_err());
    }
}
