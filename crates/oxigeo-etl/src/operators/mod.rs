//! Transformation operators for ETL pipelines
//!
//! This module provides specialized operators for common ETL transformations:
//!
//! - **Map**: Element-wise transformations
//! - **Filter**: Conditional filtering
//! - **Window**: Sliding/tumbling windows
//! - **Join**: Stream-to-stream joins
//! - **Aggregate**: Stream aggregation and statistics

pub mod aggregate;
pub mod filter;
pub mod join;
pub mod map;
pub mod window;

pub use aggregate::{AggregateFunctions, AggregateOperator};
pub use filter::{FilterOperator, GeoFilterOperator};
pub use join::{JoinFunctions, JoinOperator, JoinType, JsonFieldExtractor};
pub use map::{CompressionType, GeoMapOperator, MapOperator};
pub use window::{WindowAggregator, WindowOperator, WindowType};
