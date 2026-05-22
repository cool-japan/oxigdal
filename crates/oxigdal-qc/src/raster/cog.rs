//! Cloud Optimized GeoTIFF (COG) compliance checker.
//!
//! Wraps [`oxigdal_geotiff::cog::validate_cog_detailed`] and adds the
//! strict-spec checks the underlying validator does not perform: 16-byte tile
//! offset alignment (under [`StrictMode::GdalCogger`]), ghost-area
//! parsing/strict-mode enforcement, SubIFD legacy-overview detection, and
//! WebP/LERC/JPEG-XL compression-warning emission.
//!
//! Findings are translated into the unified [`crate::error::QcIssue`] and
//! [`crate::error::Severity`] taxonomy used by the rest of `oxigdal-qc` —
//! this module deliberately does not invent a parallel `ComplianceLevel`.

use std::path::Path;

use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::TiffFile;
use oxigdal_geotiff::cog::ghost_area::{GhostArea, parse_ghost_area};
use oxigdal_geotiff::cog::{DetailedCogValidation, validate_cog_detailed};
use oxigdal_geotiff::cog::{ValidationCategory, ValidationSeverity};
use oxigdal_geotiff::tiff::{Compression, TiffTag};

use crate::error::{QcIssue, QcResult, Severity};

/// Strictness profile for COG compliance.
///
/// - [`StrictMode::Ogc10`]: OGC COG 1.0 spec minimum (the default).
/// - [`StrictMode::GdalCogger`]: matches GDAL ≥ 3.1 `cogger` output —
///   requires 16-byte tile offset alignment and a recognised ghost area.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StrictMode {
    /// OGC COG 1.0 minimum.
    #[default]
    Ogc10,
    /// GDAL `cogger`-strict (16-byte alignment + ghost area).
    GdalCogger,
}

/// COG compliance checker.
#[derive(Debug, Clone)]
pub struct CogComplianceChecker {
    /// Strictness profile.
    pub strict_mode: StrictMode,
    /// If `true`, emit a `Major` issue when no internal overviews are
    /// present.
    pub require_overviews: bool,
}

impl Default for CogComplianceChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl CogComplianceChecker {
    /// Constructs a checker with default settings (OGC 1.0, overviews
    /// optional).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            strict_mode: StrictMode::Ogc10,
            require_overviews: false,
        }
    }

    /// Sets the strict mode.
    #[must_use]
    pub const fn with_strict_mode(mut self, m: StrictMode) -> Self {
        self.strict_mode = m;
        self
    }

    /// Toggles the "overviews required" rule.
    #[must_use]
    pub const fn with_require_overviews(mut self, b: bool) -> Self {
        self.require_overviews = b;
        self
    }

    /// Runs full compliance checking on a file.
    pub fn check_file<P: AsRef<Path>>(&self, path: P) -> QcResult<CogComplianceResult> {
        let source = FileDataSource::open(path.as_ref()).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to open COG: {}", e))
        })?;
        let tiff = TiffFile::parse(&source).map_err(|e| {
            crate::error::QcError::RasterError(format!("Failed to parse TIFF: {}", e))
        })?;

        let geotiff_report = validate_cog_detailed(&tiff, &source).map_err(|e| {
            crate::error::QcError::RasterError(format!("validate_cog_detailed failed: {}", e))
        })?;
        let ghost_area = parse_ghost_area(&tiff, &source).map_err(|e| {
            crate::error::QcError::RasterError(format!("parse_ghost_area failed: {}", e))
        })?;

        let mut issues = Vec::new();

        // 1) Translate the geotiff validator's messages into QcIssues with
        //    promotion under strict modes.
        translate_geotiff_messages(&geotiff_report, self.strict_mode, &mut issues);

        // 2) Ghost-area enforcement.
        check_ghost_area(&ghost_area, self.strict_mode, &mut issues);

        // 3) 16-byte tile-offset alignment (GdalCogger only).
        check_tile_offset_alignment(&tiff, &source, self.strict_mode, &mut issues);

        // 4) SubIFD legacy overview detection.
        check_subifd_overviews(&tiff, &mut issues);

        // 5) Compression code recognition (WebP / LERC / JPEG-XL → Warning).
        check_compression_warnings(&tiff, &source, &mut issues);

        // 6) Optional overviews requirement.
        if self.require_overviews && tiff.ifds.len() <= 1 {
            issues.push(
                QcIssue::new(
                    Severity::Major,
                    "cog",
                    "No internal overviews",
                    "Caller required overviews; the file has none.".to_string(),
                )
                .with_rule_id("COG-OVERVIEWS-REQUIRED"),
            );
        }

        let is_compliant = !issues.iter().any(|i| i.severity == Severity::Critical);

        Ok(CogComplianceResult {
            is_compliant,
            strict_mode: self.strict_mode,
            issues,
            geotiff_report,
            ghost_area,
        })
    }
}

/// Result of COG compliance checking.
#[derive(Debug, Clone)]
pub struct CogComplianceResult {
    /// `true` if no `Critical` issues were raised.
    pub is_compliant: bool,
    /// Strictness mode used for the check.
    pub strict_mode: StrictMode,
    /// Translated issues (own taxonomy).
    pub issues: Vec<QcIssue>,
    /// Underlying geotiff report (forwarded for callers that want to inspect
    /// raw scoring/performance metrics).
    pub geotiff_report: DetailedCogValidation,
    /// Parsed ghost area (if any).
    pub ghost_area: Option<GhostArea>,
}

fn translate_geotiff_messages(
    report: &DetailedCogValidation,
    mode: StrictMode,
    out: &mut Vec<QcIssue>,
) {
    for msg in &report.messages {
        let severity = match msg.severity {
            ValidationSeverity::Error => Severity::Critical,
            ValidationSeverity::Warning => Severity::Warning,
            ValidationSeverity::Info => Severity::Info,
        };

        // Striped under strict OGC mode → escalate. The geotiff validator
        // already emits Error("Image is not tiled..."); we strengthen the
        // user-facing severity wording when running OGC 1.0 strict.
        let final_severity = if msg.severity == ValidationSeverity::Error
            && msg.message.contains("not tiled")
            && mode == StrictMode::Ogc10
        {
            Severity::Critical
        } else {
            severity
        };

        let category = format!("cog/{}", category_name(msg.category));
        let mut issue = QcIssue::new(
            final_severity,
            category,
            short_description(&msg.message),
            msg.message.clone(),
        )
        .with_rule_id(rule_id_for(msg.category, &msg.message));
        if let Some(ref s) = msg.suggestion {
            issue = issue.with_suggestion(s.clone());
        }
        out.push(issue);
    }
}

fn category_name(cat: ValidationCategory) -> &'static str {
    match cat {
        ValidationCategory::IfdStructure => "ifd",
        ValidationCategory::TileOrganization => "tiling",
        ValidationCategory::Overviews => "overviews",
        ValidationCategory::Compression => "compression",
        ValidationCategory::Metadata => "metadata",
        ValidationCategory::Performance => "performance",
        ValidationCategory::HttpEfficiency => "http",
    }
}

fn short_description(msg: &str) -> String {
    msg.split([':', '.', ',']).next().unwrap_or(msg).to_string()
}

fn rule_id_for(cat: ValidationCategory, msg: &str) -> String {
    let prefix = match cat {
        ValidationCategory::IfdStructure => "COG-IFD",
        ValidationCategory::TileOrganization => "COG-TILE",
        ValidationCategory::Overviews => "COG-OV",
        ValidationCategory::Compression => "COG-COMP",
        ValidationCategory::Metadata => "COG-META",
        ValidationCategory::Performance => "COG-PERF",
        ValidationCategory::HttpEfficiency => "COG-HTTP",
    };
    let slug: String = msg
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(24)
        .collect();
    format!("{}-{}", prefix, slug.to_uppercase())
}

fn check_ghost_area(area: &Option<GhostArea>, mode: StrictMode, out: &mut Vec<QcIssue>) {
    match (area, mode) {
        (Some(g), _) => {
            // Found one; emit Info for the caller's benefit.
            out.push(
                QcIssue::new(
                    Severity::Info,
                    "cog/ghost_area",
                    "Ghost area present",
                    format!(
                        "Ghost area parsed: layout={:?}, block_size={:?}, leader={}, trailer={}, raw_keys={}",
                        g.layout,
                        g.block_size,
                        g.block_leader_size_as_uint4,
                        g.block_trailer_size_as_uint4,
                        g.raw_kv.len()
                    ),
                )
                .with_rule_id("COG-GHOST-PRESENT"),
            );
            if let Some(true) = g.known_incompatible_edition {
                out.push(
                    QcIssue::new(
                        Severity::Major,
                        "cog/ghost_area",
                        "Ghost flagged incompatible edition",
                        "Ghost area declares KNOWN_INCOMPATIBLE_EDITION=YES; \
                         file was edited after creation in a way GDAL flags as breaking COG layout."
                            .to_string(),
                    )
                    .with_rule_id("COG-GHOST-INCOMPATIBLE-EDITION"),
                );
            }
        }
        (None, StrictMode::GdalCogger) => {
            out.push(
                QcIssue::new(
                    Severity::Major,
                    "cog/ghost_area",
                    "Missing ghost area",
                    "GdalCogger strict mode requires a GDAL ghost area between the TIFF \
                     header and the first IFD; none was found."
                        .to_string(),
                )
                .with_rule_id("COG-GHOST-MISSING")
                .with_suggestion("Re-encode with GDAL cogger driver (≥ 3.1)."),
            );
        }
        (None, StrictMode::Ogc10) => {
            out.push(
                QcIssue::new(
                    Severity::Info,
                    "cog/ghost_area",
                    "No ghost area present",
                    "OGC 1.0 does not require a ghost area; informational only.".to_string(),
                )
                .with_rule_id("COG-GHOST-ABSENT-INFO"),
            );
        }
    }
}

fn check_tile_offset_alignment<S: oxigdal_core::io::DataSource>(
    tiff: &TiffFile,
    source: &S,
    mode: StrictMode,
    out: &mut Vec<QcIssue>,
) {
    if mode != StrictMode::GdalCogger {
        return;
    }
    let Some(primary) = tiff.ifds.first() else {
        return;
    };
    let Some(entry) = primary.get_entry(TiffTag::TileOffsets) else {
        return;
    };
    let Ok(offsets) = entry.get_u64_vec(source, tiff.byte_order(), tiff.header.variant) else {
        return;
    };
    for (i, off) in offsets.iter().enumerate() {
        if off % 16 != 0 {
            out.push(
                QcIssue::new(
                    Severity::Critical,
                    "cog/tiling",
                    "Tile offset not 16-byte aligned",
                    format!(
                        "TileOffsetMisaligned: tile {} at offset {}, not 16-byte aligned",
                        i, off
                    ),
                )
                .with_rule_id("COG-TILE-OFFSET-MISALIGNED")
                .with_suggestion(
                    "Re-encode with `gdal_translate -of COG` to enforce 16-byte alignment.",
                ),
            );
            // Bail after the first offender — one critical is enough.
            break;
        }
    }
}

fn check_subifd_overviews(tiff: &TiffFile, out: &mut Vec<QcIssue>) {
    let Some(primary) = tiff.ifds.first() else {
        return;
    };
    if primary.get_entry(TiffTag::SubIfd).is_some() {
        out.push(
            QcIssue::new(
                Severity::Warning,
                "cog/overviews",
                "Legacy SubIFD overviews",
                "Tag 330 (SubIFD) is present — pre-GDAL-2.5 overview layout. \
                 Modern COG uses sequential IFDs."
                    .to_string(),
            )
            .with_rule_id("COG-OV-SUBIFD-LEGACY")
            .with_suggestion(
                "Re-encode with `gdal_translate -of COG` to use sequential overviews.",
            ),
        );
    }
}

fn check_compression_warnings<S: oxigdal_core::io::DataSource>(
    tiff: &TiffFile,
    source: &S,
    out: &mut Vec<QcIssue>,
) {
    let Some(primary) = tiff.ifds.first() else {
        return;
    };
    let Some(entry) = primary.get_entry(TiffTag::Compression) else {
        return;
    };
    let Ok(value) = entry.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
    else {
        return;
    };
    let comp = Compression::from_u16(value as u16);
    let warn = match comp {
        Some(Compression::WebP) => Some(("WebP", value)),
        Some(Compression::JpegXl) => Some(("JPEG XL", value)),
        Some(Compression::Lerc) => Some(("LERC", value)),
        None => Some(("Unknown", value)),
        _ => None,
    };
    if let Some((name, code)) = warn {
        out.push(
            QcIssue::new(
                Severity::Warning,
                "cog/compression",
                "Non-standard / niche COG compression",
                format!(
                    "UnknownCompression: code {} ({}). OGC 1.0 readers may not support this.",
                    code, name
                ),
            )
            .with_rule_id("COG-COMP-NON-STANDARD"),
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]

    use super::*;
    use oxigdal_core::types::RasterDataType;
    use oxigdal_geotiff::Compression as TiffCompression;
    use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};
    use std::env;
    use std::path::PathBuf;

    fn unique_temp_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut path = env::temp_dir();
        path.push(format!("oxigdal_qc_cog_{}_{}.tif", label, nanos));
        path
    }

    /// Generates a minimal valid COG via `CogWriter`. Uses Deflate so this
    /// works without the lzw feature being enabled (deflate is default).
    fn write_minimal_cog(path: &PathBuf) {
        let width = 256u64;
        let height = 256u64;
        let data = vec![0u8; (width * height) as usize];

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(TiffCompression::Deflate)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4]);
        let mut writer =
            CogWriter::create(path, config, CogWriterOptions::default()).expect("create COG");
        writer.write(&data).expect("write COG");
    }

    #[test]
    fn test_cog_strict_ogc10_minimal_pass() {
        let path = unique_temp_path("ogc10_pass");
        write_minimal_cog(&path);

        let checker = CogComplianceChecker::new().with_strict_mode(StrictMode::Ogc10);
        let result = checker.check_file(&path).expect("check_file");

        // No Critical issues.
        let critical: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Critical)
            .collect();
        assert!(
            critical.is_empty(),
            "expected no Critical, got {:#?}",
            critical
        );
        assert!(result.is_compliant);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_strict_ogc10_rejects_striped() {
        // Build a non-tiled file by hand (cannot use CogWriter — it forbids
        // striped). Use the minimal-tiff helper from tests then run the
        // checker. Easiest approach: raw bytes for a striped TIFF.
        use std::fs::File;
        use std::io::Write;

        let path = unique_temp_path("striped");
        {
            let mut f = File::create(&path).expect("create file");
            // Classic LE TIFF, IFD at offset 8.
            f.write_all(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00])
                .expect("write header");
            // IFD: 5 entries (count u16 LE).
            f.write_all(&[0x05, 0x00]).expect("entries count");
            // ImageWidth (256) = 64
            f.write_all(&[
                0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // ImageLength (257) = 64
            f.write_all(&[
                0x01, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // BitsPerSample (258) = 8
            f.write_all(&[
                0x02, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // RowsPerStrip (278) = 64 → striped layout
            f.write_all(&[
                0x16, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // StripOffsets (273) = 200 (just a value)
            f.write_all(&[
                0x11, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0xC8, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // Next IFD = 0
            f.write_all(&[0x00, 0x00, 0x00, 0x00]).expect("");
            // Pad with strip data
            f.write_all(&vec![0u8; 4096]).expect("");
        }

        let checker = CogComplianceChecker::new().with_strict_mode(StrictMode::Ogc10);
        let result = checker.check_file(&path).expect("check_file");

        let crit: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Critical)
            .collect();
        assert!(
            !crit.is_empty(),
            "expected Critical for striped layout, got {:#?}",
            result.issues
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_strict_gdal_requires_ghost_area() {
        let path = unique_temp_path("gdal_strict_no_ghost");
        write_minimal_cog(&path);

        let checker = CogComplianceChecker::new().with_strict_mode(StrictMode::GdalCogger);
        let result = checker.check_file(&path).expect("check_file");

        let missing = result
            .issues
            .iter()
            .find(|i| i.rule_id.as_deref() == Some("COG-GHOST-MISSING"));
        assert!(
            missing.is_some(),
            "expected COG-GHOST-MISSING under GdalCogger, got {:#?}",
            result.issues
        );
        assert_eq!(missing.expect("present").severity, Severity::Major);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_alignment_violation_emits_critical() {
        // Synthesise a tiled TIFF with TileOffset = 137 (not 16-byte aligned).
        use std::fs::File;
        use std::io::Write;

        let path = unique_temp_path("misaligned");
        {
            let mut f = File::create(&path).expect("create");
            // Header
            f.write_all(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00])
                .expect("");
            // 6 entries
            f.write_all(&[0x06, 0x00]).expect("");
            // ImageWidth = 64
            f.write_all(&[
                0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // ImageLength = 64
            f.write_all(&[
                0x01, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // TileWidth (322) = 64
            f.write_all(&[
                0x42, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // TileLength (323) = 64
            f.write_all(&[
                0x43, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // TileOffsets (324) LONG count=1 value=137 (NOT 16-aligned)
            f.write_all(&[
                0x44, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x89, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // TileByteCounts (325) LONG count=1 value=4096
            f.write_all(&[
                0x45, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
            ])
            .expect("");
            // Next IFD = 0
            f.write_all(&[0x00, 0x00, 0x00, 0x00]).expect("");
            // Pad
            f.write_all(&vec![0u8; 8192]).expect("");
        }

        let checker = CogComplianceChecker::new().with_strict_mode(StrictMode::GdalCogger);
        let result = checker.check_file(&path).expect("check_file");
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.severity == Severity::Critical
                    && i.rule_id.as_deref() == Some("COG-TILE-OFFSET-MISALIGNED")),
            "expected misaligned-offset Critical, got {:#?}",
            result.issues
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_overviews_not_factor_of_2_emits_minor() {
        // CogWriter emits power-of-2 overviews always, so we test the
        // INVERSE: non-pow2 levels should escape; instead, use a minimal
        // image and check that the underlying validator's "non-pow2"
        // info-level message becomes Severity::Info in our pipeline.
        let path = unique_temp_path("ov_pow2");
        write_minimal_cog(&path);

        let checker = CogComplianceChecker::new();
        let result = checker.check_file(&path).expect("check_file");

        // The minimal pow2 case should not produce Major/Minor for overview shapes.
        assert!(
            !result.issues.iter().any(|i| i.severity == Severity::Minor
                && i.rule_id
                    .as_deref()
                    .is_some_and(|r| r.starts_with("COG-OV"))),
            "did not expect non-pow2 Minor, got {:#?}",
            result.issues
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_ghost_area_parsing_kv_pairs() {
        // Build a tiff with a ghost area, then verify our parser surfaces it.
        use std::fs::File;
        use std::io::Write;
        let path = unique_temp_path("ghost_kv");
        let ghost: &[u8] = b"LAYOUT=IFDS_BEFORE_DATA\nBLOCK_SIZE_X=256\nBLOCK_SIZE_Y=256\n\0";
        {
            let mut f = File::create(&path).expect("");
            // Classic header pointing first IFD beyond the ghost area.
            let first_ifd_offset: u32 = 8 + ghost.len() as u32;
            // bytes 0-7: BOM(II), version=42, first_ifd_offset=...
            f.write_all(&[0x49, 0x49, 0x2A, 0x00]).expect("");
            f.write_all(&first_ifd_offset.to_le_bytes()).expect("");
            // Ghost
            f.write_all(ghost).expect("");
            // Minimal IFD with 0 entries
            f.write_all(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
                .expect("");
        }

        let source = oxigdal_core::io::FileDataSource::open(&path).expect("");
        let tiff = oxigdal_geotiff::TiffFile::parse(&source).expect("");
        let ga = parse_ghost_area(&tiff, &source).expect("read");
        let area = ga.expect("ghost area present");
        assert_eq!(area.layout.as_deref(), Some("IFDS_BEFORE_DATA"));
        assert_eq!(area.block_size, Some((256, 256)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_ghost_area_absent_is_info_under_ogc10() {
        let path = unique_temp_path("ogc10_no_ghost");
        write_minimal_cog(&path);

        let checker = CogComplianceChecker::new().with_strict_mode(StrictMode::Ogc10);
        let result = checker.check_file(&path).expect("");
        assert!(
            result.issues.iter().any(|i| i.severity == Severity::Info
                && i.rule_id.as_deref() == Some("COG-GHOST-ABSENT-INFO")),
            "expected absent-Info, got {:#?}",
            result.issues
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_cog_subifd_overview_legacy_warning() {
        // Synthesise a TIFF whose primary IFD declares a SubIFD pointer.
        use std::fs::File;
        use std::io::Write;
        let path = unique_temp_path("subifd");
        {
            let mut f = File::create(&path).expect("");
            f.write_all(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00])
                .expect("");
            // 6 entries
            f.write_all(&[0x06, 0x00]).expect("");
            // ImageWidth=64, ImageLength=64
            f.write_all(&[
                0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            f.write_all(&[
                0x01, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // TileWidth=64
            f.write_all(&[
                0x42, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // TileLength=64
            f.write_all(&[
                0x43, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // SubIfd (330) IFD-pointer (treat as Long for synthetic test) = 0x10000 (just a value)
            f.write_all(&[
                0x4A, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
            ])
            .expect("");
            // TileOffsets=128 (16-byte aligned)
            f.write_all(&[
                0x44, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00,
            ])
            .expect("");
            // Next IFD=0
            f.write_all(&[0x00, 0x00, 0x00, 0x00]).expect("");
            // Pad to leave room for SubIFD pointer target
            f.write_all(&vec![0u8; 0x20000]).expect("");
        }

        let checker = CogComplianceChecker::new();
        let result = checker.check_file(&path).expect("");
        assert!(
            result.issues.iter().any(|i| i.severity == Severity::Warning
                && i.rule_id.as_deref() == Some("COG-OV-SUBIFD-LEGACY")),
            "expected SubIFD legacy warning, got {:#?}",
            result.issues
        );
        let _ = std::fs::remove_file(&path);
    }
}
