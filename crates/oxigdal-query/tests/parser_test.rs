//! Parser tests.

use oxigdal_query::Result;
use oxigdal_query::parser::ast::*;
use oxigdal_query::parser::sql::parse_sql;

#[test]
fn test_parse_simple_select() -> Result<()> {
    let sql = "SELECT id, name FROM users";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.projection.len(), 2);
            assert!(select.from.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_select_wildcard() -> Result<()> {
    let sql = "SELECT * FROM users";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.projection.len(), 1);
            assert!(
                matches!(&select.projection[0], SelectItem::Wildcard),
                "Expected wildcard"
            );
            Ok(())
        }
    }
}

#[test]
fn test_parse_select_with_where() -> Result<()> {
    let sql = "SELECT id FROM users WHERE age > 18";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.selection.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_select_with_join() -> Result<()> {
    let sql = "SELECT u.name, o.total FROM users u INNER JOIN orders o ON u.id = o.user_id";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.projection.len(), 2);
            assert!(select.from.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_select_with_aggregates() -> Result<()> {
    let sql = "SELECT COUNT(*), AVG(age) FROM users GROUP BY country";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.projection.len(), 2);
            assert_eq!(select.group_by.len(), 1);
            Ok(())
        }
    }
}

#[test]
fn test_parse_select_with_order_by() -> Result<()> {
    let sql = "SELECT name, age FROM users ORDER BY age DESC, name ASC";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.order_by.len(), 2);
            assert!(!select.order_by[0].asc);
            assert!(select.order_by[1].asc);
            Ok(())
        }
    }
}

#[test]
fn test_parse_select_with_limit_offset() -> Result<()> {
    let sql = "SELECT * FROM users LIMIT 10 OFFSET 20";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.limit, Some(10));
            assert_eq!(select.offset, Some(20));
            Ok(())
        }
    }
}

#[test]
fn test_parse_spatial_query() -> Result<()> {
    let sql = "SELECT geom, name FROM buildings WHERE ST_Intersects(geom, ST_MakeEnvelope(0, 0, 100, 100))";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.selection.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_complex_where_clause() -> Result<()> {
    let sql = "SELECT * FROM users WHERE (age > 18 AND age < 65) OR status = 'active'";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.selection.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_subquery() -> Result<()> {
    let sql = "SELECT * FROM (SELECT id, name FROM users WHERE age > 18) AS adults";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.from.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_case_expression() -> Result<()> {
    let sql = "SELECT CASE WHEN age < 18 THEN 'minor' ELSE 'adult' END FROM users";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.projection.len(), 1);
            Ok(())
        }
    }
}

#[test]
fn test_parse_cast_expression() -> Result<()> {
    let sql = "SELECT CAST(age AS BIGINT) FROM users";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert_eq!(select.projection.len(), 1);
            Ok(())
        }
    }
}

#[test]
fn test_parse_in_list() -> Result<()> {
    let sql = "SELECT * FROM users WHERE country IN ('US', 'UK', 'CA')";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.selection.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_between() -> Result<()> {
    let sql = "SELECT * FROM users WHERE age BETWEEN 18 AND 65";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.selection.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_parse_is_null() -> Result<()> {
    let sql = "SELECT * FROM users WHERE email IS NOT NULL";
    let stmt = parse_sql(sql)?;

    match stmt {
        Statement::Select(select) => {
            assert!(select.selection.is_some());
            Ok(())
        }
    }
}

#[test]
fn test_invalid_sql() {
    let sql = "INVALID SQL STATEMENT";
    let result = parse_sql(sql);
    assert!(result.is_err());
}
