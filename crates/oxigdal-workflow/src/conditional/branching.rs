//! Conditional branching logic for workflows.

use crate::conditional::expressions::{Expression, ExpressionContext};
use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};

/// Conditional branch definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalBranch {
    /// Branch condition.
    pub condition: Expression,
    /// Tasks to execute if condition is true.
    pub then_tasks: Vec<String>,
    /// Tasks to execute if condition is false (optional).
    pub else_tasks: Option<Vec<String>>,
}

/// Switch-case conditional structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchCase {
    /// Variable to switch on.
    pub variable: String,
    /// Cases.
    pub cases: Vec<Case>,
    /// Default case (executed if no case matches).
    pub default: Option<Vec<String>>,
}

/// A single case in a switch statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    /// Value to match.
    pub value: serde_json::Value,
    /// Tasks to execute if value matches.
    pub tasks: Vec<String>,
}

/// Loop conditional structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopCondition {
    /// Loop condition.
    pub condition: Expression,
    /// Tasks to execute in loop body.
    pub body_tasks: Vec<String>,
    /// Maximum iterations (safety limit).
    pub max_iterations: usize,
}

impl ConditionalBranch {
    /// Create a new conditional branch.
    pub fn new(condition: Expression, then_tasks: Vec<String>) -> Self {
        Self {
            condition,
            then_tasks,
            else_tasks: None,
        }
    }

    /// Set the else branch.
    pub fn with_else(mut self, else_tasks: Vec<String>) -> Self {
        self.else_tasks = Some(else_tasks);
        self
    }

    /// Evaluate the branch and return the tasks to execute.
    pub fn evaluate(&self, context: &ExpressionContext) -> Result<Vec<String>> {
        let condition_result = self.condition.evaluate(context)?;

        let is_true = condition_result
            .as_bool()
            .ok_or_else(|| WorkflowError::conditional("Condition must evaluate to boolean"))?;

        if is_true {
            Ok(self.then_tasks.clone())
        } else if let Some(ref else_tasks) = self.else_tasks {
            Ok(else_tasks.clone())
        } else {
            Ok(Vec::new())
        }
    }
}

impl SwitchCase {
    /// Create a new switch-case structure.
    pub fn new(variable: String, cases: Vec<Case>) -> Self {
        Self {
            variable,
            cases,
            default: None,
        }
    }

    /// Set the default case.
    pub fn with_default(mut self, default_tasks: Vec<String>) -> Self {
        self.default = Some(default_tasks);
        self
    }

    /// Evaluate the switch and return the tasks to execute.
    pub fn evaluate(&self, context: &ExpressionContext) -> Result<Vec<String>> {
        let value = context.get(&self.variable).ok_or_else(|| {
            WorkflowError::conditional(format!("Variable '{}' not found", self.variable))
        })?;

        for case in &self.cases {
            if &case.value == value {
                return Ok(case.tasks.clone());
            }
        }

        // No case matched, use default
        Ok(self.default.clone().unwrap_or_default())
    }
}

impl LoopCondition {
    /// Create a new loop condition.
    pub fn new(condition: Expression, body_tasks: Vec<String>, max_iterations: usize) -> Self {
        Self {
            condition,
            body_tasks,
            max_iterations,
        }
    }

    /// Evaluate the loop condition.
    pub fn should_continue(&self, context: &ExpressionContext) -> Result<bool> {
        let condition_result = self.condition.evaluate(context)?;

        condition_result
            .as_bool()
            .ok_or_else(|| WorkflowError::conditional("Loop condition must evaluate to boolean"))
    }

    /// Get the tasks to execute in this iteration.
    pub fn get_body_tasks(&self) -> Vec<String> {
        self.body_tasks.clone()
    }
}

/// Conditional execution decision.
#[derive(Debug, Clone)]
pub enum ExecutionDecision {
    /// Execute these tasks.
    Execute(Vec<String>),
    /// Skip execution.
    Skip,
    /// Repeat execution with these tasks.
    Repeat(Vec<String>),
}

/// Conditional execution evaluator.
pub struct ConditionalEvaluator {
    /// Registered branches.
    branches: Vec<ConditionalBranch>,
    /// Registered switches.
    switches: Vec<SwitchCase>,
    /// Registered loops.
    loops: Vec<LoopCondition>,
}

impl ConditionalEvaluator {
    /// Create a new conditional evaluator.
    pub fn new() -> Self {
        Self {
            branches: Vec::new(),
            switches: Vec::new(),
            loops: Vec::new(),
        }
    }

    /// Add a conditional branch.
    pub fn add_branch(&mut self, branch: ConditionalBranch) {
        self.branches.push(branch);
    }

    /// Add a switch-case.
    pub fn add_switch(&mut self, switch: SwitchCase) {
        self.switches.push(switch);
    }

    /// Add a loop.
    pub fn add_loop(&mut self, loop_cond: LoopCondition) {
        self.loops.push(loop_cond);
    }

    /// Evaluate all conditionals and determine which tasks to execute.
    pub fn evaluate(&self, context: &ExpressionContext) -> Result<Vec<String>> {
        let mut tasks_to_execute = Vec::new();

        // Evaluate branches
        for branch in &self.branches {
            tasks_to_execute.extend(branch.evaluate(context)?);
        }

        // Evaluate switches
        for switch in &self.switches {
            tasks_to_execute.extend(switch.evaluate(context)?);
        }

        Ok(tasks_to_execute)
    }
}

impl Default for ConditionalEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conditional::expressions::Expression;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_conditional_branch() {
        let branch = ConditionalBranch::new(
            Expression::eq(
                Expression::variable("status"),
                Expression::literal(Value::String("success".to_string())),
            ),
            vec!["task1".to_string(), "task2".to_string()],
        )
        .with_else(vec!["task3".to_string()]);

        let mut ctx = HashMap::new();
        ctx.insert("status".to_string(), Value::String("success".to_string()));

        let tasks = branch.evaluate(&ctx).expect("Failed to evaluate");
        assert_eq!(tasks, vec!["task1".to_string(), "task2".to_string()]);
    }

    #[test]
    fn test_switch_case() {
        let switch = SwitchCase::new(
            "env".to_string(),
            vec![
                Case {
                    value: Value::String("dev".to_string()),
                    tasks: vec!["dev_task".to_string()],
                },
                Case {
                    value: Value::String("prod".to_string()),
                    tasks: vec!["prod_task".to_string()],
                },
            ],
        )
        .with_default(vec!["default_task".to_string()]);

        let mut ctx = HashMap::new();
        ctx.insert("env".to_string(), Value::String("prod".to_string()));

        let tasks = switch.evaluate(&ctx).expect("Failed to evaluate");
        assert_eq!(tasks, vec!["prod_task".to_string()]);
    }

    #[test]
    fn test_loop_condition() {
        let loop_cond = LoopCondition::new(
            Expression::binary(
                Expression::variable("count"),
                crate::conditional::expressions::BinaryOperator::Lt,
                Expression::literal(Value::Number(5.into())),
            ),
            vec!["increment".to_string()],
            10,
        );

        let mut ctx = HashMap::new();
        ctx.insert("count".to_string(), Value::Number(3.into()));

        let should_continue = loop_cond.should_continue(&ctx).expect("Failed to evaluate");
        assert!(should_continue);
    }
}
