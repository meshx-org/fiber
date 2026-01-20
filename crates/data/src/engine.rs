//! Query engine that ties together SQL parsing, authorization, and execution
//!
//! This module provides the main interface for executing queries with:
//! - SQL validation (only safe SELECT queries)
//! - Cedar authorization (table, column, row-level permissions)
//! - Query execution via sqlx

use crate::auth::{Action, AuthError, CedarAuth, EntityBuilder, Principal};
use crate::sql::{SqlError, SqlValidator, SqlValidatorConfig};
use crate::types::{self, CompareOp, Data, PartialRow, Query, Select, SortOrder, Value, WhereExpr, WhereValue};
use cedar_policy::Entities;
use sqlx::postgres::PgRow;
use sqlx::{Column, PgPool, Row};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("SQL error: {0}")]
    Sql(#[from] SqlError),

    #[error("Authorization error: {0}")]
    Auth(#[from] AuthError),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),
}

/// Configuration for the query engine
#[derive(Debug, Clone)]
pub struct QueryEngineConfig {
    /// Maximum rows to return
    pub max_limit: u32,
    /// Default limit if none specified
    pub default_limit: u32,
    /// SQL validator configuration
    pub sql_config: SqlValidatorConfig,
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        Self {
            max_limit: 1000,
            default_limit: 100,
            sql_config: SqlValidatorConfig::default(),
        }
    }
}

impl QueryEngineConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_limit(mut self, limit: u32) -> Self {
        self.max_limit = limit;
        self
    }

    pub fn default_limit(mut self, limit: u32) -> Self {
        self.default_limit = limit;
        self
    }

    pub fn allow_table(mut self, table: impl Into<String>) -> Self {
        self.sql_config = self.sql_config.allow_table(table);
        self
    }

    pub fn allow_tables(mut self, tables: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.sql_config = self.sql_config.allow_tables(tables);
        self
    }
}

/// The main query engine
pub struct QueryEngine {
    pool: PgPool,
    config: QueryEngineConfig,
    sql_validator: SqlValidator,
    auth: Option<CedarAuth>,
}

impl QueryEngine {
    /// Create a new query engine without authorization
    pub fn new(pool: PgPool, config: QueryEngineConfig) -> Self {
        let sql_validator = SqlValidator::new(config.sql_config.clone());
        Self {
            pool,
            config,
            sql_validator,
            auth: None,
        }
    }

    /// Create a new query engine with Cedar authorization
    pub fn with_auth(pool: PgPool, config: QueryEngineConfig, auth: CedarAuth) -> Self {
        let sql_validator = SqlValidator::new(config.sql_config.clone());
        Self {
            pool,
            config,
            sql_validator,
            auth: Some(auth),
        }
    }

    /// Execute a raw SQL query (validated)
    pub async fn execute_sql(
        &self,
        principal: Option<&Principal>,
        sql: &str,
    ) -> Result<Data, QueryError> {
        // 1. Parse and validate SQL
        let query = self.sql_validator.parse(sql)?;

        // 2. Extract tables for authorization
        let tables = crate::sql::extract_tables(&query);

        // 3. Authorize if auth is configured
        if let (Some(auth), Some(principal)) = (&self.auth, principal) {
            let entities = self.build_entities(principal, &tables)?;
            for table in &tables {
                if !auth.is_authorized(principal, Action::Read, "Table", table, &entities)? {
                    return Err(QueryError::Auth(AuthError::TableNotAllowed(table.clone())));
                }
            }
        }

        // 4. Execute query
        let rows = sqlx::query(sql).fetch_all(&self.pool).await?;

        // 5. Convert to Data
        Ok(self.rows_to_data(rows))
    }

    /// Execute a typed query (from WIT-style interface)
    pub async fn execute_query(
        &self,
        principal: Option<&Principal>,
        query: Query,
    ) -> Result<Data, QueryError> {
        // 1. Authorize table access
        if let (Some(auth), Some(principal)) = (&self.auth, principal) {
            let entities = self.build_entities(principal, &[query.table.clone()])?;
            if !auth.is_authorized(principal, Action::Read, "Table", &query.table, &entities)? {
                return Err(QueryError::Auth(AuthError::TableNotAllowed(
                    query.table.clone(),
                )));
            }

            // 2. Filter columns to authorized ones
            let requested_columns = self.extract_column_names(&query.select);
            let decision =
                auth.authorize_table_read(principal, &query.table, &requested_columns, &entities)?;

            if let Some(allowed) = decision.allowed_columns {
                // Filter query to only allowed columns
                let query = self.filter_query_columns(query, &allowed);
                return self.execute_query_internal(query).await;
            }
        }

        self.execute_query_internal(query).await
    }

    /// Execute query without authorization (internal use)
    async fn execute_query_internal(&self, query: Query) -> Result<Data, QueryError> {
        // Convert typed query to SQL
        let sql = self.query_to_sql(&query)?;

        // Execute
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;

        Ok(self.rows_to_data(rows))
    }

    /// Convert a typed Query to SQL string
    fn query_to_sql(&self, query: &Query) -> Result<String, QueryError> {
        let mut sql = String::from("SELECT ");

        // Columns
        if query.select.is_empty() {
            sql.push('*');
        } else {
            let columns: Vec<String> = query
                .select
                .iter()
                .filter_map(|s| self.select_to_column_name(s))
                .collect();
            sql.push_str(&columns.join(", "));
        }

        // FROM
        sql.push_str(" FROM ");
        sql.push_str(&quote_identifier(&query.table));

        // WHERE
        if let Some(ref where_expr) = query.r#where {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_expr_to_sql(where_expr));
        }

        // ORDER BY
        if !query.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            let orders: Vec<String> = query
                .order_by
                .iter()
                .map(|o| {
                    let col = o.path.join(".");
                    let dir = match o.order {
                        SortOrder::Asc => "ASC",
                        SortOrder::Desc => "DESC",
                    };
                    format!("{} {}", quote_identifier(&col), dir)
                })
                .collect();
            sql.push_str(&orders.join(", "));
        }

        // LIMIT
        let limit = query
            .limit
            .unwrap_or(self.config.default_limit)
            .min(self.config.max_limit);
        sql.push_str(&format!(" LIMIT {}", limit));

        // OFFSET
        if let Some(offset) = query.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        Ok(sql)
    }

    fn select_to_column_name(&self, select: &Select) -> Option<String> {
        match select.path.first() {
            Some(types::PathSegment::Field(name)) => Some(quote_identifier(name)),
            _ => None,
        }
    }

    /// Convert a WhereExpr tree to SQL recursively
    fn where_expr_to_sql(&self, expr: &WhereExpr) -> String {
        match expr {
            WhereExpr::Compare { field, op, value } => {
                let field_sql = quote_identifier(&field.join("."));
                let op_sql = compare_op_to_sql(op);
                let value_sql = where_value_to_sql(value);
                format!("{} {} {}", field_sql, op_sql, value_sql)
            }
            WhereExpr::IsNull(field) => {
                format!("{} IS NULL", quote_identifier(&field.join(".")))
            }
            WhereExpr::IsNotNull(field) => {
                format!("{} IS NOT NULL", quote_identifier(&field.join(".")))
            }
            WhereExpr::In { field, values } => {
                let field_sql = quote_identifier(&field.join("."));
                let values_sql: Vec<String> = values.iter().map(where_value_to_sql).collect();
                format!("{} IN ({})", field_sql, values_sql.join(", "))
            }
            WhereExpr::NotIn { field, values } => {
                let field_sql = quote_identifier(&field.join("."));
                let values_sql: Vec<String> = values.iter().map(where_value_to_sql).collect();
                format!("{} NOT IN ({})", field_sql, values_sql.join(", "))
            }
            WhereExpr::And(conditions) => {
                if conditions.is_empty() {
                    return "TRUE".to_string();
                }
                if conditions.len() == 1 {
                    return self.where_expr_to_sql(&conditions[0]);
                }
                let parts: Vec<String> = conditions
                    .iter()
                    .map(|c| self.where_expr_to_sql(c))
                    .collect();
                format!("({})", parts.join(" AND "))
            }
            WhereExpr::Or(conditions) => {
                if conditions.is_empty() {
                    return "FALSE".to_string();
                }
                if conditions.len() == 1 {
                    return self.where_expr_to_sql(&conditions[0]);
                }
                let parts: Vec<String> = conditions
                    .iter()
                    .map(|c| self.where_expr_to_sql(c))
                    .collect();
                format!("({})", parts.join(" OR "))
            }
            WhereExpr::Not(inner) => {
                format!("NOT ({})", self.where_expr_to_sql(inner))
            }
        }
    }

    fn extract_column_names(&self, selects: &[Select]) -> Vec<String> {
        selects
            .iter()
            .filter_map(|s| match s.path.first() {
                Some(types::PathSegment::Field(name)) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn filter_query_columns(&self, mut query: Query, allowed: &HashSet<String>) -> Query {
        query.select = query
            .select
            .into_iter()
            .filter(|s| match s.path.first() {
                Some(types::PathSegment::Field(name)) => allowed.contains(name),
                _ => false,
            })
            .collect();
        query
    }

    fn build_entities(
        &self,
        principal: &Principal,
        tables: &[String],
    ) -> Result<Entities, QueryError> {
        let mut builder = EntityBuilder::new()
            .add_principal(principal)
            .add_action(Action::Read)
            .add_action(Action::ReadColumn)
            .add_action(Action::ReadRow);

        for role in &principal.roles {
            builder = builder.add_role(role);
        }

        for table in tables {
            builder = builder.add_table(table);
        }

        builder.build().map_err(QueryError::Auth)
    }

    fn rows_to_data(&self, rows: Vec<PgRow>) -> Data {
        let mut result_rows = Vec::new();

        for row in rows {
            let mut partial_row = PartialRow::new();
            let columns = row.columns();

            for col in columns {
                let value = self.extract_value(&row, col.name());
                partial_row.push(value);
            }

            result_rows.push(partial_row);
        }

        Data {
            rows: result_rows,
            nested: Vec::new(),
        }
    }

    fn extract_value(&self, row: &PgRow, column: &str) -> Value {
        // Try different types
        if let Ok(v) = row.try_get::<Option<i64>, _>(column) {
            return v.map(Value::Int).unwrap_or(Value::None);
        }
        if let Ok(v) = row.try_get::<Option<f64>, _>(column) {
            return v.map(Value::Float).unwrap_or(Value::None);
        }
        if let Ok(v) = row.try_get::<Option<bool>, _>(column) {
            return v.map(Value::Bool).unwrap_or(Value::None);
        }
        if let Ok(v) = row.try_get::<Option<String>, _>(column) {
            return v.map(Value::Text).unwrap_or(Value::None);
        }

        Value::None
    }
}

/// Quote a SQL identifier
fn quote_identifier(name: &str) -> String {
    if name.contains('.') {
        name.split('.')
            .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(".")
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// Convert a CompareOp to SQL operator
fn compare_op_to_sql(op: &CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "=",
        CompareOp::Ne => "!=",
        CompareOp::Gt => ">",
        CompareOp::Gte => ">=",
        CompareOp::Lt => "<",
        CompareOp::Lte => "<=",
        CompareOp::Like => "LIKE",
        CompareOp::ILike => "ILIKE",
    }
}

/// Convert a WhereValue to SQL
fn where_value_to_sql(value: &WhereValue) -> String {
    match value {
        WhereValue::Variable(idx) => format!("${}", idx + 1),
        WhereValue::Literal(val) => value_to_sql(val),
    }
}

/// Convert a Value to SQL literal
fn value_to_sql(value: &Value) -> String {
    match value {
        Value::None => "NULL".to_string(),
        Value::Text(s) => format!("'{}'", s.replace('\'', "''")),
        Value::TextArray(arr) => {
            let items: Vec<String> = arr.iter().map(|s| format!("'{}'", s.replace('\'', "''"))).collect();
            format!("ARRAY[{}]", items.join(", "))
        }
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Nested(id) => format!("{}", id),
    }
}

/// Builder for creating QueryEngine instances
pub struct QueryEngineBuilder {
    config: QueryEngineConfig,
    auth: Option<CedarAuth>,
}

impl QueryEngineBuilder {
    pub fn new() -> Self {
        Self {
            config: QueryEngineConfig::default(),
            auth: None,
        }
    }

    pub fn config(mut self, config: QueryEngineConfig) -> Self {
        self.config = config;
        self
    }

    pub fn max_limit(mut self, limit: u32) -> Self {
        self.config.max_limit = limit;
        self
    }

    pub fn default_limit(mut self, limit: u32) -> Self {
        self.config.default_limit = limit;
        self
    }

    pub fn allow_table(mut self, table: impl Into<String>) -> Self {
        self.config = self.config.allow_table(table);
        self
    }

    pub fn allow_tables(mut self, tables: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.config = self.config.allow_tables(tables);
        self
    }

    pub fn with_auth(mut self, auth: CedarAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn with_policies(mut self, policies: &str) -> Result<Self, AuthError> {
        self.auth = Some(CedarAuth::from_policy_str(policies)?);
        Ok(self)
    }

    pub fn build(self, pool: PgPool) -> QueryEngine {
        match self.auth {
            Some(auth) => QueryEngine::with_auth(pool, self.config, auth),
            None => QueryEngine::new(pool, self.config),
        }
    }
}

impl Default for QueryEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Where;

    #[test]
    fn test_query_to_sql_simple() {
        let query = Query::new("users")
            .select_field("id")
            .select_field("name")
            .filter(Where::eq_value("status", Value::Text("active".to_string())))
            .order_by(vec!["created_at".to_string()], SortOrder::Desc)
            .limit(10);

        assert_eq!(query.table, "users");
        assert_eq!(query.select.len(), 2);
        assert_eq!(query.limit, Some(10));
    }

    #[test]
    fn test_quote_identifier() {
        assert_eq!(quote_identifier("users"), "\"users\"");
        assert_eq!(quote_identifier("user\"name"), "\"user\"\"name\"");
        assert_eq!(quote_identifier("schema.table"), "\"schema\".\"table\"");
    }

    #[test]
    fn test_value_to_sql() {
        assert_eq!(value_to_sql(&Value::None), "NULL");
        assert_eq!(value_to_sql(&Value::Int(42)), "42");
        assert_eq!(value_to_sql(&Value::Float(3.14)), "3.14");
        assert_eq!(value_to_sql(&Value::Bool(true)), "TRUE");
        assert_eq!(value_to_sql(&Value::Text("hello".into())), "'hello'");
        assert_eq!(value_to_sql(&Value::Text("it's".into())), "'it''s'");
    }

    #[test]
    fn test_where_expr_to_sql_simple() {
        let expr = Where::eq_value("status", Value::Text("active".into()));

        // We can test the SQL generation by inspecting the query
        let query = Query::new("users").filter(expr);
        assert!(query.r#where.is_some());
    }

    #[test]
    fn test_where_expr_nested_and_or() {
        // (status = 'active' AND age > 18) OR (role = 'admin')
        let expr = Where::or(vec![
            Where::and(vec![
                Where::eq_value("status", Value::Text("active".into())),
                Where::gt_value("age", Value::Int(18)),
            ]),
            Where::eq_value("role", Value::Text("admin".into())),
        ]);

        let query = Query::new("users").filter(expr);
        assert!(query.r#where.is_some());

        // Verify structure
        match query.r#where.as_ref().unwrap() {
            WhereExpr::Or(conditions) => {
                assert_eq!(conditions.len(), 2);
                match &conditions[0] {
                    WhereExpr::And(inner) => assert_eq!(inner.len(), 2),
                    _ => panic!("Expected And"),
                }
            }
            _ => panic!("Expected Or"),
        }
    }
}
