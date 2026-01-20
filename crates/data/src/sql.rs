//! SQL parser with validation and safety checks
//!
//! Uses sqlparser-rs to parse SQL and validates that only safe operations are allowed.

use sqlparser::ast::{
    Expr, Function, Query, Select, SelectItem, SetExpr, Statement, TableFactor, TableWithJoins,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SqlError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Only single statements allowed")]
    MultipleStatements,

    #[error("Only SELECT queries allowed, got: {0}")]
    NotSelect(String),

    #[error("Table '{0}' not allowed")]
    TableNotAllowed(String),

    #[error("Functions are not allowed in queries")]
    FunctionsNotAllowed,

    #[error("Subqueries not allowed")]
    SubqueryNotAllowed,

    #[error("CTEs (WITH clause) not allowed")]
    CteNotAllowed,

    #[error("UNION/INTERSECT/EXCEPT not allowed")]
    SetOperationNotAllowed,

    #[error("Complex table expressions not allowed")]
    ComplexTableExpression,
}

/// Configuration for SQL validation
#[derive(Debug, Clone)]
pub struct SqlValidatorConfig {
    /// Tables that are allowed to be queried
    pub allowed_tables: HashSet<String>,
    /// Whether to allow any functions (default: false for security)
    pub allow_functions: bool,
    /// Maximum allowed LIMIT value
    pub max_limit: Option<u64>,
    /// Whether to allow subqueries
    pub allow_subqueries: bool,
    /// Whether to allow CTEs (WITH clause)
    pub allow_ctes: bool,
    /// Whether to allow UNION/INTERSECT/EXCEPT
    pub allow_set_operations: bool,
}

impl Default for SqlValidatorConfig {
    fn default() -> Self {
        Self {
            allowed_tables: HashSet::new(),
            allow_functions: false,
            max_limit: Some(1000),
            allow_subqueries: false,
            allow_ctes: false,
            allow_set_operations: false,
        }
    }
}

impl SqlValidatorConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_table(mut self, table: impl Into<String>) -> Self {
        self.allowed_tables.insert(table.into().to_lowercase());
        self
    }

    pub fn allow_tables(mut self, tables: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for table in tables {
            self.allowed_tables.insert(table.into().to_lowercase());
        }
        self
    }

    pub fn allow_functions(mut self, allow: bool) -> Self {
        self.allow_functions = allow;
        self
    }

    pub fn max_limit(mut self, limit: u64) -> Self {
        self.max_limit = Some(limit);
        self
    }

    pub fn allow_subqueries(mut self, allow: bool) -> Self {
        self.allow_subqueries = allow;
        self
    }

    pub fn allow_ctes(mut self, allow: bool) -> Self {
        self.allow_ctes = allow;
        self
    }

    pub fn allow_set_operations(mut self, allow: bool) -> Self {
        self.allow_set_operations = allow;
        self
    }
}

/// SQL parser that validates queries against a configuration
pub struct SqlValidator {
    config: SqlValidatorConfig,
}

impl SqlValidator {
    pub fn new(config: SqlValidatorConfig) -> Self {
        Self { config }
    }

    /// Parse and validate a SQL query string
    pub fn parse(&self, sql: &str) -> Result<Box<Query>, SqlError> {
        let dialect = PostgreSqlDialect {};
        let statements =
            Parser::parse_sql(&dialect, sql).map_err(|e| SqlError::Parse(e.to_string()))?;

        // Only allow single statement
        if statements.len() != 1 {
            return Err(SqlError::MultipleStatements);
        }

        let statement = statements.into_iter().next().unwrap();

        // Only allow SELECT
        let query = match statement {
            Statement::Query(q) => q,
            other => return Err(SqlError::NotSelect(format!("{:?}", other))),
        };

        // Validate the query
        self.validate_query(&query)?;

        Ok(query)
    }

    /// Validate a parsed query
    fn validate_query(&self, query: &Query) -> Result<(), SqlError> {
        // Check for CTEs
        if query.with.is_some() && !self.config.allow_ctes {
            return Err(SqlError::CteNotAllowed);
        }

        // Validate the body
        self.validate_set_expr(&query.body)?;

        // Validate LIMIT
        if let Some(ref limit_expr) = query.limit {
            self.validate_expr(limit_expr)?;
        }

        // Validate OFFSET
        if let Some(ref offset) = query.offset {
            self.validate_expr(&offset.value)?;
        }

        Ok(())
    }

    fn validate_set_expr(&self, set_expr: &SetExpr) -> Result<(), SqlError> {
        match set_expr {
            SetExpr::Select(select) => self.validate_select(select),
            SetExpr::Query(query) => self.validate_query(query),
            SetExpr::SetOperation { .. } if !self.config.allow_set_operations => {
                Err(SqlError::SetOperationNotAllowed)
            }
            SetExpr::SetOperation { left, right, .. } => {
                self.validate_set_expr(left)?;
                self.validate_set_expr(right)?;
                Ok(())
            }
            SetExpr::Values(_) => Ok(()),
            _ => Ok(()),
        }
    }

    fn validate_select(&self, select: &Select) -> Result<(), SqlError> {
        // Validate FROM clause (tables)
        for table_with_joins in &select.from {
            self.validate_table_with_joins(table_with_joins)?;
        }

        // Validate SELECT items (projections)
        for item in &select.projection {
            self.validate_select_item(item)?;
        }

        // Validate WHERE clause
        if let Some(ref selection) = select.selection {
            self.validate_expr(selection)?;
        }

        // Validate GROUP BY
        match &select.group_by {
            sqlparser::ast::GroupByExpr::Expressions(exprs, _) => {
                for expr in exprs {
                    self.validate_expr(expr)?;
                }
            }
            sqlparser::ast::GroupByExpr::All(_) => {}
        }

        // Validate HAVING
        if let Some(ref having) = select.having {
            self.validate_expr(having)?;
        }

        Ok(())
    }

    fn validate_table_with_joins(&self, twj: &TableWithJoins) -> Result<(), SqlError> {
        self.validate_table_factor(&twj.relation)?;

        for join in &twj.joins {
            self.validate_table_factor(&join.relation)?;
        }

        Ok(())
    }

    fn validate_table_factor(&self, table: &TableFactor) -> Result<(), SqlError> {
        match table {
            TableFactor::Table { name, .. } => {
                let table_name = name.to_string().to_lowercase();

                // If allowed_tables is empty, allow all tables
                // Otherwise, check if the table is in the allowlist
                if !self.config.allowed_tables.is_empty()
                    && !self.config.allowed_tables.contains(&table_name)
                {
                    return Err(SqlError::TableNotAllowed(table_name));
                }

                Ok(())
            }
            TableFactor::Derived { subquery, .. } => {
                if !self.config.allow_subqueries {
                    return Err(SqlError::SubqueryNotAllowed);
                }
                self.validate_query(subquery)
            }
            TableFactor::NestedJoin { table_with_joins, .. } => {
                self.validate_table_with_joins(table_with_joins)
            }
            _ => Err(SqlError::ComplexTableExpression),
        }
    }

    fn validate_select_item(&self, item: &SelectItem) -> Result<(), SqlError> {
        match item {
            SelectItem::UnnamedExpr(expr) => self.validate_expr(expr),
            SelectItem::ExprWithAlias { expr, .. } => self.validate_expr(expr),
            SelectItem::QualifiedWildcard(_, _) | SelectItem::Wildcard(_) => Ok(()),
        }
    }

    fn validate_expr(&self, expr: &Expr) -> Result<(), SqlError> {
        match expr {
            Expr::Function(func) => self.validate_function(func),
            Expr::Subquery(query) => {
                if !self.config.allow_subqueries {
                    return Err(SqlError::SubqueryNotAllowed);
                }
                self.validate_query(query)
            }
            Expr::BinaryOp { left, right, .. } => {
                self.validate_expr(left)?;
                self.validate_expr(right)?;
                Ok(())
            }
            Expr::UnaryOp { expr, .. } => self.validate_expr(expr),
            Expr::Between {
                expr, low, high, ..
            } => {
                self.validate_expr(expr)?;
                self.validate_expr(low)?;
                self.validate_expr(high)?;
                Ok(())
            }
            Expr::InList { expr, list, .. } => {
                self.validate_expr(expr)?;
                for item in list {
                    self.validate_expr(item)?;
                }
                Ok(())
            }
            Expr::InSubquery { expr, subquery, .. } => {
                self.validate_expr(expr)?;
                if !self.config.allow_subqueries {
                    return Err(SqlError::SubqueryNotAllowed);
                }
                self.validate_query(subquery)
            }
            Expr::Case {
                operand,
                conditions,
                results,
                else_result,
                ..
            } => {
                if let Some(op) = operand {
                    self.validate_expr(op)?;
                }
                for cond in conditions {
                    self.validate_expr(cond)?;
                }
                for res in results {
                    self.validate_expr(res)?;
                }
                if let Some(el) = else_result {
                    self.validate_expr(el)?;
                }
                Ok(())
            }
            Expr::Cast { expr, .. } => self.validate_expr(expr),
            Expr::Nested(inner) => self.validate_expr(inner),
            // Literals and identifiers are safe
            Expr::Value(_)
            | Expr::Identifier(_)
            | Expr::CompoundIdentifier(_)
            | Expr::Wildcard(_) => Ok(()),
            // Allow other expressions by default
            _ => Ok(()),
        }
    }

    fn validate_function(&self, _func: &Function) -> Result<(), SqlError> {
        // Block all functions by default for security
        if !self.config.allow_functions {
            return Err(SqlError::FunctionsNotAllowed);
        }
        Ok(())
    }
}

/// Extract all table names from a query
pub fn extract_tables(query: &Query) -> Vec<String> {
    let mut tables = Vec::new();
    extract_tables_from_set_expr(&query.body, &mut tables);
    tables
}

fn extract_tables_from_set_expr(set_expr: &SetExpr, tables: &mut Vec<String>) {
    match set_expr {
        SetExpr::Select(select) => {
            for twj in &select.from {
                extract_tables_from_table_factor(&twj.relation, tables);
                for join in &twj.joins {
                    extract_tables_from_table_factor(&join.relation, tables);
                }
            }
        }
        SetExpr::Query(query) => extract_tables_from_set_expr(&query.body, tables),
        SetExpr::SetOperation { left, right, .. } => {
            extract_tables_from_set_expr(left, tables);
            extract_tables_from_set_expr(right, tables);
        }
        _ => {}
    }
}

fn extract_tables_from_table_factor(table: &TableFactor, tables: &mut Vec<String>) {
    match table {
        TableFactor::Table { name, .. } => {
            tables.push(name.to_string());
        }
        TableFactor::Derived { subquery, .. } => {
            extract_tables_from_set_expr(&subquery.body, tables);
        }
        TableFactor::NestedJoin { table_with_joins, .. } => {
            extract_tables_from_table_factor(&table_with_joins.relation, tables);
            for join in &table_with_joins.joins {
                extract_tables_from_table_factor(&join.relation, tables);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validator_with_tables(tables: &[&str]) -> SqlValidator {
        let config = SqlValidatorConfig::new().allow_tables(tables.iter().map(|s| *s));
        SqlValidator::new(config)
    }

    #[test]
    fn test_simple_select() {
        let validator = validator_with_tables(&["users", "orders"]);
        let result = validator.parse("SELECT id, name FROM users");
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_where() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("SELECT * FROM users WHERE id = 1 AND status = 'active'");
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_join() {
        let validator = validator_with_tables(&["users", "orders"]);
        let result =
            validator.parse("SELECT u.name, o.total FROM users u JOIN orders o ON u.id = o.user_id");
        assert!(result.is_ok());
    }

    #[test]
    fn test_disallowed_table() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("SELECT * FROM secret_data");
        assert!(matches!(result, Err(SqlError::TableNotAllowed(_))));
    }

    #[test]
    fn test_functions_blocked_by_default() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("SELECT upper(name) FROM users");
        assert!(matches!(result, Err(SqlError::FunctionsNotAllowed)));
    }

    #[test]
    fn test_functions_allowed_when_enabled() {
        let config = SqlValidatorConfig::new()
            .allow_tables(["users"])
            .allow_functions(true);
        let validator = SqlValidator::new(config);
        let result = validator.parse("SELECT upper(name) FROM users");
        assert!(result.is_ok());
    }

    #[test]
    fn test_insert_rejected() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("INSERT INTO users (name) VALUES ('test')");
        assert!(matches!(result, Err(SqlError::NotSelect(_))));
    }

    #[test]
    fn test_delete_rejected() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("DELETE FROM users WHERE id = 1");
        assert!(matches!(result, Err(SqlError::NotSelect(_))));
    }

    #[test]
    fn test_drop_rejected() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("DROP TABLE users");
        assert!(matches!(result, Err(SqlError::NotSelect(_))));
    }

    #[test]
    fn test_subquery_rejected_by_default() {
        let validator = validator_with_tables(&["users", "orders"]);
        let result =
            validator.parse("SELECT * FROM users WHERE id IN (SELECT user_id FROM orders)");
        assert!(matches!(result, Err(SqlError::SubqueryNotAllowed)));
    }

    #[test]
    fn test_subquery_allowed_when_enabled() {
        let config = SqlValidatorConfig::new()
            .allow_tables(["users", "orders"])
            .allow_subqueries(true);
        let validator = SqlValidator::new(config);
        let result =
            validator.parse("SELECT * FROM users WHERE id IN (SELECT user_id FROM orders)");
        assert!(result.is_ok());
    }

    #[test]
    fn test_cte_rejected_by_default() {
        let validator = validator_with_tables(&["users"]);
        let result =
            validator.parse("WITH active_users AS (SELECT * FROM users) SELECT * FROM active_users");
        assert!(matches!(result, Err(SqlError::CteNotAllowed)));
    }

    #[test]
    fn test_multiple_statements_rejected() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("SELECT * FROM users; SELECT * FROM users");
        assert!(matches!(result, Err(SqlError::MultipleStatements)));
    }

    #[test]
    fn test_sql_injection_attempt() {
        let validator = validator_with_tables(&["users"]);
        let result = validator.parse("SELECT * FROM users; DROP TABLE users;--");
        assert!(matches!(result, Err(SqlError::MultipleStatements)));
    }

    #[test]
    fn test_extract_tables() {
        let validator = validator_with_tables(&["users", "orders", "products"]);
        let query = validator
            .parse("SELECT * FROM users u JOIN orders o ON u.id = o.user_id")
            .unwrap();
        let tables = extract_tables(&query);
        assert!(tables.contains(&"users".to_string()) || tables.contains(&"u".to_string()));
    }
}
