//! A focused, dependency-free JSONPath evaluator.
//!
//! Supports the common JSONPath subset used for request/response body extraction:
//!
//! - `$`                      — the root
//! - `.name` / `['name']`     — object member access
//! - `[index]`                — array index (negative indexes count from the end)
//! - `.*` / `[*]`             — wildcard over all array elements or object values
//! - `..name`                 — recursive descent to every `name` member at any depth
//! - `..*`                    — recursive descent to every value
//!
//! Evaluation returns the list of matched nodes (JSONPath is inherently multi-valued). The
//! caller decides how to render them; a malformed expression is a hard error rather than a
//! silent passthrough.

use crate::error::{GatewayError, Result};
use serde_json::Value;

/// A single navigation step in a parsed JSONPath expression.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Key(String),
    Index(i64),
    Wildcard,
    RecursiveKey(String),
    RecursiveWildcard,
}

/// A parsed JSONPath expression.
pub struct JsonPath {
    steps: Vec<Step>,
    /// True when every step is a plain `Key`/`Index` (no wildcard or recursion), so the
    /// expression addresses at most one node.
    deterministic: bool,
}

impl JsonPath {
    /// Parses a JSONPath expression. The leading `$` is optional.
    pub fn parse(expr: &str) -> Result<Self> {
        let bytes = expr.as_bytes();
        let mut i = 0;
        let len = bytes.len();

        // Optional leading root marker.
        if i < len && bytes[i] == b'$' {
            i += 1;
        }

        let mut steps = Vec::new();
        while i < len {
            match bytes[i] {
                b'.' => {
                    // `..` = recursive descent, `.` = child.
                    if i + 1 < len && bytes[i + 1] == b'.' {
                        i += 2;
                        // `..*` or `..name`
                        if i < len && bytes[i] == b'*' {
                            steps.push(Step::RecursiveWildcard);
                            i += 1;
                        } else if i < len && bytes[i] == b'[' {
                            // `..[...]` — descend then bracketed selector; treat as recursive
                            // then apply the bracket normally.
                            let (step, ni) = parse_bracket(bytes, i, expr)?;
                            // Wrap a bracket key/index under recursion where it makes sense.
                            steps.push(match step {
                                Step::Key(k) => Step::RecursiveKey(k),
                                Step::Wildcard => Step::RecursiveWildcard,
                                other => other,
                            });
                            i = ni;
                        } else {
                            let (name, ni) = parse_ident(bytes, i, expr)?;
                            steps.push(Step::RecursiveKey(name));
                            i = ni;
                        }
                    } else {
                        i += 1;
                        if i < len && bytes[i] == b'*' {
                            steps.push(Step::Wildcard);
                            i += 1;
                        } else {
                            let (name, ni) = parse_ident(bytes, i, expr)?;
                            steps.push(Step::Key(name));
                            i = ni;
                        }
                    }
                }
                b'[' => {
                    let (step, ni) = parse_bracket(bytes, i, expr)?;
                    steps.push(step);
                    i = ni;
                }
                b' ' | b'\t' => i += 1,
                other => {
                    // A bare leading identifier (e.g. "name.sub" without `$`/`.`).
                    if steps.is_empty() && (other.is_ascii_alphanumeric() || other == b'_') {
                        let (name, ni) = parse_ident(bytes, i, expr)?;
                        steps.push(Step::Key(name));
                        i = ni;
                    } else {
                        return Err(GatewayError::TransformationError(format!(
                            "invalid JSONPath '{expr}' near byte {i}"
                        )));
                    }
                }
            }
        }

        let deterministic = steps
            .iter()
            .all(|s| matches!(s, Step::Key(_) | Step::Index(_)));

        Ok(Self {
            steps,
            deterministic,
        })
    }

    /// Evaluates the expression against `root`, returning every matched node.
    pub fn evaluate<'a>(&self, root: &'a Value) -> Vec<&'a Value> {
        let mut current: Vec<&Value> = vec![root];
        for step in &self.steps {
            let mut next: Vec<&Value> = Vec::new();
            for node in &current {
                apply_step(node, step, &mut next);
            }
            current = next;
        }
        current
    }

    /// Applies the expression and renders the result as JSON bytes.
    ///
    /// For a deterministic path (no wildcard/recursion) the single matched value is returned
    /// as-is, and a no-match is a hard error. Otherwise a JSON array of all matches is
    /// returned (possibly empty).
    pub fn apply_to_bytes(&self, body: &[u8]) -> Result<Vec<u8>> {
        let root: Value = serde_json::from_slice(body)?;
        let matches = self.evaluate(&root);

        if self.deterministic {
            match matches.first() {
                Some(value) => Ok(serde_json::to_vec(value)?),
                None => Err(GatewayError::TransformationError(
                    "JSONPath expression matched no value".to_string(),
                )),
            }
        } else {
            let owned: Vec<Value> = matches.into_iter().cloned().collect();
            Ok(serde_json::to_vec(&owned)?)
        }
    }
}

fn apply_step<'a>(node: &'a Value, step: &Step, out: &mut Vec<&'a Value>) {
    match step {
        Step::Key(key) => {
            if let Value::Object(map) = node
                && let Some(v) = map.get(key)
            {
                out.push(v);
            }
        }
        Step::Index(idx) => {
            if let Value::Array(arr) = node
                && let Some(v) = resolve_index(arr, *idx)
            {
                out.push(v);
            }
        }
        Step::Wildcard => match node {
            Value::Array(arr) => out.extend(arr.iter()),
            Value::Object(map) => out.extend(map.values()),
            _ => {}
        },
        Step::RecursiveKey(key) => collect_recursive_key(node, key, out),
        Step::RecursiveWildcard => collect_recursive_all(node, out),
    }
}

fn resolve_index(arr: &[Value], idx: i64) -> Option<&Value> {
    let len = arr.len() as i64;
    let real = if idx < 0 { len + idx } else { idx };
    if real >= 0 && real < len {
        arr.get(real as usize)
    } else {
        None
    }
}

fn collect_recursive_key<'a>(node: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    out.push(v);
                }
                collect_recursive_key(v, key, out);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_recursive_key(v, key, out);
            }
        }
        _ => {}
    }
}

fn collect_recursive_all<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    match node {
        Value::Object(map) => {
            for v in map.values() {
                out.push(v);
                collect_recursive_all(v, out);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                out.push(v);
                collect_recursive_all(v, out);
            }
        }
        _ => {}
    }
}

fn parse_ident(bytes: &[u8], start: usize, expr: &str) -> Result<(String, usize)> {
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
            i += 1;
        } else {
            break;
        }
    }
    if i == start {
        return Err(GatewayError::TransformationError(format!(
            "expected identifier in JSONPath '{expr}' at byte {start}"
        )));
    }
    Ok((String::from_utf8_lossy(&bytes[start..i]).to_string(), i))
}

/// Parses a `[...]` selector starting at `bytes[start] == '['`.
fn parse_bracket(bytes: &[u8], start: usize, expr: &str) -> Result<(Step, usize)> {
    let len = bytes.len();
    let mut i = start + 1; // skip '['
    // Skip whitespace.
    while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= len {
        return Err(GatewayError::TransformationError(format!(
            "unterminated '[' in JSONPath '{expr}'"
        )));
    }

    let step = if bytes[i] == b'*' {
        i += 1;
        Step::Wildcard
    } else if bytes[i] == b'\'' || bytes[i] == b'"' {
        let quote = bytes[i];
        i += 1;
        let key_start = i;
        while i < len && bytes[i] != quote {
            i += 1;
        }
        if i >= len {
            return Err(GatewayError::TransformationError(format!(
                "unterminated quoted key in JSONPath '{expr}'"
            )));
        }
        let key = String::from_utf8_lossy(&bytes[key_start..i]).to_string();
        i += 1; // skip closing quote
        Step::Key(key)
    } else {
        // Integer index (optionally negative).
        let num_start = i;
        if bytes[i] == b'-' {
            i += 1;
        }
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let num_str = String::from_utf8_lossy(&bytes[num_start..i]);
        let idx = num_str.parse::<i64>().map_err(|_| {
            GatewayError::TransformationError(format!(
                "invalid array index '{num_str}' in JSONPath '{expr}'"
            ))
        })?;
        Step::Index(idx)
    };

    // Skip whitespace then require ']'.
    while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= len || bytes[i] != b']' {
        return Err(GatewayError::TransformationError(format!(
            "expected ']' in JSONPath '{expr}' at byte {i}"
        )));
    }
    i += 1; // skip ']'

    Ok((step, i))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn eval_path(expr: &str, json: &str) -> Vec<u8> {
        JsonPath::parse(expr)
            .expect("parse")
            .apply_to_bytes(json.as_bytes())
            .expect("apply")
    }

    #[test]
    fn test_simple_member() {
        let out = eval_path("$.name", r#"{"name":"alice","age":30}"#);
        assert_eq!(String::from_utf8_lossy(&out), "\"alice\"");
    }

    #[test]
    fn test_nested_member() {
        let out = eval_path("$.a.b.c", r#"{"a":{"b":{"c":42}}}"#);
        assert_eq!(String::from_utf8_lossy(&out), "42");
    }

    #[test]
    fn test_array_index() {
        let out = eval_path("$.items[1]", r#"{"items":[10,20,30]}"#);
        assert_eq!(String::from_utf8_lossy(&out), "20");
    }

    #[test]
    fn test_negative_index() {
        let out = eval_path("$.items[-1]", r#"{"items":[10,20,30]}"#);
        assert_eq!(String::from_utf8_lossy(&out), "30");
    }

    #[test]
    fn test_wildcard_array() {
        let out = eval_path("$.items[*]", r#"{"items":[1,2,3]}"#);
        assert_eq!(String::from_utf8_lossy(&out), "[1,2,3]");
    }

    #[test]
    fn test_bracket_quoted_key() {
        let out = eval_path("$['first name']", r#"{"first name":"bob"}"#);
        assert_eq!(String::from_utf8_lossy(&out), "\"bob\"");
    }

    #[test]
    fn test_recursive_key() {
        let out = eval_path("$..id", r#"{"id":1,"child":{"id":2,"child":{"id":3}}}"#);
        assert_eq!(String::from_utf8_lossy(&out), "[1,2,3]");
    }

    #[test]
    fn test_no_match_deterministic_errors() {
        let path = JsonPath::parse("$.missing").expect("parse");
        let result = path.apply_to_bytes(br#"{"present":1}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_expression_errors() {
        assert!(JsonPath::parse("$.a[").is_err());
    }
}
