# oxigdal-query

[![Crates.io](https://img.shields.io/crates/v/oxigdal-query.svg)](https://crates.io/crates/oxigdal-query)
[![Documentation](https://docs.rs/oxigdal-query/badge.svg)](https://docs.rs/oxigdal-query)
[![License](https://img.shields.io/crates/l/oxigdal-query.svg)](LICENSE)

A high-performance SQL-like query language and cost-based optimizer for geospatial data processing. This crate provides a complete query engine with parsing, optimization, parallel execution, and result caching designed for efficient data filtering and transformation.

## Features

- **SQL-like Query Language**: Parse and execute SQL queries with full support for SELECT, WHERE, JOIN, GROUP BY, ORDER BY, LIMIT, and OFFSET clauses
- **Cost-Based Query Optimizer**: Intelligent query optimization using cost models for join reordering, predicate pushdown, and constant folding
- **Parallel Query Execution**: Built-in support for parallelized query execution using Rayon with efficient data batching
- **Result Caching**: Configurable query result caching with TTL and size limits using DashMap for concurrent access
- **Index Selection**: Automatic index selection for optimized query execution
- **Geospatial Support**: Full support for spatial predicates including ST_Intersects and other PostGIS-compatible functions
- **Async/Await**: Non-blocking asynchronous query execution with Tokio integration
- **Error Handling**: Comprehensive error types with detailed position information for parsing errors
- **Pure Rust**: 100% Pure Rust implementation with no C/Fortran dependencies
- **Performance**: Benchmarked query parser, optimizer, and executor for baseline performance metrics

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-query = "0.1.2"
oxigdal-core = "0.1.2"
tokio = { version = "1", features = ["full"] }
```

## Quick Start

### Basic Query Parsing and Optimization

```rust
use oxigdal_query::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse SQL query
    let sql = "SELECT id, name FROM users WHERE age > 18";
    let statement = parser::sql::parse_sql(sql)?;

    // Optimize query
    let optimizer = optimizer::Optimizer::new();
    let optimized = optimizer.optimize(statement)?;

    println!("Original cost: {}", optimized.original_cost.total());
    println!("Optimized cost: {}", optimized.optimized_cost.total());
    Ok(())
}
```

### Query Engine with Execution

```rust
use oxigdal_query::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create query engine
    let mut engine = QueryEngine::new();

    // Register data sources
    // (Implement DataSource trait for your data)
    // engine.register_data_source("users".to_string(), your_data_source);

    // Execute SQL query
    let sql = "SELECT id, name FROM users WHERE age > 18 ORDER BY name LIMIT 10";
    let results = engine.execute_sql(sql).await?;

    println!("Results: {} rows", results.iter().map(|b| b.num_rows).sum::<usize>());
    Ok(())
}
```

### Query Explanation

```rust
use oxigdal_query::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = QueryEngine::new();

    // Get query execution plan
    let sql = "SELECT COUNT(*), AVG(age) FROM users GROUP BY country";
    let explain = engine.explain_sql(sql)?;

    println!("Query Plan:");
    for node in &explain.nodes {
        println!("  - {}: {}", node.node_type, node.description);
    }
    println!("Total Cost: {}", explain.total_cost.total());
    Ok(())
}
```

## Usage

### Basic Query Execution

```rust
use oxigdal_query::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = QueryEngine::new();

    // Parse and execute simple query
    let results = engine.execute_sql("SELECT * FROM users").await?;
    Ok(())
}
```

### With Custom Optimizer Configuration

```rust
use oxigdal_query::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create optimizer with custom config
    let config = OptimizerConfig {
        max_passes: 5,
        enable_predicate_pushdown: true,
        enable_join_reordering: true,
        enable_constant_folding: true,
        enable_cse: true,
        enable_filter_fusion: true,
        enable_projection_pushdown: true,
    };

    let optimizer = Optimizer::with_config(config);
    let sql = "SELECT * FROM users WHERE age > 18 AND status = 'active'";
    let statement = parser::sql::parse_sql(sql)?;

    let optimized = optimizer.optimize(statement)?;
    println!("Cost reduction: {:.2}%",
        (1.0 - optimized.optimized_cost.total() / optimized.original_cost.total()) * 100.0);
    Ok(())
}
```

### Query Result Caching

```rust
use oxigdal_query::*;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create cache with custom config
    let cache_config = CacheConfig {
        max_size_bytes: 2 * 1024 * 1024 * 1024, // 2 GB
        ttl: Duration::from_secs(600), // 10 minutes
        enabled: true,
    };

    let optimizer_config = OptimizerConfig::default();
    let mut engine = QueryEngine::with_config(optimizer_config, cache_config);

    // First execution - cache miss
    let sql = "SELECT COUNT(*) FROM large_table";
    let result1 = engine.execute_sql(sql).await?;

    // Second execution - cache hit
    let result2 = engine.execute_sql(sql).await?;

    let stats = engine.cache_statistics();
    println!("Cache hits: {}", stats.hits);
    println!("Cache misses: {}", stats.misses);
    Ok(())
}
```

### Geospatial Queries

```rust
use oxigdal_query::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sql = "SELECT geom, name FROM buildings WHERE ST_Intersects(geom, ST_MakeEnvelope(0, 0, 100, 100))";
    let statement = parser::sql::parse_sql(sql)?;

    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(statement)?;
    println!("Spatial query optimized successfully");
    Ok(())
}
```

## API Overview

| Module | Description |
|--------|-------------|
| `parser` | SQL query parsing with AST generation - supports standard SQL SELECT statements |
| `optimizer` | Cost-based query optimization with configurable rules and heuristics |
| `executor` | Query execution engine with support for scans, filters, joins, aggregations, and sorting |
| `cache` | Query result caching with TTL and size management |
| `index` | Index selection and management for optimized data access |
| `explain` | Query plan explanation and visualization with cost estimates |
| `parallel` | Parallel execution utilities for distributed query processing |
| `error` | Comprehensive error types with detailed diagnostics |

## Optimization Rules

The optimizer implements the following optimization strategies:

- **Predicate Pushdown**: Push filter predicates down to reduce data earlier
- **Join Reordering**: Reorder joins for optimal execution
- **Constant Folding**: Pre-compute constant expressions
- **Common Subexpression Elimination**: Eliminate redundant computations
- **Filter Fusion**: Combine multiple filters into single operations
- **Projection Pushdown**: Push column selections down the tree

## Performance

Benchmarks on standard hardware demonstrate:

| Operation | Dataset Size | Time |
|-----------|--------------|------|
| Parse simple SELECT | N/A | ~10 µs |
| Parse complex query | N/A | ~50 µs |
| Optimize query | N/A | ~100 µs |
| Full table scan | 100k rows | ~1 ms |
| Filtered scan | 100k rows | ~2 ms |
| Aggregate query | 100k rows | ~5 ms |

Run benchmarks locally:

```bash
cargo bench --bench query_bench
```

## Examples

See the [tests](tests/) directory for comprehensive examples:

- `parser_test.rs` - SQL parsing examples (simple select, joins, aggregates, spatial queries)
- `executor_test.rs` - Query execution patterns
- `optimizer_test.rs` - Optimization strategies
- `end_to_end_test.rs` - Complete workflows

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, QueryError>` with specific error variants:

```rust
use oxigdal_query::*;

fn main() {
    let sql = "INVALID SQL";
    match parser::sql::parse_sql(sql) {
        Ok(statement) => println!("Parsed: {:?}", statement),
        Err(QueryError::ParseError { message, line, column }) => {
            eprintln!("Parse error at {}:{}: {}", line, column, message);
        }
        Err(QueryError::SemanticError(msg)) => eprintln!("Semantic error: {}", msg),
        Err(QueryError::ExecutionError(msg)) => eprintln!("Execution error: {}", msg),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

## Documentation

Full documentation is available at [docs.rs](https://docs.rs/oxigdal-query).

Generate and view local documentation:

```bash
cargo doc --open
```

## Pure Rust

This library is 100% Pure Rust with no C/Fortran dependencies. All functionality works out of the box without external system libraries.

## Related Projects

- [oxigdal-core](https://github.com/cool-japan/oxigdal) - Core geospatial data structures and types
- [oxigdal-algorithms](https://github.com/cool-japan/oxigdal) - Spatial algorithms and operations
- [oxigdal-drivers](https://github.com/cool-japan/oxigdal) - Data source drivers (GeoTIFF, GeoJSON, Parquet, etc.)

## Contributing

Contributions are welcome! Please ensure:

- No use of `unwrap()` - use `Result<T, E>` instead
- All tests pass: `cargo test --all-features`
- No clippy warnings: `cargo clippy`
- Code follows COOLJAPAN ecosystem policies

## License

Licensed under Apache-2.0.

See [LICENSE](LICENSE) file for details.

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem for high-performance geospatial and scientific computing in Pure Rust.
