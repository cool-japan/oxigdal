//! Database source handling for WFS
//!
//! Provides database connection handling, feature counting, and caching
//! for WFS database-backed feature types.

use crate::error::{ServiceError, ServiceResult};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Database connection type
#[derive(Debug, Clone, Default)]
pub enum DatabaseType {
    /// PostgreSQL/PostGIS
    #[default]
    PostGis,
    /// MySQL with spatial extensions
    MySql,
    /// SQLite/SpatiaLite
    Sqlite,
    /// Generic SQL database
    Generic,
}

/// Database source configuration
#[derive(Debug, Clone)]
pub struct DatabaseSource {
    /// Connection string
    pub connection_string: String,
    /// Database type
    pub database_type: DatabaseType,
    /// Table name
    pub table_name: String,
    /// Geometry column name
    pub geometry_column: String,
    /// Feature ID column name (optional)
    pub id_column: Option<String>,
    /// SRID for spatial operations
    pub srid: Option<i32>,
    /// Schema name (optional)
    pub schema: Option<String>,
    /// Count cache settings
    pub count_cache: Option<CountCacheConfig>,
}

impl DatabaseSource {
    /// Create a new database source
    pub fn new(connection_string: impl Into<String>, table_name: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            database_type: DatabaseType::default(),
            table_name: table_name.into(),
            geometry_column: "geom".to_string(),
            id_column: Some("id".to_string()),
            srid: Some(4326),
            schema: None,
            count_cache: Some(CountCacheConfig::default()),
        }
    }

    /// Set the database type
    pub fn with_database_type(mut self, db_type: DatabaseType) -> Self {
        self.database_type = db_type;
        self
    }

    /// Set the geometry column name
    pub fn with_geometry_column(mut self, column: impl Into<String>) -> Self {
        self.geometry_column = column.into();
        self
    }

    /// Set the ID column name
    pub fn with_id_column(mut self, column: impl Into<String>) -> Self {
        self.id_column = Some(column.into());
        self
    }

    /// Set the SRID
    pub fn with_srid(mut self, srid: i32) -> Self {
        self.srid = Some(srid);
        self
    }

    /// Set the schema name
    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    /// Set count cache configuration
    pub fn with_count_cache(mut self, cache: CountCacheConfig) -> Self {
        self.count_cache = Some(cache);
        self
    }

    /// Disable count caching
    pub fn without_count_cache(mut self) -> Self {
        self.count_cache = None;
        self
    }

    /// Get the fully qualified table name
    pub fn qualified_table_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("\"{}\".\"{}\"", schema, self.table_name),
            None => format!("\"{}\"", self.table_name),
        }
    }
}

/// Count cache configuration
#[derive(Debug, Clone)]
pub struct CountCacheConfig {
    /// Cache duration
    pub ttl: Duration,
    /// Maximum cached entries
    pub max_entries: usize,
    /// Use estimation for large tables
    pub use_estimation_threshold: Option<usize>,
}

impl Default for CountCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(60),
            max_entries: 100,
            use_estimation_threshold: Some(1_000_000),
        }
    }
}

/// Cached count entry
#[derive(Debug, Clone)]
struct CachedCount {
    count: usize,
    timestamp: Instant,
    is_estimated: bool,
}

/// Database feature counter with caching
pub struct DatabaseFeatureCounter {
    cache: Arc<dashmap::DashMap<String, CachedCount>>,
    config: CountCacheConfig,
}

impl DatabaseFeatureCounter {
    /// Create a new feature counter
    pub fn new(config: CountCacheConfig) -> Self {
        Self {
            cache: Arc::new(dashmap::DashMap::new()),
            config,
        }
    }

    /// Get count for a database source
    pub async fn get_count(
        &self,
        source: &DatabaseSource,
        filter: Option<&CqlFilter>,
        bbox: Option<&BboxFilter>,
    ) -> ServiceResult<CountResult> {
        let cache_key = self.build_cache_key(source, filter, bbox);

        // Check cache first
        if let Some(cached) = self.get_cached(&cache_key) {
            return Ok(cached);
        }

        // Execute count query
        let result = self.execute_count(source, filter, bbox).await?;

        // Cache the result
        self.cache_result(&cache_key, &result);

        Ok(result)
    }

    /// Build cache key from source and filters
    fn build_cache_key(
        &self,
        source: &DatabaseSource,
        filter: Option<&CqlFilter>,
        bbox: Option<&BboxFilter>,
    ) -> String {
        let mut key = format!("{}:{}", source.connection_string, source.table_name);

        if let Some(f) = filter {
            key.push(':');
            key.push_str(&f.expression);
        }

        if let Some(b) = bbox {
            key.push_str(&format!(
                ":bbox({},{},{},{})",
                b.min_x, b.min_y, b.max_x, b.max_y
            ));
        }

        key
    }

    /// Check cache for existing result
    fn get_cached(&self, key: &str) -> Option<CountResult> {
        if let Some(entry) = self.cache.get(key) {
            if entry.timestamp.elapsed() < self.config.ttl {
                return Some(CountResult {
                    count: entry.count,
                    is_estimated: entry.is_estimated,
                    from_cache: true,
                });
            }
            // Remove expired entry
            drop(entry);
            self.cache.remove(key);
        }
        None
    }

    /// Cache a count result
    fn cache_result(&self, key: &str, result: &CountResult) {
        // Enforce max entries by removing oldest if needed
        if self.cache.len() >= self.config.max_entries {
            // Find and remove oldest entry
            let mut oldest_key = None;
            let mut oldest_time = Instant::now();

            for entry in self.cache.iter() {
                if entry.value().timestamp < oldest_time {
                    oldest_time = entry.value().timestamp;
                    oldest_key = Some(entry.key().clone());
                }
            }

            if let Some(key) = oldest_key {
                self.cache.remove(&key);
            }
        }

        self.cache.insert(
            key.to_string(),
            CachedCount {
                count: result.count,
                timestamp: Instant::now(),
                is_estimated: result.is_estimated,
            },
        );
    }

    /// Execute count query against database
    async fn execute_count(
        &self,
        source: &DatabaseSource,
        filter: Option<&CqlFilter>,
        bbox: Option<&BboxFilter>,
    ) -> ServiceResult<CountResult> {
        // Build the count SQL based on database type
        let sql = self.build_count_sql(source, filter, bbox)?;

        // Execute the query based on database type
        match source.database_type {
            DatabaseType::PostGis => self.execute_postgis_count(source, &sql).await,
            DatabaseType::MySql => self.execute_generic_count(source, &sql).await,
            DatabaseType::Sqlite => self.execute_generic_count(source, &sql).await,
            DatabaseType::Generic => self.execute_generic_count(source, &sql).await,
        }
    }

    /// Build count SQL query
    fn build_count_sql(
        &self,
        source: &DatabaseSource,
        filter: Option<&CqlFilter>,
        bbox: Option<&BboxFilter>,
    ) -> ServiceResult<String> {
        let table = source.qualified_table_name();
        let mut sql = format!("SELECT COUNT(*) FROM {table}");

        let mut conditions: Vec<String> = Vec::new();

        // Add BBOX condition
        if let Some(b) = bbox {
            let geom_col = &source.geometry_column;
            let srid = source.srid.unwrap_or(4326);

            let bbox_condition = match source.database_type {
                DatabaseType::PostGis => {
                    format!(
                        "ST_Intersects(\"{geom_col}\", ST_MakeEnvelope({}, {}, {}, {}, {srid}))",
                        b.min_x, b.min_y, b.max_x, b.max_y
                    )
                }
                DatabaseType::MySql => {
                    format!(
                        "MBRIntersects(`{geom_col}`, ST_GeomFromText('POLYGON(({} {}, {} {}, {} {}, {} {}, {} {}))', {srid}))",
                        b.min_x,
                        b.min_y,
                        b.max_x,
                        b.min_y,
                        b.max_x,
                        b.max_y,
                        b.min_x,
                        b.max_y,
                        b.min_x,
                        b.min_y
                    )
                }
                DatabaseType::Sqlite => {
                    format!(
                        "Intersects(\"{geom_col}\", BuildMbr({}, {}, {}, {}, {srid}))",
                        b.min_x, b.min_y, b.max_x, b.max_y
                    )
                }
                DatabaseType::Generic => {
                    // Generic SQL using envelope intersection
                    format!(
                        "(\"{geom_col}_minx\" <= {} AND \"{geom_col}_maxx\" >= {} AND \"{geom_col}_miny\" <= {} AND \"{geom_col}_maxy\" >= {})",
                        b.max_x, b.min_x, b.max_y, b.min_y
                    )
                }
            };
            conditions.push(bbox_condition);
        }

        // Add CQL filter condition
        if let Some(f) = filter {
            let parsed = f.to_sql(&source.database_type)?;
            conditions.push(parsed);
        }

        // Combine conditions
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        Ok(sql)
    }

    /// Execute count for PostGIS database
    async fn execute_postgis_count(
        &self,
        source: &DatabaseSource,
        sql: &str,
    ) -> ServiceResult<CountResult> {
        // Check if we should use estimation for large tables
        if let Some(threshold) = self
            .config
            .use_estimation_threshold
            .filter(|_| source.count_cache.is_some())
        {
            // Try to get estimated count first
            if let Ok(estimate) = self.get_postgis_estimate(source).await {
                if estimate > threshold {
                    return Ok(CountResult {
                        count: estimate,
                        is_estimated: true,
                        from_cache: false,
                    });
                }
            }
        }

        // Execute actual count
        let count = self
            .execute_sql_count(&source.connection_string, sql)
            .await?;

        Ok(CountResult {
            count,
            is_estimated: false,
            from_cache: false,
        })
    }

    /// Get estimated count from PostgreSQL statistics
    async fn get_postgis_estimate(&self, source: &DatabaseSource) -> ServiceResult<usize> {
        let estimate_sql = match &source.schema {
            Some(schema) => {
                format!(
                    "SELECT reltuples::bigint AS estimate FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = '{}' AND c.relname = '{}'",
                    schema, source.table_name
                )
            }
            None => {
                format!(
                    "SELECT reltuples::bigint AS estimate FROM pg_class \
                     WHERE relname = '{}'",
                    source.table_name
                )
            }
        };

        self.execute_sql_count(&source.connection_string, &estimate_sql)
            .await
    }

    /// Execute generic SQL count
    async fn execute_generic_count(
        &self,
        source: &DatabaseSource,
        sql: &str,
    ) -> ServiceResult<CountResult> {
        let count = self
            .execute_sql_count(&source.connection_string, sql)
            .await?;

        Ok(CountResult {
            count,
            is_estimated: false,
            from_cache: false,
        })
    }

    /// Execute SQL and return count
    async fn execute_sql_count(
        &self,
        _connection_string: &str,
        _sql: &str,
    ) -> ServiceResult<usize> {
        // This is a placeholder for actual database execution
        // In a real implementation, this would:
        // 1. Get a connection from the pool
        // 2. Execute the SQL query
        // 3. Parse the count result
        //
        // For now, we return an error indicating the database is not connected
        // This will be replaced with actual database calls when integrated
        // with oxigdal-postgis or oxigdal-db-connectors

        Err(ServiceError::Internal(
            "Database connection not configured. Use oxigdal-postgis for PostGIS connections."
                .to_string(),
        ))
    }

    /// Clear the count cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        let mut expired = 0;
        let mut valid = 0;

        for entry in self.cache.iter() {
            if entry.value().timestamp.elapsed() < self.config.ttl {
                valid += 1;
            } else {
                expired += 1;
            }
        }

        CacheStats {
            total_entries: self.cache.len(),
            valid_entries: valid,
            expired_entries: expired,
            max_entries: self.config.max_entries,
        }
    }
}

impl Default for DatabaseFeatureCounter {
    fn default() -> Self {
        Self::new(CountCacheConfig::default())
    }
}

/// Count result
#[derive(Debug, Clone)]
pub struct CountResult {
    /// The count value
    pub count: usize,
    /// Whether this is an estimated count
    pub is_estimated: bool,
    /// Whether this was retrieved from cache
    pub from_cache: bool,
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total entries in cache
    pub total_entries: usize,
    /// Valid (non-expired) entries
    pub valid_entries: usize,
    /// Expired entries
    pub expired_entries: usize,
    /// Maximum allowed entries
    pub max_entries: usize,
}

/// CQL (Common Query Language) filter
#[derive(Debug, Clone)]
pub struct CqlFilter {
    /// The CQL expression
    pub expression: String,
}

impl CqlFilter {
    /// Create a new CQL filter
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
        }
    }

    /// Parse and convert CQL to SQL
    pub fn to_sql(&self, db_type: &DatabaseType) -> ServiceResult<String> {
        // Parse CQL expression and convert to SQL
        // This is a simplified implementation - a full CQL parser would be more complex
        let sql = self.parse_cql_expression(db_type)?;
        Ok(sql)
    }

    /// Parse a CQL expression into SQL
    fn parse_cql_expression(&self, db_type: &DatabaseType) -> ServiceResult<String> {
        let expr = self.expression.trim();

        // Handle empty expression
        if expr.is_empty() {
            return Ok("1=1".to_string());
        }

        // Tokenize and parse the expression
        let tokens = self.tokenize(expr)?;
        self.tokens_to_sql(&tokens, db_type)
    }

    /// Tokenize CQL expression
    fn tokenize(&self, expr: &str) -> ServiceResult<Vec<CqlToken>> {
        let mut tokens = Vec::new();
        let mut chars = expr.chars().peekable();
        let mut current = String::new();

        while let Some(c) = chars.next() {
            match c {
                ' ' | '\t' | '\n' | '\r' => {
                    if !current.is_empty() {
                        tokens.push(self.classify_token(&current)?);
                        current.clear();
                    }
                }
                '(' => {
                    if !current.is_empty() {
                        tokens.push(self.classify_token(&current)?);
                        current.clear();
                    }
                    tokens.push(CqlToken::OpenParen);
                }
                ')' => {
                    if !current.is_empty() {
                        tokens.push(self.classify_token(&current)?);
                        current.clear();
                    }
                    tokens.push(CqlToken::CloseParen);
                }
                '\'' => {
                    // String literal
                    if !current.is_empty() {
                        tokens.push(self.classify_token(&current)?);
                        current.clear();
                    }
                    let mut string_val = String::new();
                    while let Some(&next_c) = chars.peek() {
                        chars.next();
                        if next_c == '\'' {
                            // Check for escaped quote
                            if chars.peek() == Some(&'\'') {
                                string_val.push('\'');
                                chars.next();
                            } else {
                                break;
                            }
                        } else {
                            string_val.push(next_c);
                        }
                    }
                    tokens.push(CqlToken::StringLiteral(string_val));
                }
                '=' | '<' | '>' | '!' => {
                    if !current.is_empty() {
                        tokens.push(self.classify_token(&current)?);
                        current.clear();
                    }
                    let mut op = c.to_string();
                    if let Some(&next_c) = chars.peek() {
                        if next_c == '=' || (c == '<' && next_c == '>') {
                            op.push(next_c);
                            chars.next();
                        }
                    }
                    tokens.push(CqlToken::Operator(op));
                }
                ',' => {
                    if !current.is_empty() {
                        tokens.push(self.classify_token(&current)?);
                        current.clear();
                    }
                    tokens.push(CqlToken::Comma);
                }
                _ => {
                    current.push(c);
                }
            }
        }

        if !current.is_empty() {
            tokens.push(self.classify_token(&current)?);
        }

        Ok(tokens)
    }

    /// Classify a token string
    fn classify_token(&self, token: &str) -> ServiceResult<CqlToken> {
        let upper = token.to_uppercase();

        match upper.as_str() {
            "AND" => Ok(CqlToken::And),
            "OR" => Ok(CqlToken::Or),
            "NOT" => Ok(CqlToken::Not),
            "LIKE" => Ok(CqlToken::Operator("LIKE".to_string())),
            "ILIKE" => Ok(CqlToken::Operator("ILIKE".to_string())),
            "IN" => Ok(CqlToken::Operator("IN".to_string())),
            "BETWEEN" => Ok(CqlToken::Operator("BETWEEN".to_string())),
            "IS" => Ok(CqlToken::Operator("IS".to_string())),
            "NULL" => Ok(CqlToken::Null),
            "TRUE" => Ok(CqlToken::BoolLiteral(true)),
            "FALSE" => Ok(CqlToken::BoolLiteral(false)),
            _ => {
                // Check if it's a number
                if let Ok(n) = token.parse::<f64>() {
                    Ok(CqlToken::NumberLiteral(n))
                } else {
                    // It's an identifier (column name)
                    Ok(CqlToken::Identifier(token.to_string()))
                }
            }
        }
    }

    /// Convert tokens to SQL
    fn tokens_to_sql(&self, tokens: &[CqlToken], db_type: &DatabaseType) -> ServiceResult<String> {
        let mut sql = String::new();
        let mut i = 0;

        while i < tokens.len() {
            let token = &tokens[i];

            match token {
                CqlToken::Identifier(name) => {
                    sql.push_str(&self.quote_identifier(name, db_type));
                }
                CqlToken::StringLiteral(val) => {
                    sql.push_str(&format!("'{}'", val.replace('\'', "''")));
                }
                CqlToken::NumberLiteral(n) => {
                    sql.push_str(&n.to_string());
                }
                CqlToken::BoolLiteral(b) => {
                    sql.push_str(if *b { "TRUE" } else { "FALSE" });
                }
                CqlToken::Null => {
                    sql.push_str("NULL");
                }
                CqlToken::And => {
                    sql.push_str(" AND ");
                }
                CqlToken::Or => {
                    sql.push_str(" OR ");
                }
                CqlToken::Not => {
                    sql.push_str("NOT ");
                }
                CqlToken::Operator(op) => {
                    sql.push(' ');
                    sql.push_str(op);
                    sql.push(' ');
                }
                CqlToken::OpenParen => {
                    sql.push('(');
                }
                CqlToken::CloseParen => {
                    sql.push(')');
                }
                CqlToken::Comma => {
                    sql.push_str(", ");
                }
            }

            i += 1;
        }

        Ok(sql.trim().to_string())
    }

    /// Quote an identifier based on database type
    fn quote_identifier(&self, name: &str, db_type: &DatabaseType) -> String {
        match db_type {
            DatabaseType::PostGis | DatabaseType::Sqlite | DatabaseType::Generic => {
                format!("\"{}\"", name.replace('"', "\"\""))
            }
            DatabaseType::MySql => {
                format!("`{}`", name.replace('`', "``"))
            }
        }
    }
}

/// CQL token types
#[derive(Debug, Clone)]
enum CqlToken {
    Identifier(String),
    StringLiteral(String),
    NumberLiteral(f64),
    BoolLiteral(bool),
    Null,
    And,
    Or,
    Not,
    Operator(String),
    OpenParen,
    CloseParen,
    Comma,
}

/// Bounding box filter
#[derive(Debug, Clone, Copy)]
pub struct BboxFilter {
    /// Minimum X coordinate
    pub min_x: f64,
    /// Minimum Y coordinate
    pub min_y: f64,
    /// Maximum X coordinate
    pub max_x: f64,
    /// Maximum Y coordinate
    pub max_y: f64,
    /// Optional CRS (defaults to EPSG:4326)
    pub crs: Option<i32>,
}

impl BboxFilter {
    /// Create a new bounding box filter
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
            crs: None,
        }
    }

    /// Create from a BBOX string (minx,miny,maxx,maxy\[,crs\])
    pub fn from_bbox_string(bbox_str: &str) -> ServiceResult<Self> {
        let parts: Vec<&str> = bbox_str.split(',').collect();

        if parts.len() < 4 {
            return Err(ServiceError::InvalidBbox(
                "BBOX must have at least 4 coordinates".to_string(),
            ));
        }

        let min_x = parts[0]
            .trim()
            .parse::<f64>()
            .map_err(|_| ServiceError::InvalidBbox("Invalid minx".to_string()))?;
        let min_y = parts[1]
            .trim()
            .parse::<f64>()
            .map_err(|_| ServiceError::InvalidBbox("Invalid miny".to_string()))?;
        let max_x = parts[2]
            .trim()
            .parse::<f64>()
            .map_err(|_| ServiceError::InvalidBbox("Invalid maxx".to_string()))?;
        let max_y = parts[3]
            .trim()
            .parse::<f64>()
            .map_err(|_| ServiceError::InvalidBbox("Invalid maxy".to_string()))?;

        let crs = if parts.len() > 4 {
            // Parse CRS - could be "EPSG:4326" or just "4326"
            let crs_str = parts[4].trim();
            let srid = if crs_str.to_uppercase().starts_with("EPSG:") {
                crs_str[5..]
                    .parse::<i32>()
                    .map_err(|_| ServiceError::InvalidBbox("Invalid CRS".to_string()))?
            } else {
                crs_str
                    .parse::<i32>()
                    .map_err(|_| ServiceError::InvalidBbox("Invalid CRS".to_string()))?
            };
            Some(srid)
        } else {
            None
        };

        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
            crs,
        })
    }

    /// Set the CRS
    pub fn with_crs(mut self, crs: i32) -> Self {
        self.crs = Some(crs);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_source_creation() {
        let source = DatabaseSource::new("postgresql://localhost/gis", "buildings");

        assert_eq!(source.table_name, "buildings");
        assert_eq!(source.geometry_column, "geom");
        assert!(matches!(source.database_type, DatabaseType::PostGis));
    }

    #[test]
    fn test_database_source_builder() {
        let source = DatabaseSource::new("postgresql://localhost/gis", "roads")
            .with_database_type(DatabaseType::PostGis)
            .with_geometry_column("the_geom")
            .with_id_column("gid")
            .with_srid(3857)
            .with_schema("public");

        assert_eq!(source.geometry_column, "the_geom");
        assert_eq!(source.id_column, Some("gid".to_string()));
        assert_eq!(source.srid, Some(3857));
        assert_eq!(source.schema, Some("public".to_string()));
    }

    #[test]
    fn test_qualified_table_name() {
        let source = DatabaseSource::new("postgresql://localhost/gis", "buildings");
        assert_eq!(source.qualified_table_name(), "\"buildings\"");

        let source_with_schema = source.with_schema("public");
        assert_eq!(
            source_with_schema.qualified_table_name(),
            "\"public\".\"buildings\""
        );
    }

    #[test]
    fn test_bbox_filter_from_string() {
        let bbox = BboxFilter::from_bbox_string("-180,-90,180,90");
        assert!(bbox.is_ok());

        let bbox = bbox.expect("bbox should parse");
        assert!((bbox.min_x - (-180.0)).abs() < f64::EPSILON);
        assert!((bbox.min_y - (-90.0)).abs() < f64::EPSILON);
        assert!((bbox.max_x - 180.0).abs() < f64::EPSILON);
        assert!((bbox.max_y - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_filter_with_crs() {
        let bbox = BboxFilter::from_bbox_string("-180,-90,180,90,EPSG:4326");
        assert!(bbox.is_ok());

        let bbox = bbox.expect("bbox should parse");
        assert_eq!(bbox.crs, Some(4326));
    }

    #[test]
    fn test_bbox_filter_invalid() {
        let bbox = BboxFilter::from_bbox_string("invalid");
        assert!(bbox.is_err());

        let bbox = BboxFilter::from_bbox_string("-180,-90,180");
        assert!(bbox.is_err());
    }

    #[test]
    fn test_cql_filter_simple() {
        let filter = CqlFilter::new("name = 'test'");
        let sql = filter.to_sql(&DatabaseType::PostGis);
        assert!(sql.is_ok());

        let sql = sql.expect("sql should parse");
        assert!(sql.contains("\"name\""));
        assert!(sql.contains("'test'"));
    }

    #[test]
    fn test_cql_filter_with_and() {
        let filter = CqlFilter::new("status = 'active' AND count > 10");
        let sql = filter.to_sql(&DatabaseType::PostGis);
        assert!(sql.is_ok());

        let sql = sql.expect("sql should parse");
        assert!(sql.contains("AND"));
    }

    #[test]
    fn test_cql_filter_mysql_quoting() {
        let filter = CqlFilter::new("name = 'test'");
        let sql = filter.to_sql(&DatabaseType::MySql);
        assert!(sql.is_ok());

        let sql = sql.expect("sql should parse");
        assert!(sql.contains("`name`"));
    }

    #[test]
    fn test_count_cache_config_default() {
        let config = CountCacheConfig::default();
        assert_eq!(config.ttl, Duration::from_secs(60));
        assert_eq!(config.max_entries, 100);
        assert_eq!(config.use_estimation_threshold, Some(1_000_000));
    }

    #[test]
    fn test_database_feature_counter_creation() {
        let counter = DatabaseFeatureCounter::new(CountCacheConfig::default());
        let stats = counter.cache_stats();
        assert_eq!(stats.total_entries, 0);
    }

    #[test]
    fn test_cache_stats() {
        let counter = DatabaseFeatureCounter::default();
        let stats = counter.cache_stats();

        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.valid_entries, 0);
        assert_eq!(stats.expired_entries, 0);
    }

    #[tokio::test]
    async fn test_get_count_returns_error_without_connection() {
        let counter = DatabaseFeatureCounter::default();
        let source = DatabaseSource::new("postgresql://localhost/gis", "buildings");

        let result = counter.get_count(&source, None, None).await;
        assert!(result.is_err());
    }
}
