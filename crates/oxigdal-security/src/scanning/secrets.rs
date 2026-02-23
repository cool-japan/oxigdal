//! Secret detection.

use crate::scanning::{Finding, ScanResult, ScanType, Severity};
use regex::Regex;

/// Secret scanner.
pub struct SecretScanner {
    patterns: Vec<(String, Regex)>,
}

impl SecretScanner {
    /// Create new secret scanner.
    pub fn new() -> Self {
        let mut scanner = Self {
            patterns: Vec::new(),
        };
        scanner.add_default_patterns();
        scanner
    }

    fn add_default_patterns(&mut self) {
        // AWS Access Key
        self.add_pattern("AWS Access Key", r"AKIA[0-9A-Z]{16}");

        // API Key pattern
        self.add_pattern(
            "Generic API Key",
            r#"api[_-]?key["']?\s*[:=]\s*["']?([a-zA-Z0-9_-]{32,})"#,
        );

        // Private key
        self.add_pattern("Private Key", r"-----BEGIN (RSA |EC )?PRIVATE KEY-----");
    }

    fn add_pattern(&mut self, name: &str, pattern: &str) {
        if let Ok(regex) = Regex::new(pattern) {
            self.patterns.push((name.to_string(), regex));
        }
    }

    /// Scan text for secrets.
    pub fn scan(&self, text: &str) -> ScanResult {
        let mut findings = Vec::new();

        for (name, regex) in &self.patterns {
            for mat in regex.find_iter(text) {
                findings.push(Finding {
                    id: uuid::Uuid::new_v4().to_string(),
                    severity: Severity::Critical,
                    description: format!("Possible {} detected", name),
                    location: Some(format!("Position: {}", mat.start())),
                });
            }
        }

        ScanResult {
            scan_type: ScanType::Secrets,
            findings,
            scanned_at: chrono::Utc::now(),
        }
    }
}

impl Default for SecretScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_scanner() {
        let scanner = SecretScanner::new();
        let text = "AWS Key: AKIAIOSFODNN7EXAMPLE";
        let result = scanner.scan(text);

        assert!(!result.findings.is_empty());
        assert_eq!(result.scan_type, ScanType::Secrets);
    }
}
