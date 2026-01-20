//! Query DSL types - equivalent to WIT interface definitions
//!
//! These types provide a safe, structured way to express queries
//! without allowing arbitrary SQL.

use serde::{Deserialize, Serialize};

/// Subquery configuration for nested/related data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subquery {
    pub limit: Option<u32>,
}

/// A segment in a field path - either a field name or a reference to follow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathSegment {
    /// A field name (e.g., "room_name")
    Field(String),
    /// A reference/foreign key to follow (like JOIN)
    Ref(u64),
}

/// SELECT clause - what fields to fetch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Select {
    /// Path to the field, can traverse relationships
    pub path: Vec<PathSegment>,
    /// Optional subquery for nested data
    pub subquery: Option<Subquery>,
}

/// Comparison operators for WHERE clauses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Ne,
    Like,
    ILike,
}

/// Value in a WHERE comparison - either a literal or a bound variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WhereValue {
    /// A bound variable (like $1 in prepared statements)
    Variable(u32),
    /// A literal value
    Literal(Value),
}

/// Recursive WHERE expression tree
/// Supports nested AND/OR conditions like: `(a = 1 AND b > 2) OR (c = 3)`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WhereExpr {
    /// Comparison: field op value (e.g., `age > 18`)
    Compare {
        field: Vec<String>,
        op: CompareOp,
        value: WhereValue,
    },
    /// IS NULL check
    IsNull(Vec<String>),
    /// IS NOT NULL check
    IsNotNull(Vec<String>),
    /// IN list check: field IN (values)
    In {
        field: Vec<String>,
        values: Vec<WhereValue>,
    },
    /// NOT IN list check: field NOT IN (values)
    NotIn {
        field: Vec<String>,
        values: Vec<WhereValue>,
    },
    /// AND combinator: all conditions must be true
    And(Vec<WhereExpr>),
    /// OR combinator: at least one condition must be true
    Or(Vec<WhereExpr>),
    /// NOT: negate the inner condition
    Not(Box<WhereExpr>),
}

/// Sort order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// ORDER BY clause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBy {
    pub path: Vec<String>,
    pub order: SortOrder,
}

/// Value types that can be stored/returned
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    None,
    Text(String),
    TextArray(Vec<String>),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Reference to nested data
    Nested(u64),
}

/// A row of values
pub type PartialRow = Vec<Value>;

/// Multiple rows
pub type PartialRows = Vec<PartialRow>;

/// Query result data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data {
    /// Main result rows
    pub rows: PartialRows,
    /// Nested/joined data keyed by reference ID
    pub nested: Vec<(u64, PartialRows)>,
}

/// A complete query request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub table: String,
    pub select: Vec<Select>,
    /// Optional WHERE clause (recursive expression tree)
    pub r#where: Option<WhereExpr>,
    pub order_by: Vec<OrderBy>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

impl Query {
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            select: Vec::new(),
            r#where: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    pub fn select(mut self, path: Vec<PathSegment>) -> Self {
        self.select.push(Select {
            path,
            subquery: None,
        });
        self
    }

    pub fn select_field(mut self, field: impl Into<String>) -> Self {
        self.select.push(Select {
            path: vec![PathSegment::Field(field.into())],
            subquery: None,
        });
        self
    }

    /// Set the WHERE clause (replaces any existing)
    pub fn filter(mut self, condition: WhereExpr) -> Self {
        self.r#where = Some(condition);
        self
    }

    /// Add a condition with AND (combines with existing WHERE)
    pub fn and_filter(mut self, condition: WhereExpr) -> Self {
        self.r#where = Some(match self.r#where {
            Some(existing) => WhereExpr::And(vec![existing, condition]),
            None => condition,
        });
        self
    }

    /// Add a condition with OR (combines with existing WHERE)
    pub fn or_filter(mut self, condition: WhereExpr) -> Self {
        self.r#where = Some(match self.r#where {
            Some(existing) => WhereExpr::Or(vec![existing, condition]),
            None => condition,
        });
        self
    }

    pub fn order_by(mut self, path: Vec<String>, order: SortOrder) -> Self {
        self.order_by.push(OrderBy { path, order });
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Helper to build WHERE expressions
pub struct Where;

impl Where {
    // Comparison with bound variable

    pub fn eq(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Eq,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn ne(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Ne,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn gt(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Gt,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn gte(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Gte,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn lt(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Lt,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn lte(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Lte,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn like(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Like,
            value: WhereValue::Variable(var_index),
        }
    }

    pub fn ilike(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::ILike,
            value: WhereValue::Variable(var_index),
        }
    }

    // Comparison with literal value

    pub fn eq_value(field: impl Into<String>, value: Value) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Eq,
            value: WhereValue::Literal(value),
        }
    }

    pub fn ne_value(field: impl Into<String>, value: Value) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Ne,
            value: WhereValue::Literal(value),
        }
    }

    pub fn gt_value(field: impl Into<String>, value: Value) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Gt,
            value: WhereValue::Literal(value),
        }
    }

    pub fn gte_value(field: impl Into<String>, value: Value) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Gte,
            value: WhereValue::Literal(value),
        }
    }

    pub fn lt_value(field: impl Into<String>, value: Value) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Lt,
            value: WhereValue::Literal(value),
        }
    }

    pub fn lte_value(field: impl Into<String>, value: Value) -> WhereExpr {
        WhereExpr::Compare {
            field: vec![field.into()],
            op: CompareOp::Lte,
            value: WhereValue::Literal(value),
        }
    }

    // Null checks

    pub fn is_null(field: impl Into<String>) -> WhereExpr {
        WhereExpr::IsNull(vec![field.into()])
    }

    pub fn is_not_null(field: impl Into<String>) -> WhereExpr {
        WhereExpr::IsNotNull(vec![field.into()])
    }

    // IN clauses

    pub fn r#in(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::In {
            field: vec![field.into()],
            values: vec![WhereValue::Variable(var_index)],
        }
    }

    pub fn in_values(field: impl Into<String>, values: Vec<Value>) -> WhereExpr {
        WhereExpr::In {
            field: vec![field.into()],
            values: values.into_iter().map(WhereValue::Literal).collect(),
        }
    }

    pub fn not_in(field: impl Into<String>, var_index: u32) -> WhereExpr {
        WhereExpr::NotIn {
            field: vec![field.into()],
            values: vec![WhereValue::Variable(var_index)],
        }
    }

    pub fn not_in_values(field: impl Into<String>, values: Vec<Value>) -> WhereExpr {
        WhereExpr::NotIn {
            field: vec![field.into()],
            values: values.into_iter().map(WhereValue::Literal).collect(),
        }
    }

    // Boolean combinators

    pub fn and(conditions: Vec<WhereExpr>) -> WhereExpr {
        WhereExpr::And(conditions)
    }

    pub fn or(conditions: Vec<WhereExpr>) -> WhereExpr {
        WhereExpr::Or(conditions)
    }

    pub fn not(condition: WhereExpr) -> WhereExpr {
        WhereExpr::Not(Box::new(condition))
    }
}

/// Backward compatibility alias
pub type WhereBuilder = Where;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_builder() {
        let query = Query::new("chat_room")
            .select_field("room_name")
            .select_field("type")
            .select_field("members")
            .filter(Where::eq("type", 0))
            .order_by(vec!["modified".into()], SortOrder::Desc)
            .limit(10);

        assert_eq!(query.table, "chat_room");
        assert_eq!(query.select.len(), 3);
        assert!(query.r#where.is_some());
        assert_eq!(query.limit, Some(10));
    }

    #[test]
    fn test_nested_where() {
        // (status = 'active' AND age > 18) OR (role = 'admin')
        let condition = Where::or(vec![
            Where::and(vec![
                Where::eq_value("status", Value::Text("active".into())),
                Where::gt_value("age", Value::Int(18)),
            ]),
            Where::eq_value("role", Value::Text("admin".into())),
        ]);

        let query = Query::new("users").filter(condition);

        assert!(query.r#where.is_some());
        match query.r#where.unwrap() {
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

    #[test]
    fn test_and_filter_chaining() {
        let query = Query::new("users")
            .filter(Where::eq("status", 0))
            .and_filter(Where::gt("age", 1))
            .and_filter(Where::is_not_null("email"));

        assert!(query.r#where.is_some());
        // Should create nested AND structure
    }
}
