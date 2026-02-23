//! Variable binding and environment management
//!
//! This module provides variable scoping and lookup for the DSL.

use super::ast::{Expr, Type};
use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, collections::BTreeMap as HashMap, string::String, vec::Vec};

#[cfg(feature = "std")]
use std::collections::HashMap;

/// Value type in the DSL runtime
#[derive(Debug, Clone)]
pub enum Value {
    /// Numeric value
    Number(f64),
    /// Boolean value
    Bool(bool),
    /// Raster buffer
    Raster(Box<RasterBuffer>),
    /// Function closure
    Function {
        /// Function parameter names
        params: Vec<String>,
        /// Function body expression
        body: Box<Expr>,
        /// Captured environment
        env: Environment,
    },
}

impl Value {
    /// Gets the type of this value
    pub fn get_type(&self) -> Type {
        match self {
            Value::Number(_) => Type::Number,
            Value::Bool(_) => Type::Bool,
            Value::Raster(_) => Type::Raster,
            Value::Function { .. } => Type::Unknown,
        }
    }

    /// Converts value to f64 if possible
    pub fn as_number(&self) -> Result<f64> {
        match self {
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "value",
                message: "Cannot convert to number".to_string(),
            }),
        }
    }

    /// Converts value to bool if possible
    pub fn as_bool(&self) -> Result<bool> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Number(n) => Ok(n.abs() > f64::EPSILON),
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "value",
                message: "Cannot convert to bool".to_string(),
            }),
        }
    }

    /// Gets raster buffer reference
    pub fn as_raster(&self) -> Result<&RasterBuffer> {
        match self {
            Value::Raster(r) => Ok(r),
            _ => Err(AlgorithmError::InvalidParameter {
                parameter: "value",
                message: "Not a raster".to_string(),
            }),
        }
    }
}

/// Variable environment for scoping
#[derive(Debug, Clone)]
pub struct Environment {
    /// Variable bindings in this scope
    bindings: HashMap<String, Value>,
    /// Parent scope (for nested scopes)
    parent: Option<Box<Environment>>,
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

impl Environment {
    /// Creates a new empty environment
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            parent: None,
        }
    }

    /// Creates a new child environment
    pub fn with_parent(parent: Environment) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    /// Defines a variable in the current scope
    pub fn define(&mut self, name: String, value: Value) {
        self.bindings.insert(name, value);
    }

    /// Looks up a variable in this scope or parent scopes
    pub fn lookup(&self, name: &str) -> Result<&Value> {
        if let Some(value) = self.bindings.get(name) {
            Ok(value)
        } else if let Some(parent) = &self.parent {
            parent.lookup(name)
        } else {
            Err(AlgorithmError::InvalidParameter {
                parameter: "variable",
                message: format!("Undefined variable: {name}"),
            })
        }
    }

    /// Checks if a variable is defined
    pub fn is_defined(&self, name: &str) -> bool {
        self.bindings.contains_key(name) || self.parent.as_ref().is_some_and(|p| p.is_defined(name))
    }

    /// Updates a variable in the current scope or parent scopes
    pub fn update(&mut self, name: &str, value: Value) -> Result<()> {
        if self.bindings.contains_key(name) {
            self.bindings.insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &mut self.parent {
            parent.update(name, value)
        } else {
            Err(AlgorithmError::InvalidParameter {
                parameter: "variable",
                message: format!("Undefined variable: {name}"),
            })
        }
    }

    /// Gets all variable names in this scope
    pub fn variables(&self) -> Vec<String> {
        self.bindings.keys().cloned().collect()
    }

    /// Gets the number of bindings in this scope (not including parents)
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Checks if this environment is empty
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Merges another environment into this one
    pub fn merge(&mut self, other: Environment) {
        for (name, value) in other.bindings {
            self.bindings.insert(name, value);
        }
    }

    /// Creates a snapshot of all bindings (flattened)
    pub fn snapshot(&self) -> HashMap<String, Value> {
        let mut result = HashMap::new();

        // Add parent bindings first
        if let Some(parent) = &self.parent {
            for (k, v) in parent.snapshot() {
                result.insert(k, v);
            }
        }

        // Add our bindings (overriding parent)
        for (k, v) in &self.bindings {
            result.insert(k.clone(), v.clone());
        }

        result
    }
}

/// Variable context for band references
#[derive(Debug, Clone)]
pub struct BandContext<'a> {
    bands: &'a [RasterBuffer],
}

impl<'a> BandContext<'a> {
    /// Creates a new band context
    pub fn new(bands: &'a [RasterBuffer]) -> Self {
        Self { bands }
    }

    /// Gets a band by index (1-based)
    pub fn get_band(&self, index: usize) -> Result<&RasterBuffer> {
        if index == 0 || index > self.bands.len() {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "band",
                message: format!("Band index {} out of range (1-{})", index, self.bands.len()),
            });
        }
        Ok(&self.bands[index - 1])
    }

    /// Gets the number of bands
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }

    /// Checks if a band index is valid
    pub fn is_valid_band(&self, index: usize) -> bool {
        index > 0 && index <= self.bands.len()
    }

    /// Gets all bands
    pub fn all_bands(&self) -> &[RasterBuffer] {
        self.bands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_environment_define_lookup() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(42.0));

        let val = env.lookup("x").expect("Should find x");
        assert!(matches!(val, Value::Number(n) if (n - 42.0).abs() < 1e-10));
    }

    #[test]
    fn test_environment_parent() {
        let mut parent = Environment::new();
        parent.define("x".to_string(), Value::Number(10.0));

        let mut child = Environment::with_parent(parent);
        child.define("y".to_string(), Value::Number(20.0));

        assert!(child.lookup("x").is_ok());
        assert!(child.lookup("y").is_ok());
        assert!(child.lookup("z").is_err());
    }

    #[test]
    fn test_environment_update() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(10.0));

        let result = env.update("x", Value::Number(20.0));
        assert!(result.is_ok());

        let val = env.lookup("x").expect("Should find x");
        assert!(matches!(val, Value::Number(n) if (n - 20.0).abs() < 1e-10));
    }

    #[test]
    fn test_band_context() {
        let bands = vec![
            RasterBuffer::zeros(10, 10, RasterDataType::Float32),
            RasterBuffer::zeros(10, 10, RasterDataType::Float32),
        ];

        let ctx = BandContext::new(&bands);
        assert_eq!(ctx.num_bands(), 2);
        assert!(ctx.is_valid_band(1));
        assert!(ctx.is_valid_band(2));
        assert!(!ctx.is_valid_band(0));
        assert!(!ctx.is_valid_band(3));
    }

    #[test]
    fn test_value_conversions() {
        let num = Value::Number(42.5);
        assert!((num.as_number().expect("Should convert") - 42.5).abs() < 1e-10);

        let bool_val = Value::Bool(true);
        assert!(bool_val.as_bool().expect("Should convert"));

        let zero = Value::Number(0.0);
        assert!(!zero.as_bool().expect("Should convert"));
    }

    #[test]
    fn test_environment_snapshot() {
        let mut parent = Environment::new();
        parent.define("x".to_string(), Value::Number(10.0));

        let mut child = Environment::with_parent(parent);
        child.define("y".to_string(), Value::Number(20.0));

        let snapshot = child.snapshot();
        assert_eq!(snapshot.len(), 2);
    }
}
