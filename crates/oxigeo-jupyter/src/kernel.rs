//! Jupyter kernel implementation for OxiGeo
//!
//! This module provides a custom Jupyter kernel that supports OxiGeo operations
//! with rich display and interactive features.

use crate::Result;
use oxigeo_core::types::RasterMetadata;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// OxiGeo Jupyter kernel
pub struct OxiGeoKernel {
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
    /// Loaded raster dataset: the source path plus metadata parsed from the
    /// file at load time (CRS, geotransform, bands, data type, nodata, ...).
    Raster(Box<RasterHandle>),
}

/// A raster dataset that has actually been opened and parsed via
/// `oxigeo-geotiff`. Kept alongside the source path so that operations that
/// need pixel data (e.g. `%stats`) can re-open the file and decode bands.
#[derive(Debug, Clone)]
pub struct RasterHandle {
    /// Path the raster was loaded from
    pub path: PathBuf,
    /// Metadata parsed from the file when it was loaded
    pub metadata: RasterMetadata,
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
            kernel_name: "oxigeo".to_string(),
            display_name: "OxiGeo".to_string(),
            language: "rust".to_string(),
            language_version: "1.89".to_string(),
            file_extension: ".rs".to_string(),
            mimetype: "text/x-rustsrc".to_string(),
        }
    }
}

impl OxiGeoKernel {
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

    /// Parse and execute code.
    ///
    /// This is **not** a general Rust interpreter. It recognizes exactly three
    /// statement shapes:
    ///
    /// 1. `let name = <expr>` — variable assignment.
    /// 2. `name` — echo a known variable's value.
    /// 3. `<expr>` — evaluate and echo a scalar expression.
    ///
    /// Where `<expr>` is a string/integer/float/boolean literal, a known
    /// variable name, or a single binary arithmetic operation (`a + b`,
    /// `a - b`, `a * b`, `a / b`) between two such operands. Anything outside
    /// this grammar — real Rust syntax, function/OxiGeo API calls, chained or
    /// parenthesized arithmetic — is rejected with an honest
    /// [`JupyterError::Kernel`] error rather than being silently ignored.
    fn parse_and_execute(&mut self, code: &str) -> Result<Option<HashMap<String, String>>> {
        let code = code.trim();

        if code.is_empty() {
            return Ok(None);
        }

        // Variable assignment: let name = <expr>
        if code.starts_with("let ")
            && let Some((name, value)) = code.strip_prefix("let ").and_then(|s| s.split_once('='))
        {
            let name = name.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(crate::JupyterError::Kernel(format!(
                    "Invalid variable name in assignment: '{name}'"
                )));
            }
            let value_str = value.trim();
            let val = self.eval_simple_expr(value_str)?;
            self.namespace.insert(name.to_string(), val);
            return Ok(None);
        }

        // Bare variable echo
        if let Some(var) = self.namespace.get(code) {
            let mut output = HashMap::new();
            output.insert("text/plain".to_string(), format!("{var:?}"));
            return Ok(Some(output));
        }

        // Scalar expression (literal or arithmetic over known variables)
        let value = self.eval_simple_expr(code)?;
        let mut output = HashMap::new();
        output.insert("text/plain".to_string(), format!("{value:?}"));
        Ok(Some(output))
    }

    /// Evaluates a scalar expression: a literal, a known variable, or a
    /// single binary arithmetic operation between two such operands.
    ///
    /// This deliberately rejects (rather than silently mis-evaluates) any
    /// expression with more than one top-level operator, since correctly
    /// handling operator precedence and associativity would require a real
    /// parser, which is out of scope for this demo evaluator.
    fn eval_simple_expr(&self, expr: &str) -> Result<Value> {
        let expr = expr.trim();

        if expr.is_empty() {
            return Err(crate::JupyterError::Kernel(
                "Unsupported statement: empty expression".to_string(),
            ));
        }

        // String literal
        if expr.len() >= 2 && expr.starts_with('"') && expr.ends_with('"') {
            return Ok(Value::String(expr[1..expr.len() - 1].to_string()));
        }

        // Boolean literal
        if expr == "true" || expr == "false" {
            return Ok(Value::Boolean(expr == "true"));
        }

        // Integer literal
        if let Ok(i) = expr.parse::<i64>() {
            return Ok(Value::Integer(i));
        }

        // Float literal
        if let Ok(f) = expr.parse::<f64>() {
            return Ok(Value::Float(f));
        }

        // Known variable
        if let Some(v) = self.namespace.get(expr) {
            return Ok(v.clone());
        }

        // Single binary arithmetic operation
        let ops = Self::find_top_level_operators(expr);
        match ops.len() {
            0 => Err(crate::JupyterError::Kernel(format!(
                "Unsupported statement: '{expr}'. This kernel evaluates \
                 `let name = <expr>`, bare variable echoes, and single binary \
                 arithmetic expressions (`a + b`, `a - b`, `a * b`, `a / b`) \
                 over known variables/literals; it is not a full Rust \
                 interpreter and this input matched none of those shapes."
            ))),
            1 => {
                let (op_pos, op) = ops[0];
                let lhs_str = expr[..op_pos].trim();
                let rhs_str = expr[op_pos + op.len_utf8()..].trim();
                if lhs_str.is_empty() || rhs_str.is_empty() {
                    return Err(crate::JupyterError::Kernel(format!(
                        "Unsupported statement: '{expr}' has an empty operand around '{op}'"
                    )));
                }
                let lhs = self.eval_simple_expr(lhs_str)?;
                let rhs = self.eval_simple_expr(rhs_str)?;
                Self::apply_binary_op(&lhs, op, &rhs)
            }
            n => Err(crate::JupyterError::Kernel(format!(
                "Unsupported statement: '{expr}' contains {n} operators; only a \
                 single binary arithmetic operation (`a op b`) is supported, not \
                 chained or parenthesized expressions"
            ))),
        }
    }

    /// Finds every top-level `+ - * /` operator in `expr`, skipping a
    /// leading sign (index 0) and anything inside a `"..."` string literal.
    fn find_top_level_operators(expr: &str) -> Vec<(usize, char)> {
        let mut in_quotes = false;
        let mut ops = Vec::new();
        for (idx, (byte_pos, c)) in expr.char_indices().enumerate() {
            if c == '"' {
                in_quotes = !in_quotes;
                continue;
            }
            if in_quotes || idx == 0 {
                continue;
            }
            if matches!(c, '+' | '-' | '*' | '/') {
                ops.push((byte_pos, c));
            }
        }
        ops
    }

    /// Extracts the numeric value of an `Integer` or `Float`, or `None` for
    /// any other variant.
    fn as_f64(value: &Value) -> Option<f64> {
        match value {
            Value::Integer(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Applies a single binary arithmetic operator to two values. Integer +
    /// Integer stays integer (with truncating division and a division-by-zero
    /// error, matching Rust's own `i64` semantics); any other numeric
    /// combination is promoted to `Float`. Non-numeric operands are an
    /// honest error rather than a silently wrong result.
    fn apply_binary_op(lhs: &Value, op: char, rhs: &Value) -> Result<Value> {
        if let (Value::Integer(a), Value::Integer(b)) = (lhs, rhs) {
            let (a, b) = (*a, *b);
            return match op {
                '+' => Ok(Value::Integer(a + b)),
                '-' => Ok(Value::Integer(a - b)),
                '*' => Ok(Value::Integer(a * b)),
                '/' => {
                    if b == 0 {
                        Err(crate::JupyterError::Kernel(
                            "Division by zero in integer expression".to_string(),
                        ))
                    } else {
                        Ok(Value::Integer(a / b))
                    }
                }
                _ => Err(crate::JupyterError::Kernel(format!(
                    "Unsupported operator '{op}'"
                ))),
            };
        }

        match (Self::as_f64(lhs), Self::as_f64(rhs)) {
            (Some(a), Some(b)) => match op {
                '+' => Ok(Value::Float(a + b)),
                '-' => Ok(Value::Float(a - b)),
                '*' => Ok(Value::Float(a * b)),
                '/' => {
                    if b == 0.0 {
                        Err(crate::JupyterError::Kernel(
                            "Division by zero in floating-point expression".to_string(),
                        ))
                    } else {
                        Ok(Value::Float(a / b))
                    }
                }
                _ => Err(crate::JupyterError::Kernel(format!(
                    "Unsupported operator '{op}'"
                ))),
            },
            _ => Err(crate::JupyterError::Kernel(format!(
                "Cannot apply operator '{op}' to non-numeric values: {lhs:?}, {rhs:?}"
            ))),
        }
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
        let kernel = OxiGeoKernel::new()?;
        assert_eq!(kernel.execution_count(), 0);
        assert_eq!(kernel.history().len(), 0);
        Ok(())
    }

    #[test]
    fn test_variable_assignment() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        let result = kernel.execute("let x = 42")?;
        assert_eq!(result.status, "ok");
        assert!(kernel.namespace().contains_key("x"));
        Ok(())
    }

    #[test]
    fn test_completion() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let raster = 1")?;

        let result = kernel.complete("%plo", 4)?;
        assert!(result.matches.contains(&"%plot".to_string()));

        let result = kernel.complete("ras", 3)?;
        assert!(result.matches.contains(&"raster".to_string()));
        Ok(())
    }

    #[test]
    fn test_magic_command_help() -> Result<()> {
        let kernel = OxiGeoKernel::new()?;
        let result = kernel.inspect("%load_raster", 0)?;
        assert!(result.found);
        Ok(())
    }

    #[test]
    fn test_custom_config() -> Result<()> {
        let config = KernelConfig {
            kernel_name: "test_kernel".to_string(),
            display_name: "Test Kernel".to_string(),
            language: "python".to_string(),
            language_version: "3.11".to_string(),
            file_extension: ".py".to_string(),
            mimetype: "text/x-python".to_string(),
        };
        let kernel = OxiGeoKernel::with_config(config)?;
        assert_eq!(kernel.config().kernel_name, "test_kernel");
        assert_eq!(kernel.config().language, "python");
        Ok(())
    }

    #[test]
    fn test_default_config() -> Result<()> {
        let kernel = OxiGeoKernel::new()?;
        assert_eq!(kernel.config().kernel_name, "oxigeo");
        assert_eq!(kernel.config().language, "rust");
        Ok(())
    }

    #[test]
    fn test_execution_count_increments() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        assert_eq!(kernel.execution_count(), 0);
        kernel.execute("let a = 1")?;
        assert_eq!(kernel.execution_count(), 1);
        kernel.execute("let b = 2")?;
        assert_eq!(kernel.execution_count(), 2);
        Ok(())
    }

    #[test]
    fn test_history_tracking() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let x = 10")?;
        kernel.execute("let y = 20")?;
        let history = kernel.history();
        assert_eq!(history.len(), 2);
        assert!(history[0].contains("x"));
        assert!(history[1].contains("y"));
        Ok(())
    }

    #[test]
    fn test_string_variable_assignment() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute(r#"let name = "hello""#)?;
        let ns = kernel.namespace();
        assert!(ns.contains_key("name"));
        assert!(
            matches!(ns.get("name"), Some(Value::String(s)) if s == "hello"),
            "Expected String value"
        );
        Ok(())
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_float_variable_assignment() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let pi = 3.14")?;
        let ns = kernel.namespace();
        assert!(ns.contains_key("pi"));
        match ns.get("pi") {
            Some(Value::Float(f)) => assert!((f - 3.14_f64).abs() < 1e-10),
            other => {
                return Err(crate::JupyterError::Kernel(format!(
                    "Expected Float value, got {other:?}"
                )));
            }
        }
        Ok(())
    }

    #[test]
    fn test_boolean_variable_assignment() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let flag = true")?;
        let ns = kernel.namespace();
        assert!(ns.contains_key("flag"));
        assert!(
            matches!(ns.get("flag"), Some(Value::Boolean(true))),
            "Expected Boolean true value"
        );
        Ok(())
    }

    #[test]
    fn test_clear_namespace() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let x = 1")?;
        kernel.execute("let y = 2")?;
        assert!(!kernel.namespace().is_empty());
        kernel.clear_namespace();
        assert!(kernel.namespace().is_empty());
        Ok(())
    }

    #[test]
    fn test_inspect_variable() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let myvar = 42")?;
        let result = kernel.inspect("myvar", 5)?;
        assert!(result.found);
        let text = result.data.get("text/plain");
        assert!(text.is_some());
        Ok(())
    }

    #[test]
    fn test_inspect_unknown_returns_not_found() -> Result<()> {
        let kernel = OxiGeoKernel::new()?;
        let result = kernel.inspect("nonexistent", 5)?;
        assert!(!result.found);
        Ok(())
    }

    #[test]
    fn test_complete_magic_prefix() -> Result<()> {
        let kernel = OxiGeoKernel::new()?;
        let result = kernel.complete("%", 1)?;
        assert!(!result.matches.is_empty());
        Ok(())
    }

    #[test]
    fn test_complete_keywords() -> Result<()> {
        let kernel = OxiGeoKernel::new()?;
        let result = kernel.complete("le", 2)?;
        assert!(result.matches.contains(&"let".to_string()));
        Ok(())
    }

    #[test]
    fn test_execution_result_has_ok_status() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        let result = kernel.execute("let z = 99")?;
        assert_eq!(result.status, "ok");
        assert!(result.error.is_none());
        Ok(())
    }

    #[test]
    fn test_kernel_config_serialization() -> Result<()> {
        let config = KernelConfig::default();
        let json = serde_json::to_string(&config).map_err(crate::JupyterError::Serialization)?;
        assert!(json.contains("oxigeo"));
        let parsed: KernelConfig =
            serde_json::from_str(&json).map_err(crate::JupyterError::Serialization)?;
        assert_eq!(parsed.kernel_name, "oxigeo");
        Ok(())
    }

    #[test]
    fn test_help_for_all_magic_commands() -> Result<()> {
        let kernel = OxiGeoKernel::new()?;
        let commands = [
            "%load_raster",
            "%plot",
            "%info",
            "%crs",
            "%bounds",
            "%stats",
        ];
        for cmd in &commands {
            let result = kernel.inspect(cmd, 0)?;
            assert!(result.found, "Help not found for {}", cmd);
        }
        Ok(())
    }

    #[test]
    fn test_unsupported_statement_reports_error_not_silent_ok() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        // A real Rust expression this toy evaluator does not understand.
        let result = kernel.execute("fn foo() -> i32 { 42 }")?;
        assert_eq!(result.status, "error");
        assert!(result.data.is_none());
        let err = result
            .error
            .as_ref()
            .ok_or_else(|| crate::JupyterError::Kernel("expected error info".to_string()))?;
        assert_eq!(err.ename, "ExecutionError");
        assert!(err.evalue.contains("Unsupported statement"));
        Ok(())
    }

    #[test]
    fn test_unknown_variable_echo_is_an_honest_error() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        let result = kernel.execute("does_not_exist")?;
        assert_eq!(result.status, "error");
        Ok(())
    }

    #[test]
    fn test_chained_arithmetic_is_rejected_not_miscomputed() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let a = 10")?;
        kernel.execute("let b = 2")?;
        kernel.execute("let c = 3")?;
        // Ambiguous precedence/associativity across >1 operator: must error,
        // never silently return a (possibly wrong) numeric answer.
        let result = kernel.execute("a - b - c")?;
        assert_eq!(result.status, "error");
        Ok(())
    }

    #[test]
    fn test_integer_arithmetic_addition() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let a = 10")?;
        kernel.execute("let b = 32")?;
        let result = kernel.execute("a + b")?;
        assert_eq!(result.status, "ok");
        let text = result
            .data
            .as_ref()
            .and_then(|d| d.get("text/plain"))
            .ok_or_else(|| crate::JupyterError::Kernel("expected output".to_string()))?;
        assert_eq!(text, "Integer(42)");
        Ok(())
    }

    #[test]
    fn test_float_arithmetic_division() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let a = 10")?;
        kernel.execute("let b = 4.0")?;
        let result = kernel.execute("a / b")?;
        assert_eq!(result.status, "ok");
        let text = result
            .data
            .as_ref()
            .and_then(|d| d.get("text/plain"))
            .ok_or_else(|| crate::JupyterError::Kernel("expected output".to_string()))?;
        assert_eq!(text, "Float(2.5)");
        Ok(())
    }

    #[test]
    fn test_integer_division_by_zero_is_an_error() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let a = 10")?;
        kernel.execute("let b = 0")?;
        let result = kernel.execute("a / b")?;
        assert_eq!(result.status, "error");
        Ok(())
    }

    #[test]
    fn test_assign_arithmetic_expression_to_variable() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute("let a = 2")?;
        kernel.execute("let b = 3")?;
        kernel.execute("let c = a * b")?;
        assert!(
            matches!(kernel.namespace().get("c"), Some(Value::Integer(6))),
            "Expected c = 6"
        );
        Ok(())
    }

    #[test]
    fn test_arithmetic_on_non_numeric_value_is_an_error() -> Result<()> {
        let mut kernel = OxiGeoKernel::new()?;
        kernel.execute(r#"let a = "hello""#)?;
        kernel.execute("let b = 1")?;
        let result = kernel.execute("a + b")?;
        assert_eq!(result.status, "error");
        Ok(())
    }
}
