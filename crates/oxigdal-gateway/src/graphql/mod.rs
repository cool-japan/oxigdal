//! GraphQL module structure

pub mod mutations;
pub mod queries;
pub mod resolvers;
pub mod schema;
pub mod split;
pub mod subscriptions;

// Re-export from split
pub use split::*;
