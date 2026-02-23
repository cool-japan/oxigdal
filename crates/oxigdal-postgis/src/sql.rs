//! SQL generation helpers for PostGIS queries
//!
//! This module provides safe SQL query generation with SQL injection prevention.

use crate::error::{Result, SqlError};
use oxigdal_core::types::BoundingBox;

/// SQL identifier validator and quoter
pub struct SqlIdentifier(String);

impl SqlIdentifier {
    /// Creates a new SQL identifier
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        Self::validate(&name)?;
        Ok(Self(name))
    }

    /// Validates an SQL identifier
    fn validate(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(SqlError::InvalidIdentifier {
                identifier: name.to_string(),
            }
            .into());
        }

        // Check for SQL injection attempts
        let suspicious = ['\'', '"', ';', '-', '/', '*', '\\', '\0'];
        if name.chars().any(|c| suspicious.contains(&c)) {
            return Err(SqlError::InjectionAttempt {
                input: name.to_string(),
            }
            .into());
        }

        // Support qualified names (table.column) by splitting on dots
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() > 2 {
            return Err(SqlError::InvalidIdentifier {
                identifier: name.to_string(),
            }
            .into());
        }

        // Validate each part
        for part in parts {
            if part.is_empty() {
                return Err(SqlError::InvalidIdentifier {
                    identifier: name.to_string(),
                }
                .into());
            }

            // Must start with letter or underscore
            if !part
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                return Err(SqlError::InvalidIdentifier {
                    identifier: name.to_string(),
                }
                .into());
            }

            // Can only contain alphanumeric and underscore
            if !part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(SqlError::InvalidIdentifier {
                    identifier: name.to_string(),
                }
                .into());
            }
        }

        Ok(())
    }

    /// Returns the quoted identifier
    pub fn quoted(&self) -> String {
        // Handle qualified names (table.column) by quoting each part
        if self.0.contains('.') {
            let parts: Vec<&str> = self.0.split('.').collect();
            parts
                .iter()
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(".")
        } else {
            format!("\"{}\"", self.0)
        }
    }

    /// Returns the unquoted identifier
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SqlIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.quoted())
    }
}

/// Table name wrapper
pub struct TableName {
    schema: Option<SqlIdentifier>,
    table: SqlIdentifier,
}

impl TableName {
    /// Creates a new table name
    pub fn new(table: impl Into<String>) -> Result<Self> {
        Ok(Self {
            schema: None,
            table: SqlIdentifier::new(table)?,
        })
    }

    /// Creates a new table name with schema
    pub fn with_schema(schema: impl Into<String>, table: impl Into<String>) -> Result<Self> {
        Ok(Self {
            schema: Some(SqlIdentifier::new(schema)?),
            table: SqlIdentifier::new(table)?,
        })
    }

    /// Returns the fully qualified table name
    pub fn qualified(&self) -> String {
        if let Some(ref schema) = self.schema {
            format!("{}.{}", schema.quoted(), self.table.quoted())
        } else {
            self.table.quoted()
        }
    }

    /// Returns the table name
    pub fn name(&self) -> &str {
        self.table.as_str()
    }

    /// Returns the schema name
    pub fn schema(&self) -> Option<&str> {
        self.schema.as_ref().map(|s| s.as_str())
    }
}

impl std::fmt::Display for TableName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.qualified())
    }
}

/// Column name wrapper
pub struct ColumnName {
    name: SqlIdentifier,
}

impl ColumnName {
    /// Creates a new column name
    pub fn new(name: impl Into<String>) -> Result<Self> {
        Ok(Self {
            name: SqlIdentifier::new(name)?,
        })
    }

    /// Returns the quoted column name
    pub fn quoted(&self) -> String {
        self.name.quoted()
    }

    /// Returns the column name
    pub fn as_str(&self) -> &str {
        self.name.as_str()
    }
}

impl std::fmt::Display for ColumnName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.quoted())
    }
}

/// PostGIS spatial function builder
pub struct SpatialFunction {
    function: String,
    args: Vec<String>,
}

impl SpatialFunction {
    /// Creates a new spatial function
    pub fn new(name: &str) -> Result<Self> {
        Self::validate_function_name(name)?;
        Ok(Self {
            function: name.to_string(),
            args: Vec::new(),
        })
    }

    fn validate_function_name(name: &str) -> Result<()> {
        // PostGIS functions should start with ST_
        if !name.starts_with("ST_") && !name.starts_with("st_") {
            return Err(SqlError::InvalidSpatialFunction {
                function: name.to_string(),
            }
            .into());
        }

        // Must be alphanumeric with underscores
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(SqlError::InvalidSpatialFunction {
                function: name.to_string(),
            }
            .into());
        }

        Ok(())
    }

    /// Adds an argument
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Builds the function call SQL
    pub fn build(&self) -> String {
        format!("{}({})", self.function, self.args.join(", "))
    }
}

/// Common PostGIS spatial functions
pub mod functions {
    use super::*;

    /// ST_AsText - Convert geometry to WKT
    pub fn as_text(geom_column: &str) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_AsText({})", col.quoted()))
    }

    /// ST_AsBinary - Convert geometry to WKB
    pub fn as_binary(geom_column: &str) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_AsBinary({})", col.quoted()))
    }

    /// ST_SetSRID - Set the SRID of a geometry
    pub fn set_srid(geom_expr: &str, srid: i32) -> String {
        format!("ST_SetSRID({geom_expr}, {srid})")
    }

    /// ST_Transform - Transform geometry to different SRID
    pub fn transform(geom_column: &str, srid: i32) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_Transform({}, {srid})", col.quoted()))
    }

    /// ST_Buffer - Create buffer around geometry
    pub fn buffer(geom_column: &str, distance: f64) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_Buffer({}, {distance})", col.quoted()))
    }

    /// ST_Intersection - Compute intersection of geometries
    pub fn intersection(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!(
            "ST_Intersection({}, {})",
            col1.quoted(),
            col2.quoted()
        ))
    }

    /// ST_Intersects - Test if geometries intersect
    pub fn intersects(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!(
            "ST_Intersects({}, {})",
            col1.quoted(),
            col2.quoted()
        ))
    }

    /// ST_Contains - Test if geometry contains another
    pub fn contains(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!("ST_Contains({}, {})", col1.quoted(), col2.quoted()))
    }

    /// ST_Within - Test if geometry is within another
    pub fn within(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!("ST_Within({}, {})", col1.quoted(), col2.quoted()))
    }

    /// ST_DWithin - Test if geometries are within distance
    pub fn d_within(geom1: &str, geom2: &str, distance: f64) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!(
            "ST_DWithin({}, {}, {distance})",
            col1.quoted(),
            col2.quoted()
        ))
    }

    /// ST_Distance - Compute distance between geometries
    pub fn distance(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!("ST_Distance({}, {})", col1.quoted(), col2.quoted()))
    }

    /// ST_Area - Compute area of geometry
    pub fn area(geom_column: &str) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_Area({})", col.quoted()))
    }

    /// ST_Length - Compute length of geometry
    pub fn length(geom_column: &str) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_Length({})", col.quoted()))
    }

    /// ST_Centroid - Compute centroid of geometry
    pub fn centroid(geom_column: &str) -> Result<String> {
        let col = ColumnName::new(geom_column)?;
        Ok(format!("ST_Centroid({})", col.quoted()))
    }

    /// ST_Union - Compute union of geometries
    pub fn union(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!("ST_Union({}, {})", col1.quoted(), col2.quoted()))
    }

    /// ST_Difference - Compute difference of geometries
    pub fn difference(geom1: &str, geom2: &str) -> Result<String> {
        let col1 = ColumnName::new(geom1)?;
        let col2 = ColumnName::new(geom2)?;
        Ok(format!(
            "ST_Difference({}, {})",
            col1.quoted(),
            col2.quoted()
        ))
    }

    /// ST_MakeEnvelope - Create bounding box geometry from coordinates
    pub fn make_envelope(bbox: &BoundingBox, srid: i32) -> String {
        format!(
            "ST_MakeEnvelope({}, {}, {}, {}, {srid})",
            bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y
        )
    }

    /// ST_GeomFromWKB - Create geometry from WKB
    pub fn geom_from_wkb(param_index: usize, srid: Option<i32>) -> String {
        if let Some(srid) = srid {
            format!("ST_GeomFromWKB(${param_index}, {srid})")
        } else {
            format!("ST_GeomFromWKB(${param_index})")
        }
    }
}

/// Spatial index hint builder
pub struct SpatialIndexHint {
    table: String,
    column: String,
}

impl SpatialIndexHint {
    /// Creates a new spatial index hint
    pub fn new(table: impl Into<String>, column: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            column: column.into(),
        }
    }

    /// Builds a GIST index creation statement
    pub fn create_index_sql(&self, index_name: Option<&str>) -> Result<String> {
        let table = TableName::new(&self.table)?;
        let column = ColumnName::new(&self.column)?;

        let idx_name = if let Some(name) = index_name {
            SqlIdentifier::new(name)?
        } else {
            SqlIdentifier::new(format!("{}_{}_gist", self.table, self.column))?
        };

        Ok(format!(
            "CREATE INDEX IF NOT EXISTS {} ON {} USING GIST ({})",
            idx_name.quoted(),
            table.qualified(),
            column.quoted()
        ))
    }

    /// Builds a spatial index check query
    pub fn has_index_sql(&self) -> Result<String> {
        let table = TableName::new(&self.table)?;
        let column = ColumnName::new(&self.column)?;

        Ok(format!(
            "SELECT COUNT(*) > 0 FROM pg_indexes WHERE tablename = '{}' AND indexdef LIKE '%USING gist%{}%'",
            table.name(),
            column.as_str()
        ))
    }
}

/// WHERE clause builder for spatial queries
pub struct WhereClause {
    conditions: Vec<String>,
}

impl WhereClause {
    /// Creates a new WHERE clause builder
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    /// Adds a condition
    pub fn and(mut self, condition: impl Into<String>) -> Self {
        self.conditions.push(condition.into());
        self
    }

    /// Adds an OR condition
    pub fn or(mut self, condition: impl Into<String>) -> Self {
        if let Some(last) = self.conditions.last_mut() {
            *last = format!("({last}) OR ({condition})", condition = condition.into());
        } else {
            self.conditions.push(condition.into());
        }
        self
    }

    /// Adds a bounding box filter
    pub fn bbox(self, geom_column: &str, bbox: &BoundingBox, srid: i32) -> Result<Self> {
        let col = ColumnName::new(geom_column)?;
        let envelope = functions::make_envelope(bbox, srid);
        let condition = format!("{} && {envelope}", col.quoted());
        Ok(self.and(condition))
    }

    /// Builds the WHERE clause
    pub fn build(&self) -> Option<String> {
        if self.conditions.is_empty() {
            None
        } else {
            Some(format!("WHERE {}", self.conditions.join(" AND ")))
        }
    }
}

impl Default for WhereClause {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_identifier_validation() {
        assert!(SqlIdentifier::new("valid_name").is_ok());
        assert!(SqlIdentifier::new("_valid").is_ok());
        assert!(SqlIdentifier::new("table123").is_ok());
        assert!(SqlIdentifier::new("123invalid").is_err());
        assert!(SqlIdentifier::new("bad-name").is_err());
        assert!(SqlIdentifier::new("bad;name").is_err());
        assert!(SqlIdentifier::new("bad'name").is_err());
    }

    #[test]
    fn test_sql_identifier_quoting() {
        let ident = SqlIdentifier::new("my_table").ok();
        assert!(ident.is_some());
        let ident = ident.expect("identifier creation failed");
        assert_eq!(ident.quoted(), "\"my_table\"");
        assert_eq!(ident.as_str(), "my_table");
    }

    #[test]
    fn test_table_name() {
        let table = TableName::new("buildings").ok();
        assert!(table.is_some());
        let table = table.expect("table name creation failed");
        assert_eq!(table.qualified(), "\"buildings\"");
    }

    #[test]
    fn test_table_name_with_schema() {
        let table = TableName::with_schema("public", "buildings").ok();
        assert!(table.is_some());
        let table = table.expect("table name creation failed");
        assert_eq!(table.qualified(), "\"public\".\"buildings\"");
        assert_eq!(table.schema(), Some("public"));
    }

    #[test]
    fn test_column_name() {
        let col = ColumnName::new("geom").ok();
        assert!(col.is_some());
        let col = col.expect("column name creation failed");
        assert_eq!(col.quoted(), "\"geom\"");
    }

    #[test]
    fn test_spatial_function() {
        let func = SpatialFunction::new("ST_Buffer").ok();
        assert!(func.is_some());
        let func = func.expect("spatial function creation failed");
        let sql = func.arg("geom").arg("100").build();
        assert_eq!(sql, "ST_Buffer(geom, 100)");
    }

    #[test]
    fn test_spatial_function_invalid() {
        assert!(SpatialFunction::new("InvalidFunc").is_err());
        assert!(SpatialFunction::new("ST_Bad;Name").is_err());
    }

    #[test]
    fn test_functions_buffer() {
        let sql = functions::buffer("geom", 100.0).ok();
        assert!(sql.is_some());
        let sql = sql.expect("buffer function failed");
        assert!(sql.contains("ST_Buffer"));
        assert!(sql.contains("geom"));
        assert!(sql.contains("100"));
    }

    #[test]
    fn test_functions_intersects() {
        let sql = functions::intersects("geom1", "geom2").ok();
        assert!(sql.is_some());
        let sql = sql.expect("intersects function failed");
        assert!(sql.contains("ST_Intersects"));
    }

    #[test]
    fn test_where_clause() {
        let where_clause = WhereClause::new().and("id > 10").and("name = 'test'");

        let sql = where_clause.build();
        assert!(sql.is_some());
        let sql = sql.expect("where clause build failed");
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("id > 10"));
        assert!(sql.contains("AND"));
    }

    #[test]
    fn test_where_clause_or() {
        let where_clause = WhereClause::new().and("id > 10").or("id < 5");

        let sql = where_clause.build();
        assert!(sql.is_some());
        let sql = sql.expect("where clause build failed");
        assert!(sql.contains("OR"));
    }

    #[test]
    fn test_spatial_index_hint() {
        let hint = SpatialIndexHint::new("buildings", "geom");
        let sql = hint.create_index_sql(Some("buildings_geom_idx")).ok();
        assert!(sql.is_some());
        let sql = sql.expect("index creation SQL failed");
        assert!(sql.contains("CREATE INDEX"));
        assert!(sql.contains("GIST"));
    }
}
