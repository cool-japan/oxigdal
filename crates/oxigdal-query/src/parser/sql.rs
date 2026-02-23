//! SQL parser implementation.

use crate::error::{QueryError, Result};
use crate::parser::ast::*;
use sqlparser::ast as sql_ast;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser as SqlParser;

/// Parse SQL string into AST.
pub fn parse_sql(sql: &str) -> Result<Statement> {
    let dialect = GenericDialect {};
    let statements = SqlParser::parse_sql(&dialect, sql)?;

    if statements.is_empty() {
        return Err(QueryError::semantic("Empty SQL statement"));
    }

    if statements.len() > 1 {
        return Err(QueryError::semantic("Multiple statements not supported"));
    }

    convert_statement(&statements[0])
}

fn convert_statement(stmt: &sql_ast::Statement) -> Result<Statement> {
    match stmt {
        sql_ast::Statement::Query(query) => {
            let select = convert_query(query)?;
            Ok(Statement::Select(select))
        }
        _ => Err(QueryError::unsupported("Only SELECT statements supported")),
    }
}

fn convert_query(query: &sql_ast::Query) -> Result<SelectStatement> {
    if let sql_ast::SetExpr::Select(select) = &*query.body {
        let mut stmt = SelectStatement {
            projection: Vec::new(),
            from: None,
            selection: None,
            group_by: Vec::new(),
            having: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        };

        // Convert projection
        for item in &select.projection {
            stmt.projection.push(convert_select_item(item)?);
        }

        // Convert FROM clause
        if !select.from.is_empty() {
            stmt.from = Some(convert_table_reference(&select.from[0])?);
        }

        // Convert WHERE clause
        if let Some(selection) = &select.selection {
            stmt.selection = Some(convert_expr(selection)?);
        }

        // Convert GROUP BY clause
        match &select.group_by {
            sql_ast::GroupByExpr::Expressions(exprs, _) => {
                for expr in exprs {
                    stmt.group_by.push(convert_expr(expr)?);
                }
            }
            sql_ast::GroupByExpr::All(_) => {
                return Err(QueryError::unsupported("GROUP BY ALL not supported"));
            }
        }

        // Convert HAVING clause
        if let Some(having) = &select.having {
            stmt.having = Some(convert_expr(having)?);
        }

        // Convert ORDER BY clause
        if let Some(order_by) = &query.order_by {
            // OrderBy in sqlparser 0.52 contains a list of OrderByExpr
            for order_expr in &order_by.exprs {
                stmt.order_by.push(convert_order_by_expr(order_expr)?);
            }
        }

        // Convert LIMIT clause
        if let Some(limit) = &query.limit {
            stmt.limit = Some(convert_limit(limit)?);
        }

        // Convert OFFSET clause
        if let Some(offset) = &query.offset {
            stmt.offset = Some(convert_offset(offset)?);
        }

        Ok(stmt)
    } else {
        Err(QueryError::unsupported(
            "Only simple SELECT queries supported",
        ))
    }
}

fn convert_select_item(item: &sql_ast::SelectItem) -> Result<SelectItem> {
    match item {
        sql_ast::SelectItem::UnnamedExpr(expr) => Ok(SelectItem::Expr {
            expr: convert_expr(expr)?,
            alias: None,
        }),
        sql_ast::SelectItem::ExprWithAlias { expr, alias } => Ok(SelectItem::Expr {
            expr: convert_expr(expr)?,
            alias: Some(alias.value.clone()),
        }),
        sql_ast::SelectItem::Wildcard(_) => Ok(SelectItem::Wildcard),
        sql_ast::SelectItem::QualifiedWildcard(obj_name, _) => {
            Ok(SelectItem::QualifiedWildcard(obj_name.to_string()))
        }
    }
}

fn convert_table_reference(table: &sql_ast::TableWithJoins) -> Result<TableReference> {
    let mut result = convert_table_factor(&table.relation)?;

    for join in &table.joins {
        let right = convert_table_factor(&join.relation)?;
        let join_type = convert_join_type(&join.join_operator)?;
        let on = match &join.join_operator {
            sql_ast::JoinOperator::Inner(constraint)
            | sql_ast::JoinOperator::LeftOuter(constraint)
            | sql_ast::JoinOperator::RightOuter(constraint)
            | sql_ast::JoinOperator::FullOuter(constraint) => convert_join_constraint(constraint)?,
            sql_ast::JoinOperator::CrossJoin => None,
            _ => return Err(QueryError::unsupported("Unsupported join type")),
        };

        result = TableReference::Join {
            left: Box::new(result),
            right: Box::new(right),
            join_type,
            on,
        };
    }

    Ok(result)
}

fn convert_table_factor(factor: &sql_ast::TableFactor) -> Result<TableReference> {
    match factor {
        sql_ast::TableFactor::Table {
            name, alias, args, ..
        } => {
            if args.is_some() {
                return Err(QueryError::unsupported("Table functions not supported"));
            }
            Ok(TableReference::Table {
                name: name.to_string(),
                alias: alias.as_ref().map(|a| a.name.value.clone()),
            })
        }
        sql_ast::TableFactor::Derived {
            subquery, alias, ..
        } => {
            let query = convert_query(subquery)?;
            let alias_name = alias
                .as_ref()
                .map(|a| a.name.value.clone())
                .ok_or_else(|| QueryError::semantic("Subquery must have an alias"))?;
            Ok(TableReference::Subquery {
                query: Box::new(query),
                alias: alias_name,
            })
        }
        _ => Err(QueryError::unsupported("Unsupported table reference")),
    }
}

fn convert_join_type(op: &sql_ast::JoinOperator) -> Result<JoinType> {
    match op {
        sql_ast::JoinOperator::Inner(_) => Ok(JoinType::Inner),
        sql_ast::JoinOperator::LeftOuter(_) => Ok(JoinType::Left),
        sql_ast::JoinOperator::RightOuter(_) => Ok(JoinType::Right),
        sql_ast::JoinOperator::FullOuter(_) => Ok(JoinType::Full),
        sql_ast::JoinOperator::CrossJoin => Ok(JoinType::Cross),
        _ => Err(QueryError::unsupported("Unsupported join type")),
    }
}

fn convert_join_constraint(constraint: &sql_ast::JoinConstraint) -> Result<Option<Expr>> {
    match constraint {
        sql_ast::JoinConstraint::On(expr) => Ok(Some(convert_expr(expr)?)),
        sql_ast::JoinConstraint::Using(_) => {
            Err(QueryError::unsupported("USING clause not supported"))
        }
        sql_ast::JoinConstraint::Natural => {
            Err(QueryError::unsupported("NATURAL join not supported"))
        }
        sql_ast::JoinConstraint::None => Ok(None),
    }
}

fn convert_expr(expr: &sql_ast::Expr) -> Result<Expr> {
    match expr {
        sql_ast::Expr::Identifier(ident) => Ok(Expr::Column {
            table: None,
            name: ident.value.clone(),
        }),
        sql_ast::Expr::CompoundIdentifier(parts) => {
            if parts.len() == 2 {
                Ok(Expr::Column {
                    table: Some(parts[0].value.clone()),
                    name: parts[1].value.clone(),
                })
            } else {
                Err(QueryError::semantic("Invalid column reference"))
            }
        }
        sql_ast::Expr::Value(value) => Ok(Expr::Literal(convert_value(value)?)),
        sql_ast::Expr::BinaryOp { left, op, right } => Ok(Expr::BinaryOp {
            left: Box::new(convert_expr(left)?),
            op: convert_binary_op(op)?,
            right: Box::new(convert_expr(right)?),
        }),
        sql_ast::Expr::UnaryOp { op, expr } => Ok(Expr::UnaryOp {
            op: convert_unary_op(op)?,
            expr: Box::new(convert_expr(expr)?),
        }),
        sql_ast::Expr::Function(func) => {
            let name = func.name.to_string();
            let mut args = Vec::new();

            // Handle FunctionArguments enum
            match &func.args {
                sql_ast::FunctionArguments::None => {
                    // No arguments
                }
                sql_ast::FunctionArguments::Subquery(_) => {
                    return Err(QueryError::unsupported(
                        "Subquery in function arguments not supported",
                    ));
                }
                sql_ast::FunctionArguments::List(arg_list) => {
                    for arg in &arg_list.args {
                        match arg {
                            sql_ast::FunctionArg::Unnamed(sql_ast::FunctionArgExpr::Expr(e)) => {
                                args.push(convert_expr(e)?);
                            }
                            sql_ast::FunctionArg::Unnamed(sql_ast::FunctionArgExpr::Wildcard) => {
                                // Handle COUNT(*) and similar
                                args.push(Expr::Wildcard);
                            }
                            sql_ast::FunctionArg::Named {
                                name: _,
                                arg: sql_ast::FunctionArgExpr::Expr(e),
                                ..
                            } => {
                                args.push(convert_expr(e)?);
                            }
                            sql_ast::FunctionArg::Named {
                                name: _,
                                arg: sql_ast::FunctionArgExpr::Wildcard,
                                ..
                            } => {
                                args.push(Expr::Wildcard);
                            }
                            _ => {
                                return Err(QueryError::unsupported(
                                    "Unsupported function argument",
                                ));
                            }
                        }
                    }
                }
            }
            Ok(Expr::Function { name, args })
        }
        sql_ast::Expr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            let operand = operand
                .as_ref()
                .map(|e| convert_expr(e))
                .transpose()?
                .map(Box::new);
            let mut when_then = Vec::new();
            for (cond, result) in conditions.iter().zip(results.iter()) {
                when_then.push((convert_expr(cond)?, convert_expr(result)?));
            }
            let else_result = else_result
                .as_ref()
                .map(|e| convert_expr(e))
                .transpose()?
                .map(Box::new);
            Ok(Expr::Case {
                operand,
                when_then,
                else_result,
            })
        }
        sql_ast::Expr::Cast {
            expr, data_type, ..
        } => Ok(Expr::Cast {
            expr: Box::new(convert_expr(expr)?),
            data_type: convert_data_type(data_type)?,
        }),
        sql_ast::Expr::IsNull(expr) => Ok(Expr::IsNull(Box::new(convert_expr(expr)?))),
        sql_ast::Expr::IsNotNull(expr) => Ok(Expr::IsNotNull(Box::new(convert_expr(expr)?))),
        sql_ast::Expr::InList {
            expr,
            list,
            negated,
        } => {
            let expr = Box::new(convert_expr(expr)?);
            let list = list.iter().map(convert_expr).collect::<Result<Vec<_>>>()?;
            Ok(Expr::InList {
                expr,
                list,
                negated: *negated,
            })
        }
        sql_ast::Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Ok(Expr::Between {
            expr: Box::new(convert_expr(expr)?),
            low: Box::new(convert_expr(low)?),
            high: Box::new(convert_expr(high)?),
            negated: *negated,
        }),
        sql_ast::Expr::Like {
            negated,
            expr,
            pattern,
            ..
        } => Ok(Expr::BinaryOp {
            left: Box::new(convert_expr(expr)?),
            op: if *negated {
                BinaryOperator::NotLike
            } else {
                BinaryOperator::Like
            },
            right: Box::new(convert_expr(pattern)?),
        }),
        sql_ast::Expr::ILike {
            negated,
            expr,
            pattern,
            ..
        } => {
            // ILIKE (case-insensitive) is treated as regular LIKE for now
            Ok(Expr::BinaryOp {
                left: Box::new(convert_expr(expr)?),
                op: if *negated {
                    BinaryOperator::NotLike
                } else {
                    BinaryOperator::Like
                },
                right: Box::new(convert_expr(pattern)?),
            })
        }
        sql_ast::Expr::Subquery(query) => Ok(Expr::Subquery(Box::new(convert_query(query)?))),
        sql_ast::Expr::Nested(expr) => convert_expr(expr),
        sql_ast::Expr::Wildcard => Ok(Expr::Wildcard),
        _ => Err(QueryError::unsupported(format!(
            "Unsupported expression: {:?}",
            expr
        ))),
    }
}

fn convert_value(value: &sql_ast::Value) -> Result<Literal> {
    match value {
        sql_ast::Value::Null => Ok(Literal::Null),
        sql_ast::Value::Boolean(b) => Ok(Literal::Boolean(*b)),
        sql_ast::Value::Number(n, _) => {
            if let Ok(i) = n.parse::<i64>() {
                Ok(Literal::Integer(i))
            } else if let Ok(f) = n.parse::<f64>() {
                Ok(Literal::Float(f))
            } else {
                Err(QueryError::parse_error("Invalid number", 0, 0))
            }
        }
        sql_ast::Value::SingleQuotedString(s) | sql_ast::Value::DoubleQuotedString(s) => {
            Ok(Literal::String(s.clone()))
        }
        _ => Err(QueryError::unsupported("Unsupported literal value")),
    }
}

fn convert_binary_op(op: &sql_ast::BinaryOperator) -> Result<BinaryOperator> {
    match op {
        sql_ast::BinaryOperator::Plus => Ok(BinaryOperator::Plus),
        sql_ast::BinaryOperator::Minus => Ok(BinaryOperator::Minus),
        sql_ast::BinaryOperator::Multiply => Ok(BinaryOperator::Multiply),
        sql_ast::BinaryOperator::Divide => Ok(BinaryOperator::Divide),
        sql_ast::BinaryOperator::Modulo => Ok(BinaryOperator::Modulo),
        sql_ast::BinaryOperator::Eq => Ok(BinaryOperator::Eq),
        sql_ast::BinaryOperator::NotEq => Ok(BinaryOperator::NotEq),
        sql_ast::BinaryOperator::Lt => Ok(BinaryOperator::Lt),
        sql_ast::BinaryOperator::LtEq => Ok(BinaryOperator::LtEq),
        sql_ast::BinaryOperator::Gt => Ok(BinaryOperator::Gt),
        sql_ast::BinaryOperator::GtEq => Ok(BinaryOperator::GtEq),
        sql_ast::BinaryOperator::And => Ok(BinaryOperator::And),
        sql_ast::BinaryOperator::Or => Ok(BinaryOperator::Or),
        sql_ast::BinaryOperator::StringConcat => Ok(BinaryOperator::Concat),
        // Note: LIKE and NOT LIKE are handled as separate expression types in sqlparser 0.52+
        _ => Err(QueryError::unsupported("Unsupported binary operator")),
    }
}

fn convert_unary_op(op: &sql_ast::UnaryOperator) -> Result<UnaryOperator> {
    match op {
        sql_ast::UnaryOperator::Minus => Ok(UnaryOperator::Minus),
        sql_ast::UnaryOperator::Not => Ok(UnaryOperator::Not),
        _ => Err(QueryError::unsupported("Unsupported unary operator")),
    }
}

fn convert_order_by_expr(order: &sql_ast::OrderByExpr) -> Result<OrderByExpr> {
    Ok(OrderByExpr {
        expr: convert_expr(&order.expr)?,
        asc: order.asc.unwrap_or(true),
        nulls_first: order.nulls_first.unwrap_or(false),
    })
}

fn convert_limit(limit: &sql_ast::Expr) -> Result<usize> {
    match limit {
        sql_ast::Expr::Value(sql_ast::Value::Number(n, _)) => n
            .parse::<usize>()
            .map_err(|_| QueryError::semantic("Invalid LIMIT value")),
        _ => Err(QueryError::semantic("LIMIT must be a number")),
    }
}

fn convert_offset(offset: &sql_ast::Offset) -> Result<usize> {
    match &offset.value {
        sql_ast::Expr::Value(sql_ast::Value::Number(n, _)) => n
            .parse::<usize>()
            .map_err(|_| QueryError::semantic("Invalid OFFSET value")),
        _ => Err(QueryError::semantic("OFFSET must be a number")),
    }
}

fn convert_data_type(data_type: &sql_ast::DataType) -> Result<DataType> {
    match data_type {
        sql_ast::DataType::Boolean => Ok(DataType::Boolean),
        sql_ast::DataType::TinyInt(_) => Ok(DataType::Int8),
        sql_ast::DataType::SmallInt(_) => Ok(DataType::Int16),
        sql_ast::DataType::Int(_) | sql_ast::DataType::Integer(_) => Ok(DataType::Int32),
        sql_ast::DataType::BigInt(_) => Ok(DataType::Int64),
        sql_ast::DataType::UnsignedTinyInt(_) => Ok(DataType::UInt8),
        sql_ast::DataType::UnsignedSmallInt(_) => Ok(DataType::UInt16),
        sql_ast::DataType::UnsignedInt(_) | sql_ast::DataType::UnsignedInteger(_) => {
            Ok(DataType::UInt32)
        }
        sql_ast::DataType::UnsignedBigInt(_) => Ok(DataType::UInt64),
        sql_ast::DataType::Float(_) | sql_ast::DataType::Real => Ok(DataType::Float32),
        sql_ast::DataType::Double | sql_ast::DataType::DoublePrecision => Ok(DataType::Float64),
        sql_ast::DataType::Varchar(_)
        | sql_ast::DataType::Char(_)
        | sql_ast::DataType::Text
        | sql_ast::DataType::String(_) => Ok(DataType::String),
        sql_ast::DataType::Binary(_) | sql_ast::DataType::Varbinary(_) => Ok(DataType::Binary),
        sql_ast::DataType::Timestamp(_, _) => Ok(DataType::Timestamp),
        sql_ast::DataType::Date => Ok(DataType::Date),
        sql_ast::DataType::Custom(name, _) if name.to_string().to_uppercase() == "GEOMETRY" => {
            Ok(DataType::Geometry)
        }
        _ => Err(QueryError::unsupported(format!(
            "Unsupported data type: {:?}",
            data_type
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_select() {
        let sql = "SELECT id, name FROM users";
        let result = parse_sql(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_select_with_where() {
        let sql = "SELECT * FROM users WHERE age > 18";
        let result = parse_sql(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_select_with_join() {
        let sql = "SELECT u.name, o.total FROM users u INNER JOIN orders o ON u.id = o.user_id";
        let result = parse_sql(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_spatial_function() {
        let sql = "SELECT ST_Area(geom) FROM buildings";
        let result = parse_sql(sql);
        assert!(result.is_ok());
    }
}
