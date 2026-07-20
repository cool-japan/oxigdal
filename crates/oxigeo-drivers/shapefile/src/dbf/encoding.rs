//! Character-encoding resolution for DBF string fields.
//!
//! DBF tables predate Unicode, so a `.dbf`'s `Character` columns are stored in
//! whatever single- or multi-byte code page the authoring software used. The
//! encoding can be declared two ways:
//!
//! 1. a sibling `.cpg` file containing a free-text label (`UTF-8`, `1252`,
//!    `ISO-8859-1`, `TIS-620`, `GBK`, …), or
//! 2. the DBF header's "language driver" (LDID) byte at offset 29.
//!
//! This module maps both forms onto an [`encoding_rs::Encoding`], which the
//! reader then uses to transcode field bytes to UTF-8 `String`s. Encodings
//! outside the WHATWG set supported by `encoding_rs` (e.g. legacy DOS code
//! pages such as cp437/cp850) resolve to `None`; the caller falls back to a
//! UTF-8 lossy decode, preserving the previous behaviour.

use encoding_rs::Encoding;

/// The default encoding when nothing is declared: UTF-8.
pub const DEFAULT: &Encoding = encoding_rs::UTF_8;

/// Resolve a `.cpg` label (free text) to an encoding.
///
/// Returns `None` for an empty/unknown label so the caller can fall back.
pub fn resolve_cpg(label: &str) -> Option<&'static Encoding> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return None;
    }

    // A bare code-page number (optionally prefixed by "CP" or "LDID/", e.g.
    // "1252", "CP1252", "LDID/65001") maps onto a WHATWG label. We only take
    // this branch when the remainder is *entirely* digits, so textual labels
    // like "utf8" are left for `Encoding::for_label`.
    let upper = trimmed.to_ascii_uppercase();
    let mut number_part = upper.as_str();
    number_part = number_part.strip_prefix("LDID/").unwrap_or(number_part);
    number_part = number_part.strip_prefix("CP").unwrap_or(number_part);
    if !number_part.is_empty() && number_part.bytes().all(|b| b.is_ascii_digit()) {
        return number_part
            .parse::<u32>()
            .ok()
            .and_then(label_for_codepage)
            .and_then(|label| Encoding::for_label(label.as_bytes()));
    }

    // Otherwise let encoding_rs match its rich set of textual labels
    // (utf-8, windows-1252, iso-8859-1, shift_jis, gbk, big5, euc-kr, …).
    Encoding::for_label(trimmed.as_bytes())
}

/// Resolve a DBF header language-driver (LDID) byte to an encoding.
pub fn resolve_ldid(code_page: u8) -> Option<&'static Encoding> {
    let label = match code_page {
        0x03 | 0x57 | 0x58 | 0x59 => "windows-1252", // ANSI / Western European
        0x65 => "ibm866",                            // Cyrillic (DOS Russian)
        0x78 => "big5",                              // Traditional Chinese (cp950)
        0x79 => "euc-kr",                            // Korean (cp949)
        0x7a => "gbk",                               // Simplified Chinese (cp936)
        0x7b => "shift_jis",                         // Japanese (cp932)
        0x7c => "windows-874",                       // Thai
        0x7d => "windows-1255",                      // Hebrew
        0x7e => "windows-1256",                      // Arabic
        0xc8 => "windows-1250",                      // Eastern European
        0xc9 => "windows-1251",                      // Russian
        0xca => "windows-1254",                      // Turkish
        0xcb => "windows-1253",                      // Greek
        0xcc => "windows-1257",                      // Baltic
        _ => return None,
    };
    Encoding::for_label(label.as_bytes())
}

/// Map a numeric code-page identifier to a WHATWG label `encoding_rs` accepts.
fn label_for_codepage(cp: u32) -> Option<String> {
    let label = match cp {
        65001 => "utf-8",
        1200 => "utf-16le",
        1201 => "utf-16be",
        20127 => "ascii",
        874 => "windows-874",
        932 => "shift_jis",
        936 => "gbk",
        949 => "euc-kr",
        950 => "big5",
        866 => "ibm866",
        1250..=1258 => return Some(format!("windows-{cp}")),
        28591..=28599 => return Some(format!("iso-8859-{}", cp - 28590)),
        28603 => "iso-8859-13",
        28605 => "iso-8859-15",
        _ => return None,
    };
    Some(label.to_string())
}

/// Transcode raw DBF field bytes to a UTF-8 `String` using `encoding`.
///
/// Invalid sequences are replaced (lossy), matching the previous fallback for
/// the UTF-8 path. BOM handling is disabled so a leading BOM-looking byte in a
/// fixed-width field is treated as data, not stripped.
pub fn decode(bytes: &[u8], encoding: &'static Encoding) -> String {
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn cpg_textual_labels() {
        assert_eq!(resolve_cpg("UTF-8").unwrap(), encoding_rs::UTF_8);
        assert_eq!(resolve_cpg("utf8").unwrap(), encoding_rs::UTF_8);
        assert_eq!(
            resolve_cpg("ISO-8859-1").unwrap(),
            encoding_rs::WINDOWS_1252
        );
        assert_eq!(
            resolve_cpg("windows-1251").unwrap(),
            encoding_rs::WINDOWS_1251
        );
        assert_eq!(resolve_cpg("GBK").unwrap(), encoding_rs::GBK);
    }

    #[test]
    fn cpg_numeric_codepages() {
        assert_eq!(resolve_cpg("65001").unwrap(), encoding_rs::UTF_8);
        assert_eq!(resolve_cpg("1252").unwrap(), encoding_rs::WINDOWS_1252);
        assert_eq!(resolve_cpg("CP1252").unwrap(), encoding_rs::WINDOWS_1252);
        assert_eq!(resolve_cpg("936").unwrap(), encoding_rs::GBK);
        assert_eq!(resolve_cpg("874").unwrap(), encoding_rs::WINDOWS_874);
    }

    #[test]
    fn cpg_empty_or_unknown() {
        assert!(resolve_cpg("").is_none());
        assert!(resolve_cpg("   ").is_none());
        assert!(resolve_cpg("not-an-encoding").is_none());
    }

    #[test]
    fn ldid_bytes() {
        assert_eq!(resolve_ldid(0x57).unwrap(), encoding_rs::WINDOWS_1252);
        assert_eq!(resolve_ldid(0xc9).unwrap(), encoding_rs::WINDOWS_1251);
        assert_eq!(resolve_ldid(0x7c).unwrap(), encoding_rs::WINDOWS_874);
        assert!(resolve_ldid(0x00).is_none());
    }

    #[test]
    fn decode_windows1252() {
        // 0xE9 = 'é' in windows-1252.
        let s = decode(&[0x43, 0x61, 0x66, 0xe9], encoding_rs::WINDOWS_1252);
        assert_eq!(s, "Café");
    }

    #[test]
    fn decode_utf8_passthrough() {
        // Lao "ກ" (U+0E81) as UTF-8 bytes survives a UTF-8 decode.
        let s = decode(&[0xe0, 0xba, 0x81], encoding_rs::UTF_8);
        assert_eq!(s, "ກ");
    }
}
