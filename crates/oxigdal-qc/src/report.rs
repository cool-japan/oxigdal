//! Quality control report generation.
//!
//! This module provides functionality to generate quality control reports
//! in various formats (HTML, JSON, etc.).

use crate::error::{QcError, QcIssue, QcResult, Severity};
use std::io::Write;

/// Quality control report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QualityReport {
    /// Report title.
    pub title: String,

    /// Report generation timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Executive summary.
    pub summary: ReportSummary,

    /// Detailed sections.
    pub sections: Vec<ReportSection>,

    /// All issues collected.
    pub issues: Vec<QcIssue>,

    /// Overall quality score (0.0 - 100.0).
    pub quality_score: f64,
}

/// Executive summary of the report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReportSummary {
    /// Total number of checks performed.
    pub total_checks: usize,

    /// Number of passed checks.
    pub passed_checks: usize,

    /// Number of failed checks.
    pub failed_checks: usize,

    /// Total issues by severity.
    pub issues_by_severity: SeverityCounts,

    /// Overall assessment.
    pub assessment: QualityAssessment,

    /// Key findings.
    pub key_findings: Vec<String>,
}

/// Count of issues by severity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeverityCounts {
    /// Number of critical issues.
    pub critical: usize,

    /// Number of major issues.
    pub major: usize,

    /// Number of minor issues.
    pub minor: usize,

    /// Number of warnings.
    pub warnings: usize,

    /// Number of info messages.
    pub info: usize,
}

/// Overall quality assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum QualityAssessment {
    /// Excellent quality.
    Excellent,

    /// Good quality.
    Good,

    /// Fair quality.
    Fair,

    /// Poor quality.
    Poor,

    /// Unacceptable quality.
    Unacceptable,
}

/// A section of the report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReportSection {
    /// Section title.
    pub title: String,

    /// Section description.
    pub description: String,

    /// Section results (key-value pairs).
    pub results: Vec<(String, String)>,

    /// Section-specific issues.
    pub issues: Vec<QcIssue>,

    /// Section pass/fail status.
    pub passed: bool,
}

impl QualityReport {
    /// Creates a new quality report.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            timestamp: chrono::Utc::now(),
            summary: ReportSummary {
                total_checks: 0,
                passed_checks: 0,
                failed_checks: 0,
                issues_by_severity: SeverityCounts {
                    critical: 0,
                    major: 0,
                    minor: 0,
                    warnings: 0,
                    info: 0,
                },
                assessment: QualityAssessment::Excellent,
                key_findings: Vec::new(),
            },
            sections: Vec::new(),
            issues: Vec::new(),
            quality_score: 100.0,
        }
    }

    /// Adds a section to the report.
    pub fn add_section(&mut self, section: ReportSection) {
        self.summary.total_checks += 1;
        if section.passed {
            self.summary.passed_checks += 1;
        } else {
            self.summary.failed_checks += 1;
        }

        self.issues.extend(section.issues.clone());
        self.sections.push(section);
    }

    /// Adds issues to the report.
    pub fn add_issues(&mut self, issues: Vec<QcIssue>) {
        for issue in &issues {
            match issue.severity {
                Severity::Critical => self.summary.issues_by_severity.critical += 1,
                Severity::Major => self.summary.issues_by_severity.major += 1,
                Severity::Minor => self.summary.issues_by_severity.minor += 1,
                Severity::Warning => self.summary.issues_by_severity.warnings += 1,
                Severity::Info => self.summary.issues_by_severity.info += 1,
            }
        }
        self.issues.extend(issues);
    }

    /// Finalizes the report by computing scores and assessment.
    pub fn finalize(&mut self) {
        // Calculate quality score
        let total_issues = self.summary.issues_by_severity.critical
            + self.summary.issues_by_severity.major
            + self.summary.issues_by_severity.minor
            + self.summary.issues_by_severity.warnings;

        // Weighted scoring
        let critical_weight = 20.0;
        let major_weight = 10.0;
        let minor_weight = 3.0;
        let warning_weight = 1.0;

        let penalty = (self.summary.issues_by_severity.critical as f64 * critical_weight)
            + (self.summary.issues_by_severity.major as f64 * major_weight)
            + (self.summary.issues_by_severity.minor as f64 * minor_weight)
            + (self.summary.issues_by_severity.warnings as f64 * warning_weight);

        self.quality_score = (100.0 - penalty).max(0.0);

        // Determine assessment
        self.summary.assessment = if self.summary.issues_by_severity.critical > 0 {
            QualityAssessment::Unacceptable
        } else if self.quality_score >= 90.0 {
            QualityAssessment::Excellent
        } else if self.quality_score >= 75.0 {
            QualityAssessment::Good
        } else if self.quality_score >= 50.0 {
            QualityAssessment::Fair
        } else {
            QualityAssessment::Poor
        };

        // Generate key findings
        self.summary.key_findings.clear();

        if self.summary.issues_by_severity.critical > 0 {
            self.summary.key_findings.push(format!(
                "{} critical issues require immediate attention",
                self.summary.issues_by_severity.critical
            ));
        }

        if self.summary.issues_by_severity.major > 5 {
            self.summary.key_findings.push(format!(
                "{} major issues affect data quality",
                self.summary.issues_by_severity.major
            ));
        }

        if total_issues == 0 {
            self.summary
                .key_findings
                .push("No quality issues detected".to_string());
        }

        if self.summary.passed_checks == self.summary.total_checks {
            self.summary
                .key_findings
                .push("All quality checks passed".to_string());
        }
    }

    /// Generates an HTML report.
    ///
    /// # Errors
    ///
    /// Returns an error if HTML generation fails.
    pub fn generate_html(&self, path: impl AsRef<std::path::Path>) -> QcResult<()> {
        let mut file = std::fs::File::create(path).map_err(QcError::Io)?;

        writeln!(file, "<!DOCTYPE html>")?;
        writeln!(file, "<html lang=\"en\">")?;
        writeln!(file, "<head>")?;
        writeln!(file, "    <meta charset=\"UTF-8\">")?;
        writeln!(
            file,
            "    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">"
        )?;
        writeln!(file, "    <title>{}</title>", self.title)?;
        writeln!(file, "    <style>")?;
        writeln!(file, "{}", Self::get_css())?;
        writeln!(file, "    </style>")?;
        writeln!(file, "</head>")?;
        writeln!(file, "<body>")?;
        writeln!(file, "    <div class=\"container\">")?;

        // Header
        writeln!(file, "        <header>")?;
        writeln!(file, "            <h1>{}</h1>", self.title)?;
        writeln!(
            file,
            "            <p class=\"timestamp\">Generated: {}</p>",
            self.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        )?;
        writeln!(file, "        </header>")?;

        // Executive Summary
        writeln!(file, "        <section class=\"summary\">")?;
        writeln!(file, "            <h2>Executive Summary</h2>")?;
        writeln!(
            file,
            "            <div class=\"quality-score quality-{}\">{:.1}</div>",
            self.get_score_class(),
            self.quality_score
        )?;
        writeln!(
            file,
            "            <p class=\"assessment\">Assessment: <strong>{:?}</strong></p>",
            self.summary.assessment
        )?;
        writeln!(
            file,
            "            <p>Checks: {} passed, {} failed out of {} total</p>",
            self.summary.passed_checks, self.summary.failed_checks, self.summary.total_checks
        )?;

        // Issues by severity
        writeln!(file, "            <div class=\"severity-counts\">")?;
        self.write_severity_badge(
            &mut file,
            "Critical",
            self.summary.issues_by_severity.critical,
        )?;
        self.write_severity_badge(&mut file, "Major", self.summary.issues_by_severity.major)?;
        self.write_severity_badge(&mut file, "Minor", self.summary.issues_by_severity.minor)?;
        self.write_severity_badge(
            &mut file,
            "Warning",
            self.summary.issues_by_severity.warnings,
        )?;
        writeln!(file, "            </div>")?;

        // Key findings
        if !self.summary.key_findings.is_empty() {
            writeln!(file, "            <div class=\"key-findings\">")?;
            writeln!(file, "                <h3>Key Findings</h3>")?;
            writeln!(file, "                <ul>")?;
            for finding in &self.summary.key_findings {
                writeln!(file, "                    <li>{}</li>", finding)?;
            }
            writeln!(file, "                </ul>")?;
            writeln!(file, "            </div>")?;
        }

        writeln!(file, "        </section>")?;

        // Sections
        for section in &self.sections {
            self.write_section(&mut file, section)?;
        }

        // All Issues
        if !self.issues.is_empty() {
            writeln!(file, "        <section class=\"issues\">")?;
            writeln!(
                file,
                "            <h2>All Issues ({} total)</h2>",
                self.issues.len()
            )?;
            writeln!(file, "            <table>")?;
            writeln!(file, "                <thead>")?;
            writeln!(file, "                    <tr>")?;
            writeln!(file, "                        <th>Severity</th>")?;
            writeln!(file, "                        <th>Category</th>")?;
            writeln!(file, "                        <th>Description</th>")?;
            writeln!(file, "                        <th>Location</th>")?;
            writeln!(file, "                    </tr>")?;
            writeln!(file, "                </thead>")?;
            writeln!(file, "                <tbody>")?;

            for issue in &self.issues {
                writeln!(file, "                    <tr>")?;
                writeln!(
                    file,
                    "                        <td class=\"severity-{}\">{}</td>",
                    format!("{:?}", issue.severity).to_lowercase(),
                    issue.severity
                )?;
                writeln!(file, "                        <td>{}</td>", issue.category)?;
                writeln!(file, "                        <td>")?;
                writeln!(
                    file,
                    "                            <strong>{}</strong>",
                    issue.description
                )?;
                writeln!(file, "                            <p>{}</p>", issue.message)?;
                if let Some(ref suggestion) = issue.suggestion {
                    writeln!(
                        file,
                        "                            <p class=\"suggestion\">Suggestion: {}</p>",
                        suggestion
                    )?;
                }
                writeln!(file, "                        </td>")?;
                writeln!(
                    file,
                    "                        <td>{}</td>",
                    issue.location.as_deref().unwrap_or("-")
                )?;
                writeln!(file, "                    </tr>")?;
            }

            writeln!(file, "                </tbody>")?;
            writeln!(file, "            </table>")?;
            writeln!(file, "        </section>")?;
        }

        writeln!(file, "    </div>")?;
        writeln!(file, "</body>")?;
        writeln!(file, "</html>")?;

        Ok(())
    }

    /// Generates a JSON report.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON generation fails.
    pub fn generate_json(&self, path: impl AsRef<std::path::Path>) -> QcResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).map_err(QcError::Io)?;
        Ok(())
    }

    fn write_severity_badge<W: Write>(
        &self,
        writer: &mut W,
        label: &str,
        count: usize,
    ) -> QcResult<()> {
        writeln!(
            writer,
            "                <span class=\"badge badge-{}\">{}: {}</span>",
            label.to_lowercase(),
            label,
            count
        )?;
        Ok(())
    }

    fn write_section<W: Write>(&self, writer: &mut W, section: &ReportSection) -> QcResult<()> {
        writeln!(writer, "        <section class=\"report-section\">")?;
        writeln!(writer, "            <h2>{}</h2>", section.title)?;
        writeln!(writer, "            <p>{}</p>", section.description)?;

        if !section.results.is_empty() {
            writeln!(writer, "            <table>")?;
            for (key, value) in &section.results {
                writeln!(writer, "                <tr>")?;
                writeln!(
                    writer,
                    "                    <td><strong>{}</strong></td>",
                    key
                )?;
                writeln!(writer, "                    <td>{}</td>", value)?;
                writeln!(writer, "                </tr>")?;
            }
            writeln!(writer, "            </table>")?;
        }

        if !section.issues.is_empty() {
            writeln!(
                writer,
                "            <p class=\"issue-count\">{} issues found</p>",
                section.issues.len()
            )?;
        }

        writeln!(writer, "        </section>")?;
        Ok(())
    }

    fn get_score_class(&self) -> &str {
        if self.quality_score >= 90.0 {
            "excellent"
        } else if self.quality_score >= 75.0 {
            "good"
        } else if self.quality_score >= 50.0 {
            "fair"
        } else {
            "poor"
        }
    }

    fn get_css() -> &'static str {
        r#"
body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
    line-height: 1.6;
    color: #333;
    background: #f5f5f5;
    margin: 0;
    padding: 20px;
}
.container {
    max-width: 1200px;
    margin: 0 auto;
    background: white;
    padding: 30px;
    border-radius: 8px;
    box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}
header {
    border-bottom: 3px solid #007bff;
    padding-bottom: 20px;
    margin-bottom: 30px;
}
h1 {
    margin: 0;
    color: #007bff;
}
.timestamp {
    color: #666;
    margin: 10px 0 0 0;
}
.summary {
    background: #f8f9fa;
    padding: 20px;
    border-radius: 8px;
    margin-bottom: 30px;
}
.quality-score {
    font-size: 48px;
    font-weight: bold;
    text-align: center;
    margin: 20px 0;
    padding: 20px;
    border-radius: 8px;
}
.quality-excellent {
    background: #d4edda;
    color: #155724;
}
.quality-good {
    background: #d1ecf1;
    color: #0c5460;
}
.quality-fair {
    background: #fff3cd;
    color: #856404;
}
.quality-poor {
    background: #f8d7da;
    color: #721c24;
}
.assessment {
    text-align: center;
    font-size: 1.2em;
}
.severity-counts {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    margin: 20px 0;
}
.badge {
    padding: 5px 15px;
    border-radius: 20px;
    font-weight: bold;
    color: white;
}
.badge-critical {
    background: #721c24;
}
.badge-major {
    background: #dc3545;
}
.badge-minor {
    background: #fd7e14;
}
.badge-warning {
    background: #ffc107;
    color: #333;
}
.key-findings {
    margin-top: 20px;
}
.key-findings ul {
    list-style-type: none;
    padding: 0;
}
.key-findings li {
    padding: 10px;
    margin: 5px 0;
    background: white;
    border-left: 4px solid #007bff;
}
.report-section {
    margin: 30px 0;
    padding: 20px;
    border: 1px solid #dee2e6;
    border-radius: 8px;
}
table {
    width: 100%;
    border-collapse: collapse;
    margin: 15px 0;
}
th, td {
    padding: 12px;
    text-align: left;
    border-bottom: 1px solid #dee2e6;
}
th {
    background: #f8f9fa;
    font-weight: bold;
}
.severity-critical {
    color: #721c24;
    font-weight: bold;
}
.severity-major {
    color: #dc3545;
    font-weight: bold;
}
.severity-minor {
    color: #fd7e14;
    font-weight: bold;
}
.severity-warning {
    color: #856404;
}
.suggestion {
    color: #0c5460;
    font-style: italic;
    margin-top: 5px;
}
.issue-count {
    color: #dc3545;
    font-weight: bold;
}
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_creation() {
        let report = QualityReport::new("Test Report");
        assert_eq!(report.title, "Test Report");
        assert_eq!(report.sections.len(), 0);
        assert_eq!(report.issues.len(), 0);
    }

    #[test]
    fn test_report_finalize() {
        let mut report = QualityReport::new("Test Report");
        report.add_issues(vec![QcIssue::new(
            Severity::Critical,
            "test",
            "Test issue",
            "Test message",
        )]);
        report.finalize();

        assert_eq!(report.summary.issues_by_severity.critical, 1);
        assert_eq!(report.summary.assessment, QualityAssessment::Unacceptable);
        assert!(report.quality_score < 100.0);
    }

    #[test]
    fn test_json_generation() {
        let mut report = QualityReport::new("Test Report");
        report.finalize();

        let temp_dir = std::env::temp_dir();
        let json_path = temp_dir.join("test_report.json");

        let result = report.generate_json(&json_path);
        assert!(result.is_ok());
        assert!(json_path.exists());

        let cleanup = std::fs::remove_file(&json_path);
        assert!(cleanup.is_ok());
    }
}
