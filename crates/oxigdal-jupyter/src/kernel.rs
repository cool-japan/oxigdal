//! Jupyter kernel implementation for OxiGDAL
//!
//! This module provides a custom Jupyter kernel that supports OxiGDAL operations
//! with rich display and interactive features.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// OxiGDAL Jupyter kernel
pub struct OxiGdalKernel {
    /// Kernel configuration
    config: KernelConfig,
    /// Execution count
    execution_count: u64,
    /// User namespace (variables)
    namespace: HashMap<String, Value>,
    /// Command history
    history: Vec<String>,
}

/// Kernel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConfig {
    /// Kernel name
    pub kernel_name: String,
    /// Display name
    pub display_name: String,
    /// Language
    pub language: String,
    /// Language version
    pub language_version: String,
    /// File extension
    pub file_extension: String,
    /// Mimetype
    pub mimetype: String,
}

/// Value stored in namespace
#[derive(Debug, Clone)]
pub enum Value {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// Boolean value
    Boolean(bool),
    /// Path value
    Path(PathBuf),
    /// Dataset handle
    Dataset(String),
    /// Array data
    Array(Vec<f64>),
}

/// Execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Status (ok, error, abort)
    pub status: String,
    /// Execution count
    pub execution_count: u64,
    /// Output data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HashMap<String, String>>,
    /// Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Error information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

/// Error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// Error name
    pub ename: String,
    /// Error value
    pub evalue: String,
    /// Traceback
    pub traceback: Vec<String>,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            kernel_name: "oxigdal".to_string(),
            display_name: "OxiGDAL".to_string(),
            language: "rust".to_string(),
            language_version: "1.85".to_string(),
            file_extension: ".rs".to_string(),
            mimetype: "text/x-rustsrc".to_string(),
        }
    }
}

impl OxiGdalKernel {
    /// Create a new kernel
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: KernelConfig::default(),
            execution_count: 0,
            namespace: HashMap::new(),
            history: Vec::new(),
        })
    }

    /// Create a kernel with custom configuration
    pub fn with_config(config: KernelConfig) -> Result<Self> {
        Ok(Self {
            config,
            execution_count: 0,
            namespace: HashMap::new(),
            history: Vec::new(),
        })
    }

    /// Get kernel configuration
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    /// Execute code
    pub fn execute(&mut self, code: &str) -> Result<ExecutionResult> {
        self.execution_count += 1;
        self.history.push(code.to_string());

        // Check if it's a magic command
        if code.trim().starts_with('%') {
            return self.execute_magic(code);
        }

        // Parse and execute regular code
        match self.parse_and_execute(code) {
            Ok(output) => Ok(ExecutionResult {
                status: "ok".to_string(),
                execution_count: self.execution_count,
                data: output,
                metadata: None,
                error: None,
            }),
            Err(e) => Ok(ExecutionResult {
                status: "error".to_string(),
                execution_count: self.execution_count,
                data: None,
                metadata: None,
                error: Some(ErrorInfo {
                    ename: "ExecutionError".to_string(),
                    evalue: e.to_string(),
                    traceback: vec![e.to_string()],
                }),
            }),
        }
    }

    /// Execute magic command
    fn execute_magic(&mut self, code: &str) -> Result<ExecutionResult> {
        use crate::magic::MagicCommand;

        let magic = MagicCommand::parse(code)?;
        let output = magic.execute(&mut self.namespace)?;

        Ok(ExecutionResult {
            status: "ok".to_string(),
            execution_count: self.execution_count,
            data: Some(output),
            metadata: None,
            error: None,
        })
    }

    /// Parse and execute code
    fn parse_and_execute(&mut self, code: &str) -> Result<Option<HashMap<String, String>>> {
        // Simple expression evaluation for demo
        let code = code.trim();

        // Variable assignment: let name = value
        if code.starts_with("let ") {
            if let Some((name, value)) = code.strip_prefix("let ").and_then(|s| s.split_once('=')) {
                let name = name.trim().to_string();
                let value_str = value.trim();

                let val = if value_str.starts_with('"') && value_str.ends_with('"') {
                    Value::String(value_str.trim_matches('"').to_string())
                } else if let Ok(i) = value_str.parse::<i64>() {
                    Value::Integer(i)
                } else if let Ok(f) = value_str.parse::<f64>() {
                    Value::Float(f)
                } else if value_str == "true" || value_str == "false" {
                    Value::Boolean(value_str == "true")
                } else {
                    Value::String(value_str.to_string())
                };

                self.namespace.insert(name, val);
                return Ok(None);
            }
        }

        // Print variable value
        if let Some(var) = self.namespace.get(code) {
            let mut output = HashMap::new();
            output.insert("text/plain".to_string(), format!("{:?}", var));
            return Ok(Some(output));
        }

        Ok(None)
    }

    /// Complete code
    pub fn complete(&self, code: &str, cursor_pos: usize) -> Result<CompletionResult> {
        let mut matches = Vec::new();

        // Get the word at cursor
        let before_cursor = &code[..cursor_pos];
        let start = before_cursor
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '%')
            .map(|i| i + 1)
            .unwrap_or(0);

        let prefix = &before_cursor[start..];

        // Complete magic commands
        if prefix.starts_with('%') {
            let magic_commands = [
                "%load_raster",
                "%plot",
                "%info",
                "%crs",
                "%bounds",
                "%stats",
            ];
            for cmd in &magic_commands {
                if cmd.starts_with(prefix) {
                    matches.push(cmd.to_string());
                }
            }
        } else {
            // Complete variable names
            for key in self.namespace.keys() {
                if key.starts_with(prefix) {
                    matches.push(key.clone());
                }
            }

            // Complete keywords
            let keywords = ["let", "fn", "struct", "enum", "impl", "trait"];
            for kw in &keywords {
                if kw.starts_with(prefix) {
                    matches.push(kw.to_string());
                }
            }
        }

        Ok(CompletionResult {
            matches,
            cursor_start: start,
            cursor_end: cursor_pos,
            metadata: HashMap::new(),
        })
    }

    /// Inspect code
    pub fn inspect(&self, code: &str, _cursor_pos: usize) -> Result<InspectionResult> {
        let mut data = HashMap::new();

        // Check if it's a magic command
        if code.trim().starts_with('%') {
            let help_text = self.get_magic_help(code.trim());
            data.insert("text/plain".to_string(), help_text);
        } else if let Some(var) = self.namespace.get(code.trim()) {
            // Inspect variable
            data.insert("text/plain".to_string(), format!("{:?}", var));
        }

        Ok(InspectionResult {
            found: !data.is_empty(),
            data,
            metadata: HashMap::new(),
        })
    }

    /// Get magic command help
    fn get_magic_help(&self, command: &str) -> String {
        match command {
            "%load_raster" => "Load a raster file\nUsage: %load_raster <path> [name]".to_string(),
            "%plot" => "Plot raster data\nUsage: %plot <dataset>".to_string(),
            "%info" => "Show dataset information\nUsage: %info <dataset>".to_string(),
            "%crs" => "Show coordinate reference system\nUsage: %crs <dataset>".to_string(),
            "%bounds" => "Show dataset bounds\nUsage: %bounds <dataset>".to_string(),
            "%stats" => "Show raster statistics\nUsage: %stats <dataset>".to_string(),
            _ => format!("Unknown magic command: {}", command),
        }
    }

    /// Get execution history
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Get namespace
    pub fn namespace(&self) -> &HashMap<String, Value> {
        &self.namespace
    }

    /// Clear namespace
    pub fn clear_namespace(&mut self) {
        self.namespace.clear();
    }

    /// Get execution count
    pub fn execution_count(&self) -> u64 {
        self.execution_count
    }
}

/// Completion result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResult {
    /// Completion matches
    pub matches: Vec<String>,
    /// Cursor start position
    pub cursor_start: usize,
    /// Cursor end position
    pub cursor_end: usize,
    /// Metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Inspection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionResult {
    /// Whether inspection found anything
    pub found: bool,
    /// Inspection data
    pub data: HashMap<String, String>,
    /// Metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_creation() -> Result<()> {
        let kernel = OxiGdalKernel::new()?;
        assert_eq!(kernel.execution_count(), 0);
        assert_eq!(kernel.history().len(), 0);
        Ok(())
    }

    #[test]
    fn test_variable_assignment() -> Result<()> {
        let mut kernel = OxiGdalKernel::new()?;
        let result = kernel.execute("let x = 42")?;
        assert_eq!(result.status, "ok");
        assert!(kernel.namespace().contains_key("x"));
        Ok(())
    }

    #[test]
    fn test_completion() -> Result<()> {
        let mut kernel = OxiGdalKernel::new()?;
        kernel.execute("let raster = 1")?;

        let result = kernel.complete("%plo", 4)?;
        assert!(result.matches.contains(&"%plot".to_string()));

        let result = kernel.complete("ras", 3)?;
        assert!(result.matches.contains(&"raster".to_string()));
        Ok(())
    }

    #[test]
    fn test_magic_command_help() -> Result<()> {
        let kernel = OxiGdalKernel::new()?;
        let result = kernel.inspect("%load_raster", 0)?;
        assert!(result.found);
        Ok(())
    }
}
