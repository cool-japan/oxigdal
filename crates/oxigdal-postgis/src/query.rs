//! Spatial query builder for PostGIS
//!
//! This module provides a fluent API for building spatial queries.

use crate::connection::ConnectionPool;
use crate::error::{QueryError, Result};
use crate::sql::{ColumnName, TableName, WhereClause, functions};
use crate::types::FeatureBuilder;
use oxigdal_core::types::BoundingBox;
use oxigdal_core::vector::feature::Feature;
use oxigdal_core::vector::geometry::Geometry;

/// Spatial query builder
pub struct SpatialQuery {
    table: TableName,
    columns: Vec<String>,
    where_clause: WhereClause,
    geometry_column: String,
    id_column: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    order_by: Vec<String>,
    srid: Option<i32>,
}

impl SpatialQuery {
    /// Creates a new spatial query
    pub fn new(table_name: impl Into<String>) -> Result<Self> {
        Ok(Self {
            table: TableName::new(table_name)?,
            columns: vec!["*".to_string()],
            where_clause: WhereClause::new(),
            geometry_column: "geom".to_string(),
            id_column: Some("id".to_string()),
            limit: None,
            offset: None,
            order_by: Vec::new(),
            srid: None,
        })
    }

    /// Sets the geometry column name
    pub fn geometry_column(mut self, column: impl Into<String>) -> Self {
        self.geometry_column = column.into();
        self
    }

    /// Sets the ID column name
    pub fn id_column(mut self, column: impl Into<String>) -> Self {
        self.id_column = Some(column.into());
        self
    }

    /// Sets specific columns to select
    pub fn select(mut self, columns: &[&str]) -> Self {
        self.columns = columns.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Adds a WHERE condition
    pub fn where_clause(mut self, condition: impl Into<String>) -> Self {
        self.where_clause = self.where_clause.and(condition);
        self
    }

    /// Adds a bounding box filter
    pub fn where_bbox(mut self, bbox: &BoundingBox) -> Result<Self> {
        let srid = self.srid.unwrap_or(4326);
        self.where_clause = self.where_clause.bbox(&self.geometry_column, bbox, srid)?;
        Ok(self)
    }

    /// Filters features that intersect with a geometry
    pub fn where_intersects(mut self, _geometry: &Geometry) -> Result<Self> {
        let condition = format!(
            "ST_Intersects({}, {})",
            ColumnName::new(&self.geometry_column)?.quoted(),
            functions::geom_from_wkb(1, self.srid)
        );
        self.where_clause = self.where_clause.and(condition);
        Ok(self)
    }

    /// Filters features that contain a geometry
    pub fn where_contains(mut self, _geometry: &Geometry) -> Result<Self> {
        let condition = format!(
            "ST_Contains({}, {})",
            ColumnName::new(&self.geometry_column)?.quoted(),
            functions::geom_from_wkb(1, self.srid)
        );
        self.where_clause = self.where_clause.and(condition);
        Ok(self)
    }

    /// Filters features within a geometry
    pub fn where_within(mut self, _geometry: &Geometry) -> Result<Self> {
        let condition = format!(
            "ST_Within({}, {})",
            ColumnName::new(&self.geometry_column)?.quoted(),
            functions::geom_from_wkb(1, self.srid)
        );
        self.where_clause = self.where_clause.and(condition);
        Ok(self)
    }

    /// Filters features within a distance
    pub fn where_dwithin(mut self, _geometry: &Geometry, distance: f64) -> Result<Self> {
        let condition = format!(
            "ST_DWithin({}, {}, {distance})",
            ColumnName::new(&self.geometry_column)?.quoted(),
            functions::geom_from_wkb(1, self.srid)
        );
        self.where_clause = self.where_clause.and(condition);
        Ok(self)
    }

    /// Sets the SRID for spatial operations
    pub const fn srid(mut self, srid: i32) -> Self {
        self.srid = Some(srid);
        self
    }

    /// Sets the result limit
    pub const fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Sets the result offset
    pub const fn offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Adds an ORDER BY clause
    pub fn order_by(mut self, column: impl Into<String>, ascending: bool) -> Self {
        let direction = if ascending { "ASC" } else { "DESC" };
        self.order_by.push(format!("{} {direction}", column.into()));
        self
    }

    /// Orders by distance from a geometry
    pub fn order_by_distance(mut self, _geometry: &Geometry) -> Result<Self> {
        let distance_expr = format!(
            "ST_Distance({}, {})",
            ColumnName::new(&self.geometry_column)?.quoted(),
            functions::geom_from_wkb(1, self.srid)
        );
        self.order_by.push(format!("{distance_expr} ASC"));
        Ok(self)
    }

    /// Builds the SQL query
    pub fn build_sql(&self) -> Result<String> {
        let mut sql = String::from("SELECT ");

        // Add columns
        sql.push_str(&self.columns.join(", "));

        // FROM clause
        sql.push_str(&format!(" FROM {}", self.table));

        // WHERE clause
        if let Some(where_sql) = self.where_clause.build() {
            sql.push(' ');
            sql.push_str(&where_sql);
        }

        // ORDER BY clause
        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&self.order_by.join(", "));
        }

        // LIMIT clause
        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        // OFFSET clause
        if let Some(offset) = self.offset {
            sql.push_str(&format!(" OFFSET {offset}"));
        }

        Ok(sql)
    }

    /// Executes the query and returns features
    pub async fn execute(self, pool: &ConnectionPool) -> Result<Vec<Feature>> {
        let sql = self.build_sql()?;
        let client = pool.get().await?;

        let rows = client
            .query(&sql, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let mut features = Vec::with_capacity(rows.len());
        for row in rows {
            let feature = FeatureBuilder::new()
                .geometry_column(&self.geometry_column)
                .build_from_row(&row)?;
            features.push(feature);
        }

        Ok(features)
    }

    /// Executes the query and returns count
    pub async fn count(self, pool: &ConnectionPool) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM ");
        sql.push_str(&self.table.to_string());

        if let Some(where_sql) = self.where_clause.build() {
            sql.push(' ');
            sql.push_str(&where_sql);
        }

        let client = pool.get().await?;
        let row = client
            .query_one(&sql, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let count: i64 = row.get(0);
        Ok(count)
    }
}

/// Spatial join builder
pub struct SpatialJoin {
    left_table: TableName,
    right_table: TableName,
    join_type: JoinType,
    join_condition: String,
    where_clause: WhereClause,
    limit: Option<usize>,
}

/// Join type
#[derive(Debug, Clone, Copy)]
pub enum JoinType {
    /// INNER JOIN
    Inner,
    /// LEFT JOIN
    Left,
    /// RIGHT JOIN
    Right,
}

impl JoinType {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Inner => "INNER JOIN",
            Self::Left => "LEFT JOIN",
            Self::Right => "RIGHT JOIN",
        }
    }
}

impl SpatialJoin {
    /// Creates a new spatial join
    pub fn new(left_table: impl Into<String>, right_table: impl Into<String>) -> Result<Self> {
        Ok(Self {
            left_table: TableName::new(left_table)?,
            right_table: TableName::new(right_table)?,
            join_type: JoinType::Inner,
            join_condition: String::new(),
            where_clause: WhereClause::new(),
            limit: None,
        })
    }

    /// Sets the join type
    pub const fn join_type(mut self, join_type: JoinType) -> Self {
        self.join_type = join_type;
        self
    }

    /// Joins on intersecting geometries
    pub fn on_intersects(
        mut self,
        left_geom: impl Into<String>,
        right_geom: impl Into<String>,
    ) -> Result<Self> {
        let left = ColumnName::new(left_geom)?;
        let right = ColumnName::new(right_geom)?;
        self.join_condition = format!("ST_Intersects({}, {})", left.quoted(), right.quoted());
        Ok(self)
    }

    /// Adds a WHERE condition
    pub fn where_clause(mut self, condition: impl Into<String>) -> Self {
        self.where_clause = self.where_clause.and(condition);
        self
    }

    /// Sets the limit
    pub const fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Builds the SQL query
    pub fn build_sql(&self) -> Result<String> {
        let mut sql = format!(
            "SELECT * FROM {} {} {} ON {}",
            self.left_table,
            self.join_type.as_str(),
            self.right_table,
            self.join_condition
        );

        if let Some(where_sql) = self.where_clause.build() {
            sql.push(' ');
            sql.push_str(&where_sql);
        }

        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        Ok(sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_query_basic() {
        let query = SpatialQuery::new("buildings").ok();
        assert!(query.is_some());
        let query = query.expect("query creation failed");

        let sql = query.build_sql().ok();
        assert!(sql.is_some());
        let sql = sql.expect("SQL build failed");
        assert!(sql.contains("SELECT"));
        assert!(sql.contains("FROM"));
        assert!(sql.contains("buildings"));
    }

    #[test]
    fn test_spatial_query_limit() {
        let query = SpatialQuery::new("buildings").ok();
        assert!(query.is_some());
        let query = query.expect("query creation failed").limit(10);

        let sql = query.build_sql().ok();
        assert!(sql.is_some());
        let sql = sql.expect("SQL build failed");
        assert!(sql.contains("LIMIT 10"));
    }

    #[test]
    fn test_spatial_query_where() {
        let query = SpatialQuery::new("buildings").ok();
        assert!(query.is_some());
        let query = query
            .expect("query creation failed")
            .where_clause("id > 10");

        let sql = query.build_sql().ok();
        assert!(sql.is_some());
        let sql = sql.expect("SQL build failed");
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("id > 10"));
    }

    #[test]
    fn test_spatial_join() {
        let join = SpatialJoin::new("parcels", "buildings").ok();
        assert!(join.is_some());
        let join = join
            .expect("join creation failed")
            .on_intersects("parcels.geom", "buildings.geom")
            .ok();
        assert!(join.is_some());

        let join = join.expect("join on_intersects failed");
        let sql = join.build_sql().ok();
        assert!(sql.is_some());
        let sql = sql.expect("SQL build failed");
        assert!(sql.contains("INNER JOIN"));
        assert!(sql.contains("ST_Intersects"));
    }
}
