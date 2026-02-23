//! Abstract Syntax Tree definitions for query language.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A complete query statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    /// SELECT query.
    Select(SelectStatement),
}

/// A SELECT statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectStatement {
    /// Projection list.
    pub projection: Vec<SelectItem>,
    /// FROM clause.
    pub from: Option<TableReference>,
    /// WHERE clause.
    pub selection: Option<Expr>,
    /// GROUP BY clause.
    pub group_by: Vec<Expr>,
    /// HAVING clause.
    pub having: Option<Expr>,
    /// ORDER BY clause.
    pub order_by: Vec<OrderByExpr>,
    /// LIMIT clause.
    pub limit: Option<usize>,
    /// OFFSET clause.
    pub offset: Option<usize>,
}

/// A select item in the projection list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SelectItem {
    /// Wildcard (*).
    Wildcard,
    /// Qualified wildcard (table.*).
    QualifiedWildcard(String),
    /// Expression with optional alias.
    Expr {
        /// The expression to select.
        expr: Expr,
        /// Optional alias for the expression.
        alias: Option<String>,
    },
}

/// A table reference in FROM clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TableReference {
    /// Base table.
    Table {
        /// Table name.
        name: String,
        /// Optional alias.
        alias: Option<String>,
    },
    /// Join.
    Join {
        /// Left table.
        left: Box<TableReference>,
        /// Right table.
        right: Box<TableReference>,
        /// Join type.
        join_type: JoinType,
        /// Join condition.
        on: Option<Expr>,
    },
    /// Subquery.
    Subquery {
        /// Subquery.
        query: Box<SelectStatement>,
        /// Alias.
        alias: String,
    },
}

/// Join type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinType {
    /// Inner join.
    Inner,
    /// Left outer join.
    Left,
    /// Right outer join.
    Right,
    /// Full outer join.
    Full,
    /// Cross join.
    Cross,
}

/// An expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Column reference.
    Column {
        /// Optional table qualifier.
        table: Option<String>,
        /// Column name.
        name: String,
    },
    /// Literal value.
    Literal(Literal),
    /// Binary operation.
    BinaryOp {
        /// Left operand.
        left: Box<Expr>,
        /// Operator.
        op: BinaryOperator,
        /// Right operand.
        right: Box<Expr>,
    },
    /// Unary operation.
    UnaryOp {
        /// Operator.
        op: UnaryOperator,
        /// Operand.
        expr: Box<Expr>,
    },
    /// Function call.
    Function {
        /// Function name.
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// CASE expression.
    Case {
        /// Optional operand (for CASE x WHEN ...).
        operand: Option<Box<Expr>>,
        /// WHEN conditions and results.
        when_then: Vec<(Expr, Expr)>,
        /// ELSE result.
        else_result: Option<Box<Expr>>,
    },
    /// CAST expression.
    Cast {
        /// Expression to cast.
        expr: Box<Expr>,
        /// Target data type.
        data_type: DataType,
    },
    /// IS NULL.
    IsNull(Box<Expr>),
    /// IS NOT NULL.
    IsNotNull(Box<Expr>),
    /// IN list.
    InList {
        /// Expression.
        expr: Box<Expr>,
        /// List of values.
        list: Vec<Expr>,
        /// Negated (NOT IN).
        negated: bool,
    },
    /// BETWEEN.
    Between {
        /// Expression.
        expr: Box<Expr>,
        /// Lower bound.
        low: Box<Expr>,
        /// Upper bound.
        high: Box<Expr>,
        /// Negated (NOT BETWEEN).
        negated: bool,
    },
    /// Subquery.
    Subquery(Box<SelectStatement>),
    /// Wildcard (*).
    Wildcard,
}

/// A literal value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// Null value.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// Integer value.
    Integer(i64),
    /// Float value.
    Float(f64),
    /// String value.
    String(String),
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOperator {
    /// Addition (+).
    Plus,
    /// Subtraction (-).
    Minus,
    /// Multiplication (*).
    Multiply,
    /// Division (/).
    Divide,
    /// Modulo (%).
    Modulo,
    /// Equality (=).
    Eq,
    /// Inequality (<>).
    NotEq,
    /// Less than (<).
    Lt,
    /// Less than or equal (<=).
    LtEq,
    /// Greater than (>).
    Gt,
    /// Greater than or equal (>=).
    GtEq,
    /// Logical AND.
    And,
    /// Logical OR.
    Or,
    /// String concatenation (||).
    Concat,
    /// LIKE pattern matching.
    Like,
    /// NOT LIKE pattern matching.
    NotLike,
}

/// Unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOperator {
    /// Negation (-).
    Minus,
    /// Logical NOT.
    Not,
}

/// ORDER BY expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderByExpr {
    /// Expression to order by.
    pub expr: Expr,
    /// Ascending or descending.
    pub asc: bool,
    /// Nulls first or last.
    pub nulls_first: bool,
}

/// Data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    /// Boolean.
    Boolean,
    /// 8-bit signed integer.
    Int8,
    /// 16-bit signed integer.
    Int16,
    /// 32-bit signed integer.
    Int32,
    /// 64-bit signed integer.
    Int64,
    /// 8-bit unsigned integer.
    UInt8,
    /// 16-bit unsigned integer.
    UInt16,
    /// 32-bit unsigned integer.
    UInt32,
    /// 64-bit unsigned integer.
    UInt64,
    /// 32-bit float.
    Float32,
    /// 64-bit float.
    Float64,
    /// UTF-8 string.
    String,
    /// Binary data.
    Binary,
    /// Timestamp.
    Timestamp,
    /// Date.
    Date,
    /// Geometry.
    Geometry,
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Select(select) => write!(f, "{}", select),
        }
    }
}

impl fmt::Display for SelectStatement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SELECT ")?;
        for (i, item) in self.projection.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", item)?;
        }
        if let Some(from) = &self.from {
            write!(f, " FROM {}", from)?;
        }
        if let Some(selection) = &self.selection {
            write!(f, " WHERE {}", selection)?;
        }
        if !self.group_by.is_empty() {
            write!(f, " GROUP BY ")?;
            for (i, expr) in self.group_by.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", expr)?;
            }
        }
        if let Some(having) = &self.having {
            write!(f, " HAVING {}", having)?;
        }
        if !self.order_by.is_empty() {
            write!(f, " ORDER BY ")?;
            for (i, expr) in self.order_by.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", expr)?;
            }
        }
        if let Some(limit) = self.limit {
            write!(f, " LIMIT {}", limit)?;
        }
        if let Some(offset) = self.offset {
            write!(f, " OFFSET {}", offset)?;
        }
        Ok(())
    }
}

impl fmt::Display for SelectItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectItem::Wildcard => write!(f, "*"),
            SelectItem::QualifiedWildcard(table) => write!(f, "{}.*", table),
            SelectItem::Expr { expr, alias } => {
                write!(f, "{}", expr)?;
                if let Some(alias) = alias {
                    write!(f, " AS {}", alias)?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for TableReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableReference::Table { name, alias } => {
                write!(f, "{}", name)?;
                if let Some(alias) = alias {
                    write!(f, " AS {}", alias)?;
                }
                Ok(())
            }
            TableReference::Join {
                left,
                right,
                join_type,
                on,
            } => {
                write!(f, "{} {} JOIN {}", left, join_type, right)?;
                if let Some(on) = on {
                    write!(f, " ON {}", on)?;
                }
                Ok(())
            }
            TableReference::Subquery { query, alias } => {
                write!(f, "({}) AS {}", query, alias)
            }
        }
    }
}

impl fmt::Display for JoinType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JoinType::Inner => write!(f, "INNER"),
            JoinType::Left => write!(f, "LEFT"),
            JoinType::Right => write!(f, "RIGHT"),
            JoinType::Full => write!(f, "FULL"),
            JoinType::Cross => write!(f, "CROSS"),
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Column { table, name } => {
                if let Some(table) = table {
                    write!(f, "{}.{}", table, name)
                } else {
                    write!(f, "{}", name)
                }
            }
            Expr::Literal(lit) => write!(f, "{}", lit),
            Expr::BinaryOp { left, op, right } => {
                write!(f, "({} {} {})", left, op, right)
            }
            Expr::UnaryOp { op, expr } => write!(f, "({} {})", op, expr),
            Expr::Function { name, args } => {
                write!(f, "{}(", name)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expr::Case {
                operand,
                when_then,
                else_result,
            } => {
                write!(f, "CASE")?;
                if let Some(operand) = operand {
                    write!(f, " {}", operand)?;
                }
                for (when, then) in when_then {
                    write!(f, " WHEN {} THEN {}", when, then)?;
                }
                if let Some(else_result) = else_result {
                    write!(f, " ELSE {}", else_result)?;
                }
                write!(f, " END")
            }
            Expr::Cast { expr, data_type } => {
                write!(f, "CAST({} AS {:?})", expr, data_type)
            }
            Expr::IsNull(expr) => write!(f, "{} IS NULL", expr),
            Expr::IsNotNull(expr) => write!(f, "{} IS NOT NULL", expr),
            Expr::InList {
                expr,
                list,
                negated,
            } => {
                write!(f, "{}", expr)?;
                if *negated {
                    write!(f, " NOT")?;
                }
                write!(f, " IN (")?;
                for (i, item) in list.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => {
                write!(f, "{}", expr)?;
                if *negated {
                    write!(f, " NOT")?;
                }
                write!(f, " BETWEEN {} AND {}", low, high)
            }
            Expr::Subquery(query) => write!(f, "({})", query),
            Expr::Wildcard => write!(f, "*"),
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Null => write!(f, "NULL"),
            Literal::Boolean(b) => write!(f, "{}", b),
            Literal::Integer(i) => write!(f, "{}", i),
            Literal::Float(fl) => write!(f, "{}", fl),
            Literal::String(s) => write!(f, "'{}'", s),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOperator::Plus => write!(f, "+"),
            BinaryOperator::Minus => write!(f, "-"),
            BinaryOperator::Multiply => write!(f, "*"),
            BinaryOperator::Divide => write!(f, "/"),
            BinaryOperator::Modulo => write!(f, "%"),
            BinaryOperator::Eq => write!(f, "="),
            BinaryOperator::NotEq => write!(f, "<>"),
            BinaryOperator::Lt => write!(f, "<"),
            BinaryOperator::LtEq => write!(f, "<="),
            BinaryOperator::Gt => write!(f, ">"),
            BinaryOperator::GtEq => write!(f, ">="),
            BinaryOperator::And => write!(f, "AND"),
            BinaryOperator::Or => write!(f, "OR"),
            BinaryOperator::Concat => write!(f, "||"),
            BinaryOperator::Like => write!(f, "LIKE"),
            BinaryOperator::NotLike => write!(f, "NOT LIKE"),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOperator::Minus => write!(f, "-"),
            UnaryOperator::Not => write!(f, "NOT"),
        }
    }
}

impl fmt::Display for OrderByExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.expr)?;
        if self.asc {
            write!(f, " ASC")?;
        } else {
            write!(f, " DESC")?;
        }
        if self.nulls_first {
            write!(f, " NULLS FIRST")?;
        } else {
            write!(f, " NULLS LAST")?;
        }
        Ok(())
    }
}
