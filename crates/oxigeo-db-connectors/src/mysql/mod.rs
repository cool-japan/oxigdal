//! MySQL/MariaDB spatial database connector.
//!
//! Provides support for reading and writing spatial data to MySQL/MariaDB databases
//! with spatial extensions.

pub mod reader;
pub mod writer;

pub use reader::MySqlFeature;

use crate::error::{Error, Result};
use geo_types::Geometry;
use mysql_async::{Pool, PoolConstraints, PoolOpts, prelude::*};
use std::time::Duration;

/// MySQL connector configuration.
#[derive(Debug, Clone)]
pub struct MySqlConfig {
    /// Database host.
    pub host: String,
    /// Database port.
    pub port: u16,
    /// Database name.
    pub database: String,
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Minimum pool size.
    pub min_connections: usize,
    /// Maximum pool size.
    pub max_connections: usize,
    /// Connection timeout.
    pub connection_timeout: Duration,
    /// Enable SSL.
    pub ssl: bool,
}

impl Default for MySqlConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            database: "gis".to_string(),
            username: "root".to_string(),
            password: String::new(),
            min_connections: 1,
            max_connections: 10,
            connection_timeout: Duration::from_secs(30),
            ssl: false,
        }
    }
}

/// MySQL spatial database connector.
pub struct MySqlConnector {
    pool: Pool,
    #[allow(dead_code)]
    config: MySqlConfig,
}

impl MySqlConnector {
    /// Create a new MySQL connector.
    pub fn new(config: MySqlConfig) -> Result<Self> {
        let url = if config.password.is_empty() {
            format!(
                "mysql://{}@{}:{}/{}",
                config.username, config.host, config.port, config.database
            )
        } else {
            format!(
                "mysql://{}:{}@{}:{}/{}",
                config.username, config.password, config.host, config.port, config.database
            )
        };

        let pool_constraints = PoolConstraints::new(config.min_connections, config.max_connections)
            .ok_or_else(|| Error::Configuration("Invalid pool constraints".to_string()))?;

        let pool_opts = PoolOpts::default()
            .with_constraints(pool_constraints)
            .with_inactive_connection_ttl(Duration::from_secs(300));

        let pool = Pool::new(
            mysql_async::OptsBuilder::from_opts(
                mysql_async::Opts::from_url(&url)
                    .map_err(|e| Error::InvalidConnectionString(e.to_string()))?,
            )
            .pool_opts(pool_opts),
        );

        Ok(Self { pool, config })
    }

    /// Get a connection from the pool.
    pub async fn get_conn(&self) -> Result<mysql_async::Conn> {
        self.pool
            .get_conn()
            .await
            .map_err(|e| Error::Connection(e.to_string()))
    }

    /// Check if the connection is healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let mut conn = self.get_conn().await?;
        let result: Option<i32> = conn
            .query_first("SELECT 1")
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(result == Some(1))
    }

    /// Get database version.
    pub async fn version(&self) -> Result<String> {
        let mut conn = self.get_conn().await?;
        let version: Option<String> = conn
            .query_first("SELECT VERSION()")
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        version.ok_or_else(|| Error::Query("Failed to get version".to_string()))
    }

    /// Check if spatial extensions are enabled.
    pub async fn has_spatial_support(&self) -> Result<bool> {
        let mut conn = self.get_conn().await?;
        let result: Option<String> = conn
            .query_first("SELECT ST_AsText(ST_GeomFromText('POINT(0 0)'))")
            .await
            .ok()
            .flatten();

        Ok(result.is_some())
    }

    /// Create a spatial table.
    pub async fn create_spatial_table(
        &self,
        table_name: &str,
        geometry_column: &str,
        geometry_type: &str,
        srid: i32,
        additional_columns: &[(String, String)],
    ) -> Result<()> {
        let mut conn = self.get_conn().await?;

        let mut columns = vec![
            "id BIGINT PRIMARY KEY AUTO_INCREMENT".to_string(),
            format!("{} GEOMETRY NOT NULL", geometry_column),
        ];

        for (col_name, col_type) in additional_columns {
            columns.push(format!("{} {}", col_name, col_type));
        }

        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} ({}, SPATIAL INDEX({}))",
            table_name,
            columns.join(", "),
            geometry_column
        );

        conn.query_drop(&create_sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        // Set SRID if supported (MySQL 8.0+)
        if srid != 0 {
            let alter_sql = format!(
                "ALTER TABLE {} MODIFY COLUMN {} {} SRID {}",
                table_name, geometry_column, geometry_type, srid
            );

            // Try to set SRID, but don't fail if not supported
            let _ = conn.query_drop(&alter_sql).await;
        }

        Ok(())
    }

    /// Drop a table.
    pub async fn drop_table(&self, table_name: &str) -> Result<()> {
        let mut conn = self.get_conn().await?;
        let sql = format!("DROP TABLE IF EXISTS {}", table_name);

        conn.query_drop(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// List all tables.
    pub async fn list_tables(&self) -> Result<Vec<String>> {
        let mut conn = self.get_conn().await?;
        let tables: Vec<String> = conn
            .query("SHOW TABLES")
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(tables)
    }

    /// Get table schema.
    pub async fn table_schema(&self, table_name: &str) -> Result<Vec<(String, String)>> {
        let mut conn = self.get_conn().await?;
        let sql = format!("DESCRIBE {}", table_name);

        let rows: Vec<(String, String)> = conn
            .query_map(&sql, |(field, field_type): (String, String)| {
                (field, field_type)
            })
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(rows)
    }

    /// Execute raw SQL.
    pub async fn execute(&self, sql: &str) -> Result<u64> {
        let mut conn = self.get_conn().await?;
        conn.query_drop(sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(conn.affected_rows())
    }

    /// Begin a transaction.
    /// Returns a connection that can be used to start a transaction.
    /// Use `conn.start_transaction()` to begin the transaction.
    pub async fn get_conn_for_transaction(&self) -> Result<mysql_async::Conn> {
        self.get_conn().await
    }
}

/// Convert geo-types Geometry to WKT string.
pub fn geometry_to_wkt(geom: &Geometry<f64>) -> Result<String> {
    use std::fmt::Write;

    let mut wkt = String::new();

    match geom {
        Geometry::Point(p) => {
            write!(wkt, "POINT({} {})", p.x(), p.y())
                .map_err(|e| Error::TypeConversion(e.to_string()))?;
        }
        Geometry::LineString(ls) => {
            wkt.push_str("LINESTRING(");
            for (i, coord) in ls.coords().enumerate() {
                if i > 0 {
                    wkt.push(',');
                }
                write!(wkt, "{} {}", coord.x, coord.y)
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
            }
            wkt.push(')');
        }
        Geometry::Polygon(poly) => {
            wkt.push_str("POLYGON(");
            let exterior = poly.exterior();
            wkt.push('(');
            for (i, coord) in exterior.coords().enumerate() {
                if i > 0 {
                    wkt.push(',');
                }
                write!(wkt, "{} {}", coord.x, coord.y)
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
            }
            wkt.push(')');

            for interior in poly.interiors() {
                wkt.push_str(",(");
                for (i, coord) in interior.coords().enumerate() {
                    if i > 0 {
                        wkt.push(',');
                    }
                    write!(wkt, "{} {}", coord.x, coord.y)
                        .map_err(|e| Error::TypeConversion(e.to_string()))?;
                }
                wkt.push(')');
            }
            wkt.push(')');
        }
        Geometry::MultiPoint(mp) => {
            wkt.push_str("MULTIPOINT(");
            for (i, point) in mp.iter().enumerate() {
                if i > 0 {
                    wkt.push(',');
                }
                write!(wkt, "{} {}", point.x(), point.y())
                    .map_err(|e| Error::TypeConversion(e.to_string()))?;
            }
            wkt.push(')');
        }
        Geometry::MultiLineString(mls) => {
            wkt.push_str("MULTILINESTRING(");
            for (i, ls) in mls.iter().enumerate() {
                if i > 0 {
                    wkt.push(',');
                }
                wkt.push('(');
                for (j, coord) in ls.coords().enumerate() {
                    if j > 0 {
                        wkt.push(',');
                    }
                    write!(wkt, "{} {}", coord.x, coord.y)
                        .map_err(|e| Error::TypeConversion(e.to_string()))?;
                }
                wkt.push(')');
            }
            wkt.push(')');
        }
        Geometry::MultiPolygon(mpoly) => {
            wkt.push_str("MULTIPOLYGON(");
            for (i, poly) in mpoly.iter().enumerate() {
                if i > 0 {
                    wkt.push(',');
                }
                wkt.push_str("((");
                for (j, coord) in poly.exterior().coords().enumerate() {
                    if j > 0 {
                        wkt.push(',');
                    }
                    write!(wkt, "{} {}", coord.x, coord.y)
                        .map_err(|e| Error::TypeConversion(e.to_string()))?;
                }
                wkt.push_str("))");
            }
            wkt.push(')');
        }
        _ => {
            return Err(Error::TypeConversion(format!(
                "Unsupported geometry type: {:?}",
                geom
            )));
        }
    }

    Ok(wkt)
}

/// Convert WKT string to geo-types Geometry.
pub fn wkt_to_geometry(wkt: &str) -> Result<Geometry<f64>> {
    use std::str::FromStr;

    let wkt_geom: wkt::Wkt<f64> = wkt::Wkt::from_str(wkt)
        .map_err(|e| Error::GeometryParsing(format!("Failed to parse WKT: {}", e)))?;
    let geo_geom = Geometry::try_from(wkt_geom)
        .map_err(|e| Error::GeometryParsing(format!("Failed to convert WKT: {}", e)))?;

    Ok(geo_geom)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use geo_types::{point, polygon};

    #[test]
    fn test_point_to_wkt() {
        let p = Geometry::Point(point!(x: 1.0, y: 2.0));
        let wkt = geometry_to_wkt(&p).expect("Failed to convert");
        assert_eq!(wkt, "POINT(1 2)");
    }

    #[test]
    fn test_polygon_to_wkt() {
        let poly = polygon![
            (x: 0.0, y: 0.0),
            (x: 4.0, y: 0.0),
            (x: 4.0, y: 4.0),
            (x: 0.0, y: 4.0),
            (x: 0.0, y: 0.0),
        ];
        let geom = Geometry::Polygon(poly);
        let wkt = geometry_to_wkt(&geom).expect("Failed to convert");
        assert!(wkt.starts_with("POLYGON("));
    }

    #[test]
    fn test_wkt_to_geometry() {
        let wkt = "POINT(1 2)";
        let geom = wkt_to_geometry(wkt).expect("Failed to parse");
        match geom {
            Geometry::Point(p) => {
                assert_eq!(p.x(), 1.0);
                assert_eq!(p.y(), 2.0);
            }
            _ => panic!("Expected Point geometry"),
        }
    }
}
