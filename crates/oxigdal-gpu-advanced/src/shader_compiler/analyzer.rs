//! Shader analysis and profiling tools.

use naga::{Expression, Function, Module, Statement};
use std::collections::HashMap;

/// Shader analysis result
#[derive(Debug, Clone)]
pub struct ShaderAnalysis {
    /// Number of instructions
    pub instruction_count: usize,
    /// Number of registers used (estimated)
    pub register_count: usize,
    /// Number of arithmetic operations
    pub arithmetic_ops: usize,
    /// Number of memory operations
    pub memory_ops: usize,
    /// Number of control flow operations
    pub control_flow_ops: usize,
    /// Estimated computational complexity
    pub complexity_score: f32,
    /// Entry points
    pub entry_points: Vec<String>,
    /// Function call graph
    pub call_graph: HashMap<String, Vec<String>>,
}

/// Shader analyzer
pub struct ShaderAnalyzer;

impl ShaderAnalyzer {
    /// Create a new analyzer
    pub fn new() -> Self {
        Self
    }

    /// Analyze a shader module
    pub fn analyze(&self, module: &Module) -> ShaderAnalysis {
        let mut analysis = ShaderAnalysis {
            instruction_count: 0,
            register_count: 0,
            arithmetic_ops: 0,
            memory_ops: 0,
            control_flow_ops: 0,
            complexity_score: 0.0,
            entry_points: Vec::new(),
            call_graph: HashMap::new(),
        };

        // Analyze entry points
        for ep in &module.entry_points {
            analysis.entry_points.push(ep.name.clone());
            self.analyze_function(&ep.function, &mut analysis);
        }

        // Analyze functions
        for (_handle, function) in module.functions.iter() {
            self.analyze_function(function, &mut analysis);
        }

        // Calculate complexity score
        analysis.complexity_score = self.calculate_complexity(&analysis);

        analysis
    }

    /// Analyze a function
    fn analyze_function(&self, function: &Function, analysis: &mut ShaderAnalysis) {
        // Analyze expressions
        for (_handle, expr) in function.expressions.iter() {
            self.analyze_expression(expr, analysis);
        }

        // Analyze statements
        for statement in &function.body {
            self.analyze_statement(statement, analysis);
        }

        // Estimate register usage based on local variables
        analysis.register_count += function.local_variables.len();
    }

    /// Analyze an expression
    fn analyze_expression(&self, expr: &Expression, analysis: &mut ShaderAnalysis) {
        analysis.instruction_count += 1;

        match expr {
            Expression::Binary { .. } => {
                analysis.arithmetic_ops += 1;
            }
            Expression::Math { .. } => {
                analysis.arithmetic_ops += 1;
            }
            Expression::Load { .. } => {
                analysis.memory_ops += 1;
            }
            Expression::AccessIndex { .. } | Expression::Access { .. } => {
                analysis.memory_ops += 1;
            }
            Expression::CallResult { .. } => {
                analysis.control_flow_ops += 1;
            }
            _ => {}
        }
    }

    /// Analyze a statement
    fn analyze_statement(&self, statement: &Statement, analysis: &mut ShaderAnalysis) {
        analysis.instruction_count += 1;

        match statement {
            Statement::Store { .. } => {
                analysis.memory_ops += 1;
            }
            Statement::If { .. } => {
                analysis.control_flow_ops += 1;
            }
            Statement::Switch { .. } => {
                analysis.control_flow_ops += 1;
            }
            Statement::Loop { .. } => {
                analysis.control_flow_ops += 1;
            }
            Statement::Return { .. } | Statement::Break | Statement::Continue => {
                analysis.control_flow_ops += 1;
            }
            Statement::Block(statements) => {
                for stmt in statements {
                    self.analyze_statement(stmt, analysis);
                }
            }
            _ => {}
        }
    }

    /// Calculate complexity score
    fn calculate_complexity(&self, analysis: &ShaderAnalysis) -> f32 {
        let arithmetic_weight = 1.0;
        let memory_weight = 2.0;
        let control_flow_weight = 3.0;

        ((analysis.arithmetic_ops as f32) * arithmetic_weight
            + (analysis.memory_ops as f32) * memory_weight
            + (analysis.control_flow_ops as f32) * control_flow_weight)
            / (analysis.instruction_count as f32 + 1.0)
    }
}

impl Default for ShaderAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaderAnalysis {
    /// Print analysis report
    pub fn print(&self) {
        println!("\nShader Analysis Report:");
        println!("  Instructions: {}", self.instruction_count);
        println!("  Estimated registers: {}", self.register_count);
        println!("  Arithmetic operations: {}", self.arithmetic_ops);
        println!("  Memory operations: {}", self.memory_ops);
        println!("  Control flow operations: {}", self.control_flow_ops);
        println!("  Complexity score: {:.2}", self.complexity_score);
        println!("  Entry points: {}", self.entry_points.len());

        for ep in &self.entry_points {
            println!("    - {}", ep);
        }
    }

    /// Get performance classification
    pub fn get_performance_class(&self) -> PerformanceClass {
        if self.complexity_score < 1.5 {
            PerformanceClass::Fast
        } else if self.complexity_score < 2.5 {
            PerformanceClass::Medium
        } else {
            PerformanceClass::Slow
        }
    }
}

/// Shader performance classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceClass {
    /// Fast shader (low complexity)
    Fast,
    /// Medium performance shader
    Medium,
    /// Slow shader (high complexity)
    Slow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = ShaderAnalyzer::new();
        assert!(std::mem::size_of_val(&analyzer) < 100);
    }

    #[test]
    fn test_performance_classification() {
        let analysis = ShaderAnalysis {
            instruction_count: 100,
            register_count: 10,
            arithmetic_ops: 50,
            memory_ops: 30,
            control_flow_ops: 10,
            complexity_score: 1.2,
            entry_points: vec!["main".to_string()],
            call_graph: HashMap::new(),
        };

        assert_eq!(analysis.get_performance_class(), PerformanceClass::Fast);
    }
}
