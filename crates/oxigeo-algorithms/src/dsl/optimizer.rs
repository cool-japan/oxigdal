//! Expression optimizer for the DSL
//!
//! This module provides various optimization passes:
//! - Constant folding
//! - Common subexpression elimination
//! - Dead code elimination
//! - Algebraic simplifications

use super::ast::{BinaryOp, Expr, Program, Statement, UnaryOp};

#[cfg(not(feature = "std"))]
use alloc::{
    boxed::Box,
    collections::{BTreeMap as HashMap, BTreeSet as HashSet},
    format,
    string::{String, ToString},
    vec::Vec,
};

#[cfg(feature = "std")]
use std::collections::{HashMap, HashSet};

/// Optimization level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimization
    None,
    /// Basic optimizations (constant folding)
    Basic,
    /// Standard optimizations (basic + algebraic simplifications)
    Standard,
    /// Aggressive optimizations (standard + CSE + DCE)
    Aggressive,
}

/// Optimizer for DSL programs
pub struct Optimizer {
    level: OptLevel,
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new(OptLevel::Standard)
    }
}

impl Optimizer {
    /// Creates a new optimizer with the given optimization level
    pub fn new(level: OptLevel) -> Self {
        Self { level }
    }

    /// Optimizes a program
    pub fn optimize_program(&self, mut program: Program) -> Program {
        if self.level == OptLevel::None {
            return program;
        }

        program.statements = program
            .statements
            .into_iter()
            .map(|stmt| self.optimize_statement(stmt))
            .collect();

        // Dead code elimination: drop `let`/`fn` declarations that are never
        // referenced (transitively) from an observable statement.
        if self.level == OptLevel::Aggressive {
            program = self.eliminate_dead_code(program);
        }

        program
    }

    /// Optimizes a single statement
    pub fn optimize_statement(&self, stmt: Statement) -> Statement {
        match stmt {
            Statement::VariableDecl { name, value } => Statement::VariableDecl {
                name,
                value: Box::new(self.optimize_expr(*value)),
            },
            Statement::FunctionDecl { name, params, body } => Statement::FunctionDecl {
                name,
                params,
                body: Box::new(self.optimize_expr(*body)),
            },
            Statement::Return(expr) => Statement::Return(Box::new(self.optimize_expr(*expr))),
            Statement::Expr(expr) => Statement::Expr(Box::new(self.optimize_expr(*expr))),
        }
    }

    /// Optimizes an expression
    pub fn optimize_expr(&self, expr: Expr) -> Expr {
        if self.level == OptLevel::None {
            return expr;
        }

        let mut optimized = expr;

        // Apply constant folding
        optimized = self.constant_fold(optimized);

        // Apply algebraic simplifications
        if matches!(self.level, OptLevel::Standard | OptLevel::Aggressive) {
            optimized = self.algebraic_simplify(optimized);
        }

        // Apply common subexpression elimination
        if self.level == OptLevel::Aggressive {
            optimized = self.eliminate_common_subexpressions(optimized);
        }

        optimized
    }

    /// Performs constant folding
    fn constant_fold(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Binary {
                left,
                op,
                right,
                ty,
            } => {
                let left_opt = self.constant_fold(*left);
                let right_opt = self.constant_fold(*right);

                if let (Expr::Number(l), Expr::Number(r)) = (&left_opt, &right_opt) {
                    if let Some(result) = self.eval_const_binary(*l, op, *r) {
                        return Expr::Number(result);
                    }
                }

                Expr::Binary {
                    left: Box::new(left_opt),
                    op,
                    right: Box::new(right_opt),
                    ty,
                }
            }
            Expr::Unary {
                op,
                expr: inner,
                ty,
            } => {
                let inner_opt = self.constant_fold(*inner);

                if let Expr::Number(n) = &inner_opt {
                    if let Some(result) = self.eval_const_unary(op, *n) {
                        return Expr::Number(result);
                    }
                }

                Expr::Unary {
                    op,
                    expr: Box::new(inner_opt),
                    ty,
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ty,
            } => {
                let cond_opt = self.constant_fold(*condition);

                // If condition is constant, return only the taken branch
                if let Expr::Number(n) = &cond_opt {
                    if n.abs() > f64::EPSILON {
                        return self.constant_fold(*then_expr);
                    } else {
                        return self.constant_fold(*else_expr);
                    }
                }

                Expr::Conditional {
                    condition: Box::new(cond_opt),
                    then_expr: Box::new(self.constant_fold(*then_expr)),
                    else_expr: Box::new(self.constant_fold(*else_expr)),
                    ty,
                }
            }
            Expr::Call { name, args, ty } => Expr::Call {
                name,
                args: args
                    .into_iter()
                    .map(|arg| self.constant_fold(arg))
                    .collect(),
                ty,
            },
            Expr::Block {
                statements,
                result,
                ty,
            } => Expr::Block {
                statements: statements
                    .into_iter()
                    .map(|stmt| self.optimize_statement(stmt))
                    .collect(),
                result: result.map(|r| Box::new(self.constant_fold(*r))),
                ty,
            },
            _ => expr,
        }
    }

    /// Evaluates a constant binary operation
    fn eval_const_binary(&self, left: f64, op: BinaryOp, right: f64) -> Option<f64> {
        let result = match op {
            BinaryOp::Add => left + right,
            BinaryOp::Subtract => left - right,
            BinaryOp::Multiply => left * right,
            BinaryOp::Divide => {
                if right.abs() < f64::EPSILON {
                    return None;
                }
                left / right
            }
            BinaryOp::Modulo => left % right,
            BinaryOp::Power => left.powf(right),
            BinaryOp::Equal => {
                if (left - right).abs() < f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::NotEqual => {
                if (left - right).abs() >= f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Less => {
                if left < right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::LessEqual => {
                if left <= right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Greater => {
                if left > right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::GreaterEqual => {
                if left >= right {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::And => {
                if left != 0.0 && right != 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            BinaryOp::Or => {
                if left != 0.0 || right != 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
        };

        Some(result)
    }

    /// Evaluates a constant unary operation
    fn eval_const_unary(&self, op: UnaryOp, operand: f64) -> Option<f64> {
        let result = match op {
            UnaryOp::Negate => -operand,
            UnaryOp::Plus => operand,
            UnaryOp::Not => {
                if operand.abs() < f64::EPSILON {
                    1.0
                } else {
                    0.0
                }
            }
        };

        Some(result)
    }

    /// Performs algebraic simplifications
    fn algebraic_simplify(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Binary {
                left,
                op,
                right,
                ty,
            } => {
                let left_opt = self.algebraic_simplify(*left);
                let right_opt = self.algebraic_simplify(*right);

                // x + 0 = x
                if op == BinaryOp::Add {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                    if let Expr::Number(n) = &left_opt {
                        if n.abs() < f64::EPSILON {
                            return right_opt;
                        }
                    }
                }

                // x - 0 = x
                if op == BinaryOp::Subtract {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                }

                // NOTE: `x * 0 = 0` and `0 * x = 0` are intentionally NOT simplified here.
                // Per IEEE-754, `NaN * 0.0 == NaN` and `Inf * 0.0 == NaN`, not `0.0`. This
                // crate's raster-algebra evaluator uses NaN as a NoData sentinel (e.g. from
                // division by near-zero), so folding `x * 0` to a constant `0.0` would
                // silently turn per-pixel NoData into a false constant zero. See the
                // `test_algebraic_simplify_mul_zero_preserves_nan_semantics` regression test.

                // x * 1 = x
                if op == BinaryOp::Multiply {
                    if let Expr::Number(n) = &right_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                    if let Expr::Number(n) = &left_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return right_opt;
                        }
                    }
                }

                // x / 1 = x
                if op == BinaryOp::Divide {
                    if let Expr::Number(n) = &right_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                }

                // x ^ 0 = 1
                if op == BinaryOp::Power {
                    if let Expr::Number(n) = &right_opt {
                        if n.abs() < f64::EPSILON {
                            return Expr::Number(1.0);
                        }
                    }
                }

                // x ^ 1 = x
                if op == BinaryOp::Power {
                    if let Expr::Number(n) = &right_opt {
                        if (n - 1.0).abs() < f64::EPSILON {
                            return left_opt;
                        }
                    }
                }

                Expr::Binary {
                    left: Box::new(left_opt),
                    op,
                    right: Box::new(right_opt),
                    ty,
                }
            }
            Expr::Unary {
                op,
                expr: inner,
                ty,
            } => {
                let inner_opt = self.algebraic_simplify(*inner);

                // --x = x
                if op == UnaryOp::Negate {
                    if let Expr::Unary {
                        op: UnaryOp::Negate,
                        expr: double_neg,
                        ..
                    } = &inner_opt
                    {
                        return *double_neg.clone();
                    }
                }

                // +x = x
                if op == UnaryOp::Plus {
                    return inner_opt;
                }

                Expr::Unary {
                    op,
                    expr: Box::new(inner_opt),
                    ty,
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ty,
            } => Expr::Conditional {
                condition: Box::new(self.algebraic_simplify(*condition)),
                then_expr: Box::new(self.algebraic_simplify(*then_expr)),
                else_expr: Box::new(self.algebraic_simplify(*else_expr)),
                ty,
            },
            Expr::Call { name, args, ty } => Expr::Call {
                name,
                args: args
                    .into_iter()
                    .map(|arg| self.algebraic_simplify(arg))
                    .collect(),
                ty,
            },
            Expr::Block {
                statements,
                result,
                ty,
            } => Expr::Block {
                statements: statements
                    .into_iter()
                    .map(|stmt| self.optimize_statement(stmt))
                    .collect(),
                result: result.map(|r| Box::new(self.algebraic_simplify(*r))),
                ty,
            },
            _ => expr,
        }
    }

    /// Eliminates common subexpressions.
    ///
    /// Repeated, side-effect-free subexpressions (those built solely from band
    /// references and numeric literals, so they carry no free variables) are
    /// hoisted into `let` bindings inside a wrapping [`Expr::Block`], and every
    /// occurrence is rewritten to reference the bound variable. This turns
    /// `(B1 - B2) / (B1 + B2)`-style algebra where the same subtree recurs into a
    /// single evaluation of each shared subtree.
    ///
    /// Only "hoistable" subexpressions are considered (no `Variable`, `Block` or
    /// `ForLoop` nodes anywhere inside), which guarantees the hoisted binding is
    /// valid in the outermost scope and cannot capture an inner-scope variable.
    fn eliminate_common_subexpressions(&self, expr: Expr) -> Expr {
        // 1. Count every hoistable, non-trivial subexpression by structural key.
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut reprs: HashMap<String, Expr> = HashMap::new();
        Self::collect_cse_candidates(&expr, &mut counts, &mut reprs);

        // 2. Keep only subexpressions that occur at least twice.
        let mut candidates: Vec<(String, Expr)> = counts
            .iter()
            .filter(|&(_, &c)| c >= 2)
            .filter_map(|(k, _)| reprs.get(k).map(|e| (k.clone(), e.clone())))
            .collect();

        if candidates.is_empty() {
            return expr;
        }

        // 3. Bind smaller (inner) subexpressions first so that a larger binding's
        //    definition can reference the variables of its nested subexpressions.
        //    Deterministic ordering via (size, structural key).
        candidates.sort_by(|(ka, ea), (kb, eb)| {
            Self::expr_size(ea)
                .cmp(&Self::expr_size(eb))
                .then_with(|| ka.cmp(kb))
        });

        // 4. Assign a fresh, collision-free binding name to each candidate key.
        let mut name_map: HashMap<String, String> = HashMap::new();
        for (i, (key, _)) in candidates.iter().enumerate() {
            name_map.insert(key.clone(), format!("__cse_{i}"));
        }

        // 5. Emit `let` bindings in dependency order. A binding's definition
        //    rewrites its *children* (never the node itself, which would produce
        //    `let x = x;`) so nested common subexpressions reference earlier binds.
        let mut statements: Vec<Statement> = Vec::with_capacity(candidates.len());
        for (key, repr) in &candidates {
            // `key` is guaranteed present in `name_map` (inserted above).
            let name = name_map.get(key).cloned().unwrap_or_default();
            let definition = Self::rewrite_children_with_cse(repr, &name_map);
            statements.push(Statement::VariableDecl {
                name,
                value: Box::new(definition),
            });
        }

        // 6. Rewrite the body: every candidate occurrence becomes its variable.
        let ty = expr.get_type();
        let body = Self::rewrite_with_cse(&expr, &name_map);

        Expr::Block {
            statements,
            result: Some(Box::new(body)),
            ty,
        }
    }

    /// Recursively collects hoistable, non-trivial subexpressions with their
    /// occurrence counts (keyed by structural fingerprint) and a representative.
    fn collect_cse_candidates(
        expr: &Expr,
        counts: &mut HashMap<String, usize>,
        reprs: &mut HashMap<String, Expr>,
    ) {
        if Self::is_cse_target(expr) {
            let key = Self::cse_key(expr);
            *counts.entry(key.clone()).or_insert(0) += 1;
            reprs.entry(key).or_insert_with(|| expr.clone());
        }

        match expr {
            Expr::Binary { left, right, .. } => {
                Self::collect_cse_candidates(left, counts, reprs);
                Self::collect_cse_candidates(right, counts, reprs);
            }
            Expr::Unary { expr: inner, .. } => {
                Self::collect_cse_candidates(inner, counts, reprs);
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    Self::collect_cse_candidates(arg, counts, reprs);
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                Self::collect_cse_candidates(condition, counts, reprs);
                Self::collect_cse_candidates(then_expr, counts, reprs);
                Self::collect_cse_candidates(else_expr, counts, reprs);
            }
            Expr::Block {
                statements, result, ..
            } => {
                for stmt in statements {
                    Self::collect_cse_candidates_stmt(stmt, counts, reprs);
                }
                if let Some(result) = result {
                    Self::collect_cse_candidates(result, counts, reprs);
                }
            }
            Expr::ForLoop {
                start, end, body, ..
            } => {
                Self::collect_cse_candidates(start, counts, reprs);
                Self::collect_cse_candidates(end, counts, reprs);
                Self::collect_cse_candidates(body, counts, reprs);
            }
            Expr::Number(_) | Expr::Band(_) | Expr::Variable(_) => {}
        }
    }

    fn collect_cse_candidates_stmt(
        stmt: &Statement,
        counts: &mut HashMap<String, usize>,
        reprs: &mut HashMap<String, Expr>,
    ) {
        match stmt {
            Statement::VariableDecl { value, .. } => {
                Self::collect_cse_candidates(value, counts, reprs);
            }
            Statement::FunctionDecl { body, .. } => {
                Self::collect_cse_candidates(body, counts, reprs);
            }
            Statement::Return(expr) | Statement::Expr(expr) => {
                Self::collect_cse_candidates(expr, counts, reprs);
            }
        }
    }

    /// A subexpression is a CSE target if it is non-trivial (worth caching) and
    /// hoistable (references only bands/literals, never a free variable, block,
    /// or loop, so it is valid to evaluate once in the outermost scope).
    fn is_cse_target(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Binary { .. } | Expr::Unary { .. } | Expr::Call { .. } | Expr::Conditional { .. }
        ) && Self::is_hoistable(expr)
    }

    /// Whether the entire subtree can be safely evaluated in the outermost scope.
    fn is_hoistable(expr: &Expr) -> bool {
        match expr {
            Expr::Number(_) | Expr::Band(_) => true,
            Expr::Variable(_) | Expr::Block { .. } | Expr::ForLoop { .. } => false,
            Expr::Binary { left, right, .. } => {
                Self::is_hoistable(left) && Self::is_hoistable(right)
            }
            Expr::Unary { expr: inner, .. } => Self::is_hoistable(inner),
            Expr::Call { args, .. } => args.iter().all(Self::is_hoistable),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                Self::is_hoistable(condition)
                    && Self::is_hoistable(then_expr)
                    && Self::is_hoistable(else_expr)
            }
        }
    }

    /// Structural fingerprint used to identify equal subexpressions.
    fn cse_key(expr: &Expr) -> String {
        format!("{expr:?}")
    }

    /// Node count of a subtree (used to order bindings inner-first).
    fn expr_size(expr: &Expr) -> usize {
        1 + match expr {
            Expr::Number(_) | Expr::Band(_) | Expr::Variable(_) => 0,
            Expr::Binary { left, right, .. } => Self::expr_size(left) + Self::expr_size(right),
            Expr::Unary { expr: inner, .. } => Self::expr_size(inner),
            Expr::Call { args, .. } => args.iter().map(Self::expr_size).sum(),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                Self::expr_size(condition) + Self::expr_size(then_expr) + Self::expr_size(else_expr)
            }
            Expr::Block {
                statements, result, ..
            } => {
                statements.iter().map(Self::stmt_size).sum::<usize>()
                    + result.as_ref().map_or(0, |r| Self::expr_size(r))
            }
            Expr::ForLoop {
                start, end, body, ..
            } => Self::expr_size(start) + Self::expr_size(end) + Self::expr_size(body),
        }
    }

    fn stmt_size(stmt: &Statement) -> usize {
        match stmt {
            Statement::VariableDecl { value, .. } => Self::expr_size(value),
            Statement::FunctionDecl { body, .. } => Self::expr_size(body),
            Statement::Return(expr) | Statement::Expr(expr) => Self::expr_size(expr),
        }
    }

    /// Rewrites an expression, replacing any subexpression whose structural key is
    /// bound in `name_map` with a reference to its cached variable.
    fn rewrite_with_cse(expr: &Expr, name_map: &HashMap<String, String>) -> Expr {
        let key = Self::cse_key(expr);
        if let Some(name) = name_map.get(&key) {
            return Expr::Variable(name.clone());
        }
        Self::rewrite_children_with_cse(expr, name_map)
    }

    /// Like [`Self::rewrite_with_cse`] but never replaces the node itself, only
    /// its descendants (used when building a binding's own definition).
    fn rewrite_children_with_cse(expr: &Expr, name_map: &HashMap<String, String>) -> Expr {
        match expr {
            Expr::Binary {
                left,
                op,
                right,
                ty,
            } => Expr::Binary {
                left: Box::new(Self::rewrite_with_cse(left, name_map)),
                op: *op,
                right: Box::new(Self::rewrite_with_cse(right, name_map)),
                ty: *ty,
            },
            Expr::Unary {
                op,
                expr: inner,
                ty,
            } => Expr::Unary {
                op: *op,
                expr: Box::new(Self::rewrite_with_cse(inner, name_map)),
                ty: *ty,
            },
            Expr::Call { name, args, ty } => Expr::Call {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|a| Self::rewrite_with_cse(a, name_map))
                    .collect(),
                ty: *ty,
            },
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ty,
            } => Expr::Conditional {
                condition: Box::new(Self::rewrite_with_cse(condition, name_map)),
                then_expr: Box::new(Self::rewrite_with_cse(then_expr, name_map)),
                else_expr: Box::new(Self::rewrite_with_cse(else_expr, name_map)),
                ty: *ty,
            },
            // Leaves and non-hoistable containers are returned unchanged. (CSE
            // targets never live inside Block/ForLoop bindings emitted here, so we
            // do not descend into them; the outer body rewrite handles those.)
            other => other.clone(),
        }
    }

    /// Removes dead `let`/`fn` declarations: a declaration is retained only if its
    /// bound name is referenced, directly or transitively, from an observable
    /// statement (a `return` or a bare expression statement).
    fn eliminate_dead_code(&self, mut program: Program) -> Program {
        // Names referenced by each declaration's definition.
        let mut decl_refs: HashMap<String, Vec<String>> = HashMap::new();
        for stmt in &program.statements {
            match stmt {
                Statement::VariableDecl { name, value } => {
                    let refs = Self::referenced_names(value);
                    decl_refs.entry(name.clone()).or_default().extend(refs);
                }
                Statement::FunctionDecl { name, params, body } => {
                    let mut refs = Self::referenced_names(body);
                    // Parameters are locally bound, not free references.
                    refs.retain(|n| !params.contains(n));
                    decl_refs.entry(name.clone()).or_default().extend(refs);
                }
                _ => {}
            }
        }

        // Seed the live set from observable statements.
        let mut live: HashSet<String> = HashSet::new();
        let mut worklist: Vec<String> = Vec::new();
        for stmt in &program.statements {
            if let Statement::Return(expr) | Statement::Expr(expr) = stmt {
                for name in Self::referenced_names(expr) {
                    if live.insert(name.clone()) {
                        worklist.push(name);
                    }
                }
            }
        }

        // Transitive closure over declaration references.
        while let Some(name) = worklist.pop() {
            if let Some(refs) = decl_refs.get(&name).cloned() {
                for r in refs {
                    if live.insert(r.clone()) {
                        worklist.push(r);
                    }
                }
            }
        }

        // Keep observable statements; keep declarations whose name is live.
        program.statements.retain(|stmt| match stmt {
            Statement::VariableDecl { name, .. } | Statement::FunctionDecl { name, .. } => {
                live.contains(name)
            }
            Statement::Return(_) | Statement::Expr(_) => true,
        });

        program
    }

    /// Collects the free variable and function-call names referenced by an expr.
    fn referenced_names(expr: &Expr) -> Vec<String> {
        let mut out = Vec::new();
        Self::collect_referenced_names(expr, &mut out);
        out
    }

    fn collect_referenced_names(expr: &Expr, out: &mut Vec<String>) {
        match expr {
            Expr::Variable(name) => out.push(name.clone()),
            Expr::Number(_) | Expr::Band(_) => {}
            Expr::Binary { left, right, .. } => {
                Self::collect_referenced_names(left, out);
                Self::collect_referenced_names(right, out);
            }
            Expr::Unary { expr: inner, .. } => Self::collect_referenced_names(inner, out),
            Expr::Call { name, args, .. } => {
                out.push(name.clone());
                for arg in args {
                    Self::collect_referenced_names(arg, out);
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                Self::collect_referenced_names(condition, out);
                Self::collect_referenced_names(then_expr, out);
                Self::collect_referenced_names(else_expr, out);
            }
            Expr::Block {
                statements, result, ..
            } => {
                for stmt in statements {
                    Self::collect_referenced_names_stmt(stmt, out);
                }
                if let Some(result) = result {
                    Self::collect_referenced_names(result, out);
                }
            }
            Expr::ForLoop {
                start, end, body, ..
            } => {
                Self::collect_referenced_names(start, out);
                Self::collect_referenced_names(end, out);
                Self::collect_referenced_names(body, out);
            }
        }
    }

    fn collect_referenced_names_stmt(stmt: &Statement, out: &mut Vec<String>) {
        match stmt {
            Statement::VariableDecl { value, .. } => Self::collect_referenced_names(value, out),
            Statement::FunctionDecl { body, .. } => Self::collect_referenced_names(body, out),
            Statement::Return(expr) | Statement::Expr(expr) => {
                Self::collect_referenced_names(expr, out)
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::dsl::Type;

    #[test]
    fn test_constant_fold_add() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Number(2.0)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(3.0)),
            ty: Type::Number,
        };

        let opt = Optimizer::new(OptLevel::Basic);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Number(n) if (n - 5.0).abs() < 1e-10));
    }

    #[test]
    fn test_constant_fold_nested() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Binary {
                left: Box::new(Expr::Number(2.0)),
                op: BinaryOp::Multiply,
                right: Box::new(Expr::Number(3.0)),
                ty: Type::Number,
            }),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(4.0)),
            ty: Type::Number,
        };

        let opt = Optimizer::new(OptLevel::Basic);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Number(n) if (n - 10.0).abs() < 1e-10));
    }

    #[test]
    fn test_algebraic_simplify_add_zero() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Band(1)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(0.0)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    #[test]
    fn test_algebraic_simplify_mul_one() {
        let expr = Expr::Binary {
            left: Box::new(Expr::Band(1)),
            op: BinaryOp::Multiply,
            right: Box::new(Expr::Number(1.0)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    #[test]
    fn test_algebraic_simplify_mul_zero_not_folded() {
        // `x * 0` and `0 * x` must NOT be simplified to a constant `Number(0.0)`.
        // Per IEEE-754, NaN * 0.0 == NaN and Inf * 0.0 == NaN (not 0.0), and this
        // crate's raster-algebra evaluator relies on NaN as a NoData sentinel.
        // Folding this to a constant would silently turn per-pixel NoData into 0.0.
        let expr = Expr::Binary {
            left: Box::new(Expr::Band(1)),
            op: BinaryOp::Multiply,
            right: Box::new(Expr::Number(0.0)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        // Must remain a Binary(Band(1) * 0), not collapse to Number(0.0).
        assert!(matches!(
            result,
            Expr::Binary {
                op: BinaryOp::Multiply,
                ..
            }
        ));
        assert!(!matches!(result, Expr::Number(_)));

        // Same for the commuted form `0 * x`.
        let expr_commuted = Expr::Binary {
            left: Box::new(Expr::Number(0.0)),
            op: BinaryOp::Multiply,
            right: Box::new(Expr::Band(1)),
            ty: Type::Raster,
        };
        let result_commuted = opt.optimize_expr(expr_commuted);
        assert!(!matches!(result_commuted, Expr::Number(_)));
    }

    #[test]
    fn test_algebraic_simplify_mul_zero_preserves_nan_semantics() {
        // Regression test: `(B1 / B2) * 0` must NOT silently turn per-pixel NoData
        // (NaN, produced by division-by-near-zero) into a false constant `0.0`.
        //
        // The evaluator treats NaN as the NoData sentinel; the algebraic simplifier
        // must not fold `x * 0` to `0` since `NaN * 0.0 == NaN` (and `Inf * 0.0 ==
        // NaN`) under IEEE-754, not `0.0`.
        use crate::dsl::RasterDsl;
        use oxigeo_core::buffer::RasterBuffer;
        use oxigeo_core::types::RasterDataType;

        let mut b1 = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        // B1 = 1.0, B2 = 0.0 => B1 / B2 evaluates to NaN (NoData) in this evaluator.
        assert!(b1.set_pixel(0, 0, 1.0).is_ok());
        assert!(b2.set_pixel(0, 0, 0.0).is_ok());

        let dsl = RasterDsl::new();
        let result = dsl
            .execute("(B1 / B2) * 0", &[b1, b2])
            .expect("DSL expression should execute successfully");

        let pixel = result
            .get_pixel(0, 0)
            .expect("pixel (0, 0) should be readable");

        assert!(
            pixel.is_nan(),
            "expected NoData (NaN) to survive `* 0`, got {pixel} instead"
        );
    }

    #[test]
    fn test_double_negation() {
        let expr = Expr::Unary {
            op: UnaryOp::Negate,
            expr: Box::new(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(Expr::Band(1)),
                ty: Type::Raster,
            }),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    #[test]
    fn test_unary_plus() {
        let expr = Expr::Unary {
            op: UnaryOp::Plus,
            expr: Box::new(Expr::Band(1)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_expr(expr);

        assert!(matches!(result, Expr::Band(1)));
    }

    /// Builds `(B1 + B2)` with a given band pair.
    fn add_bands(a: usize, b: usize) -> Expr {
        Expr::Binary {
            left: Box::new(Expr::Band(a)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Band(b)),
            ty: Type::Raster,
        }
    }

    #[test]
    fn test_cse_hoists_repeated_subexpression() {
        // (B1 + B2) * (B1 + B2)  =>  { let __cse_0 = B1 + B2; __cse_0 * __cse_0 }
        let expr = Expr::Binary {
            left: Box::new(add_bands(1, 2)),
            op: BinaryOp::Multiply,
            right: Box::new(add_bands(1, 2)),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Aggressive);
        let result = opt.optimize_expr(expr);

        // Result must be a Block with exactly one hoisted binding.
        let Expr::Block {
            statements, result, ..
        } = result
        else {
            panic!("expected CSE to produce a Block, got {result:?}");
        };
        assert_eq!(statements.len(), 1, "one common subexpression expected");
        let Statement::VariableDecl { name, value } = &statements[0] else {
            panic!("expected a VariableDecl binding");
        };
        // The binding must be the shared `B1 + B2` subtree.
        assert!(matches!(
            **value,
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));
        // The body must reference the bound variable on both sides (no B1+B2 left).
        let body = result.expect("block must have a result");
        let Expr::Binary { left, right, .. } = *body else {
            panic!("expected a Binary body");
        };
        assert!(matches!(*left, Expr::Variable(ref n) if n == name));
        assert!(matches!(*right, Expr::Variable(ref n) if n == name));
    }

    #[test]
    fn test_cse_no_hoist_when_unique() {
        // (B1 + B2) has no repeated subexpression -> unchanged (no Block wrapper).
        let expr = add_bands(1, 2);
        let opt = Optimizer::new(OptLevel::Aggressive);
        let result = opt.optimize_expr(expr);
        assert!(
            matches!(result, Expr::Binary { .. }),
            "unique expression must not be wrapped in a CSE block"
        );
    }

    #[test]
    fn test_cse_nested_common_subexpressions() {
        // ((B1 + B2) * B3) + ((B1 + B2) * B3)
        // Both the inner (B1 + B2) and the larger ((B1 + B2) * B3) repeat, so two
        // bindings are produced with the larger one referencing the smaller.
        let times_b3 = |()| Expr::Binary {
            left: Box::new(add_bands(1, 2)),
            op: BinaryOp::Multiply,
            right: Box::new(Expr::Band(3)),
            ty: Type::Raster,
        };
        let expr = Expr::Binary {
            left: Box::new(times_b3(())),
            op: BinaryOp::Add,
            right: Box::new(times_b3(())),
            ty: Type::Raster,
        };

        let opt = Optimizer::new(OptLevel::Aggressive);
        let result = opt.optimize_expr(expr);

        let Expr::Block { statements, .. } = result else {
            panic!("expected a CSE block");
        };
        assert_eq!(statements.len(), 2, "inner and outer CSE expected");
        // First binding (inner, smaller) is `B1 + B2`.
        let Statement::VariableDecl {
            name: inner_name,
            value: inner_value,
        } = &statements[0]
        else {
            panic!("expected inner binding");
        };
        assert!(matches!(
            **inner_value,
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));
        // Second binding (outer) must reference the inner binding, not re-inline
        // `B1 + B2`.
        let Statement::VariableDecl {
            value: outer_value, ..
        } = &statements[1]
        else {
            panic!("expected outer binding");
        };
        let Expr::Binary { left, .. } = &**outer_value else {
            panic!("expected outer binding to be a multiply");
        };
        assert!(
            matches!(**left, Expr::Variable(ref n) if n == inner_name),
            "outer binding must reference the inner CSE variable"
        );
    }

    #[test]
    fn test_cse_preserves_result_value() {
        use crate::dsl::RasterDsl;
        use oxigeo_core::buffer::RasterBuffer;
        use oxigeo_core::types::RasterDataType;

        let mut b1 = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        for y in 0..2 {
            for x in 0..2 {
                assert!(b1.set_pixel(x, y, 3.0).is_ok());
                assert!(b2.set_pixel(x, y, 1.0).is_ok());
            }
        }

        // (B1 - B2) / (B1 + B2) => (3-1)/(3+1) = 0.5 at every pixel, with and
        // without CSE the value must be identical.
        let expr = "(B1 - B2) / (B1 + B2)";

        let mut plain = RasterDsl::new();
        plain.set_opt_level(OptLevel::None);
        let mut aggressive = RasterDsl::new();
        aggressive.set_opt_level(OptLevel::Aggressive);

        let r_plain = plain
            .execute(expr, &[b1.clone(), b2.clone()])
            .expect("plain execution should succeed");
        let r_cse = aggressive
            .execute(expr, &[b1, b2])
            .expect("aggressive execution should succeed");

        for y in 0..2 {
            for x in 0..2 {
                let a = r_plain.get_pixel(x, y).expect("pixel readable");
                let b = r_cse.get_pixel(x, y).expect("pixel readable");
                assert!((a - b).abs() < 1e-6, "CSE changed the value: {a} vs {b}");
                assert!((a - 0.5).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn test_dce_removes_unused_declaration() {
        // let used = B1 + B2;
        // let dead = B1 * B2;   // never referenced
        // used;
        let program = Program {
            statements: vec![
                Statement::VariableDecl {
                    name: "used".to_string(),
                    value: Box::new(add_bands(1, 2)),
                },
                Statement::VariableDecl {
                    name: "dead".to_string(),
                    value: Box::new(Expr::Binary {
                        left: Box::new(Expr::Band(1)),
                        op: BinaryOp::Multiply,
                        right: Box::new(Expr::Band(2)),
                        ty: Type::Raster,
                    }),
                },
                Statement::Expr(Box::new(Expr::Variable("used".to_string()))),
            ],
        };

        let opt = Optimizer::new(OptLevel::Aggressive);
        let result = opt.optimize_program(program);

        // `dead` must be gone; `used` and the final expr must remain.
        assert_eq!(result.statements.len(), 2);
        assert!(matches!(
            &result.statements[0],
            Statement::VariableDecl { name, .. } if name == "used"
        ));
        assert!(matches!(&result.statements[1], Statement::Expr(_)));
    }

    #[test]
    fn test_dce_keeps_transitively_used_declaration() {
        // let a = B1 + B2;
        // let b = a * B3;   // b uses a
        // b;                // observable uses b -> both a and b are live
        let program = Program {
            statements: vec![
                Statement::VariableDecl {
                    name: "a".to_string(),
                    value: Box::new(add_bands(1, 2)),
                },
                Statement::VariableDecl {
                    name: "b".to_string(),
                    value: Box::new(Expr::Binary {
                        left: Box::new(Expr::Variable("a".to_string())),
                        op: BinaryOp::Multiply,
                        right: Box::new(Expr::Band(3)),
                        ty: Type::Raster,
                    }),
                },
                Statement::Expr(Box::new(Expr::Variable("b".to_string()))),
            ],
        };

        let opt = Optimizer::new(OptLevel::Aggressive);
        let result = opt.optimize_program(program);

        // Nothing dead: all three statements survive.
        assert_eq!(result.statements.len(), 3);
    }

    #[test]
    fn test_dce_not_applied_below_aggressive() {
        let program = Program {
            statements: vec![
                Statement::VariableDecl {
                    name: "dead".to_string(),
                    value: Box::new(add_bands(1, 2)),
                },
                Statement::Expr(Box::new(Expr::Band(1))),
            ],
        };

        let opt = Optimizer::new(OptLevel::Standard);
        let result = opt.optimize_program(program);

        // Standard level must not strip the (dead) declaration.
        assert_eq!(result.statements.len(), 2);
    }
}
