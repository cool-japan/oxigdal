//! WGSL shader compiler and optimizer.

pub mod analyzer;
pub mod cache;
pub mod optimizer;

use crate::error::{GpuAdvancedError, Result};
use blake3::Hash;
use naga::{Module, valid::Validator};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Shader compilation result
pub struct CompiledShader {
    /// Original source code
    pub source: String,
    /// Naga module
    pub module: Module,
    /// Entry points
    pub entry_points: Vec<String>,
    /// Compilation hash
    pub hash: Hash,
    /// Whether shader was optimized
    pub optimized: bool,
}

/// Shader compiler with optimization and caching
pub struct ShaderCompiler {
    /// Shader cache
    cache: Arc<cache::ShaderCache>,
    /// Optimizer
    optimizer: Arc<optimizer::ShaderOptimizer>,
    /// Compilation statistics
    stats: Arc<RwLock<CompilerStats>>,
}

/// Compiler statistics
#[derive(Debug, Default, Clone)]
pub struct CompilerStats {
    /// Total compilations
    pub total_compilations: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Cache misses
    pub cache_misses: u64,
    /// Optimizations performed
    pub optimizations: u64,
    /// Validation failures
    pub validation_failures: u64,
}

impl ShaderCompiler {
    /// Create a new shader compiler
    pub fn new() -> Self {
        Self {
            cache: Arc::new(cache::ShaderCache::new(1000)),
            optimizer: Arc::new(optimizer::ShaderOptimizer::new()),
            stats: Arc::new(RwLock::new(CompilerStats::default())),
        }
    }

    /// Compile WGSL source code
    pub fn compile(&self, source: &str) -> Result<CompiledShader> {
        // Update stats
        {
            let mut stats = self.stats.write();
            stats.total_compilations += 1;
        }

        // Calculate hash
        let hash = blake3::hash(source.as_bytes());

        // Check cache
        if let Some(cached) = self.cache.get(&hash) {
            let mut stats = self.stats.write();
            stats.cache_hits += 1;
            return Ok(cached);
        }

        // Cache miss
        {
            let mut stats = self.stats.write();
            stats.cache_misses += 1;
        }

        // Parse WGSL
        let module = naga::front::wgsl::parse_str(source).map_err(|e| {
            GpuAdvancedError::ShaderCompilerError(format!("WGSL parse error: {:?}", e))
        })?;

        // Validate
        let mut validator = Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );

        let _module_info = validator.validate(&module).map_err(|e| {
            let mut stats = self.stats.write();
            stats.validation_failures += 1;
            GpuAdvancedError::ShaderValidationError(format!("Validation error: {:?}", e))
        })?;

        // Extract entry points
        let entry_points: Vec<String> = module
            .entry_points
            .iter()
            .map(|ep| ep.name.clone())
            .collect();

        let compiled = CompiledShader {
            source: source.to_string(),
            module,
            entry_points,
            hash,
            optimized: false,
        };

        // Cache the result
        self.cache.insert(hash, compiled.clone());

        Ok(compiled)
    }

    /// Compile and optimize
    pub fn compile_optimized(&self, source: &str) -> Result<CompiledShader> {
        let mut compiled = self.compile(source)?;

        // Apply optimizations
        compiled.module = self.optimizer.optimize(&compiled.module)?;
        compiled.optimized = true;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.optimizations += 1;
        }

        Ok(compiled)
    }

    /// Validate shader without compilation
    pub fn validate(&self, source: &str) -> Result<()> {
        let module = naga::front::wgsl::parse_str(source).map_err(|e| {
            GpuAdvancedError::ShaderCompilerError(format!("WGSL parse error: {:?}", e))
        })?;

        let mut validator = Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );

        validator.validate(&module).map_err(|e| {
            GpuAdvancedError::ShaderValidationError(format!("Validation error: {:?}", e))
        })?;

        Ok(())
    }

    /// Get compiler statistics
    pub fn get_stats(&self) -> CompilerStats {
        self.stats.read().clone()
    }

    /// Print compiler statistics
    pub fn print_stats(&self) {
        let stats = self.stats.read();
        println!("\nShader Compiler Statistics:");
        println!("  Total compilations: {}", stats.total_compilations);
        println!(
            "  Cache hits: {} ({:.1}%)",
            stats.cache_hits,
            if stats.total_compilations > 0 {
                (stats.cache_hits as f64 / stats.total_compilations as f64) * 100.0
            } else {
                0.0
            }
        );
        println!("  Cache misses: {}", stats.cache_misses);
        println!("  Optimizations: {}", stats.optimizations);
        println!("  Validation failures: {}", stats.validation_failures);
    }

    /// Clear cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Get cache
    pub fn cache(&self) -> Arc<cache::ShaderCache> {
        self.cache.clone()
    }

    /// Get optimizer
    pub fn optimizer(&self) -> Arc<optimizer::ShaderOptimizer> {
        self.optimizer.clone()
    }
}

impl Default for ShaderCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CompiledShader {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            module: self.module.clone(),
            entry_points: self.entry_points.clone(),
            hash: self.hash,
            optimized: self.optimized,
        }
    }
}

/// Shader preprocessor for macro expansion
pub struct ShaderPreprocessor {
    /// Defined macros
    defines: HashMap<String, String>,
}

impl ShaderPreprocessor {
    /// Create a new preprocessor
    pub fn new() -> Self {
        Self {
            defines: HashMap::new(),
        }
    }

    /// Define a macro
    pub fn define(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.defines.insert(name.into(), value.into());
    }

    /// Undefine a macro
    pub fn undefine(&mut self, name: &str) {
        self.defines.remove(name);
    }

    /// Preprocess source code
    pub fn preprocess(&self, source: &str) -> String {
        let mut result = source.to_string();

        // Simple macro replacement
        for (name, value) in &self.defines {
            let pattern = format!("${}", name);
            result = result.replace(&pattern, value);
        }

        result
    }
}

impl Default for ShaderPreprocessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_preprocessor() {
        let mut preprocessor = ShaderPreprocessor::new();
        preprocessor.define("WORKGROUP_SIZE", "64");

        let source = "@compute @workgroup_size($WORKGROUP_SIZE, 1, 1)\nfn main() {}";
        let result = preprocessor.preprocess(source);

        assert!(result.contains("64"));
    }

    #[test]
    fn test_compiler_creation() {
        let compiler = ShaderCompiler::new();
        let stats = compiler.get_stats();
        assert_eq!(stats.total_compilations, 0);
    }

    #[test]
    fn test_simple_shader_compilation() {
        let compiler = ShaderCompiler::new();
        let source = r#"
@compute @workgroup_size(1, 1, 1)
fn main() {
    // Empty compute shader
}
        "#;

        let result = compiler.compile(source);
        assert!(result.is_ok());

        if let Ok(compiled) = result {
            assert!(compiled.entry_points.contains(&"main".to_string()));
        }
    }
}
