//! Data query engine with SQL validation and Cedar authorization
//!
//! This crate provides a secure query interface for database access:
//!
//! - **types**: WIT-style query DSL types for safe, structured queries
//! - **sql**: SQL parser with validation (only SELECT, blocked dangerous functions)
//! - **auth**: Cedar-based authorization (table, column, row-level permissions)
//! - **engine**: Query engine that ties everything together
//!
//! # Example
//!
//! ```rust,ignore
//! use data::{QueryEngineBuilder, Principal, Query, WhereBuilder, SortOrder};
//!
//! // Create engine with authorization
//! let engine = QueryEngineBuilder::new()
//!     .allow_tables(["users", "orders"])
//!     .max_limit(100)
//!     .with_policies(r#"
//!         permit(
//!             principal in Role::"user",
//!             action == Action::"read",
//!             resource is Table
//!         );
//!     "#)?
//!     .build(pool);
//!
//! // Execute a typed query
//! let principal = Principal::new("user_123").with_role("user");
//! let query = Query::new("users")
//!     .select_field("id")
//!     .select_field("name")
//!     .filter(WhereBuilder::eq("status", 0))
//!     .order_by(vec!["created_at".into()], SortOrder::Desc)
//!     .limit(10);
//!
//! let result = engine.execute_query(Some(&principal), query).await?;
//!
//! // Or execute raw SQL (validated)
//! let result = engine.execute_sql(
//!     Some(&principal),
//!     "SELECT id, name FROM users WHERE status = 'active'"
//! ).await?;
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │  Client (WASM component or API)         │
//! │  Query { table, select, where, ... }    │
//! └──────────────┬──────────────────────────┘
//!                │
//! ┌──────────────▼──────────────────────────┐
//! │  Query Engine                           │
//! │  - SQL validation (sqlparser-rs)        │
//! │  - Cedar authorization                  │
//! │  - Limit enforcement                    │
//! └──────────────┬──────────────────────────┘
//!                │
//! ┌──────────────▼──────────────────────────┐
//! │  Database (PostgreSQL via sqlx)         │
//! └─────────────────────────────────────────┘
//! ```

pub mod auth;
pub mod engine;
pub mod sql;
pub mod types;

// Re-export main types for convenience
pub use auth::{Action, AuthDecision, AuthError, CedarAuth, EntityBuilder, Principal};
pub use engine::{QueryEngine, QueryEngineBuilder, QueryEngineConfig, QueryError};
pub use sql::{SqlError, SqlValidator, SqlValidatorConfig};
pub use types::{
    CompareOp, Data, OrderBy, PartialRow, PartialRows, PathSegment, Query, Select, SortOrder,
    Subquery, Value, Where, WhereBuilder, WhereExpr, WhereValue,
};
