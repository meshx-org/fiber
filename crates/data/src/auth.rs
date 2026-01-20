//! Cedar-based authorization layer for query permissions
//!
//! Provides fine-grained access control using Cedar policies:
//! - Table-level permissions (can user read this table?)
//! - Column-level permissions (what columns can they see?)
//! - Row-level permissions (what rows can they access?)

use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request, RestrictedExpression,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Access denied")]
    Denied,

    #[error("Table '{0}' not allowed")]
    TableNotAllowed(String),

    #[error("Column '{0}' not allowed")]
    ColumnNotAllowed(String),

    #[error("Invalid entity: {0}")]
    InvalidEntity(String),

    #[error("Policy error: {0}")]
    PolicyError(String),

    #[error("Cedar error: {0}")]
    CedarError(String),
}

/// Actions that can be performed on resources
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Read,
    Write,
    Delete,
    ReadColumn,
    ReadRow,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Write => "write",
            Action::Delete => "delete",
            Action::ReadColumn => "read_column",
            Action::ReadRow => "read_row",
        }
    }

    pub fn to_entity_uid(&self) -> EntityUid {
        EntityUid::from_type_name_and_id(
            EntityTypeName::from_str("Action").unwrap(),
            EntityId::from_str(self.as_str()).unwrap(),
        )
    }
}

/// A user principal for authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub roles: Vec<String>,
    pub attributes: HashMap<String, String>,
}

impl Principal {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            roles: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    pub fn with_roles(mut self, roles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.roles.extend(roles.into_iter().map(|r| r.into()));
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub fn to_entity_uid(&self) -> EntityUid {
        EntityUid::from_type_name_and_id(
            EntityTypeName::from_str("User").unwrap(),
            EntityId::from_str(&self.id).unwrap(),
        )
    }

    /// Convert to Cedar entity with role memberships
    pub fn to_entity(&self) -> Entity {
        let uid = self.to_entity_uid();
        let parents: HashSet<EntityUid> = self
            .roles
            .iter()
            .map(|role| {
                EntityUid::from_type_name_and_id(
                    EntityTypeName::from_str("Role").unwrap(),
                    EntityId::from_str(role).unwrap(),
                )
            })
            .collect();

        let mut attrs = HashMap::new();
        attrs.insert(
            "id".to_string(),
            RestrictedExpression::new_string(self.id.clone()),
        );
        for (key, value) in &self.attributes {
            attrs.insert(
                key.clone(),
                RestrictedExpression::new_string(value.clone()),
            );
        }

        Entity::new(uid, attrs, parents).unwrap()
    }
}

/// Result of an authorization check
#[derive(Debug)]
pub struct AuthDecision {
    pub allowed: bool,
    pub allowed_columns: Option<HashSet<String>>,
    pub row_filter: Option<String>,
}

impl AuthDecision {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            allowed_columns: None,
            row_filter: None,
        }
    }

    pub fn deny() -> Self {
        Self {
            allowed: false,
            allowed_columns: None,
            row_filter: None,
        }
    }

    pub fn with_columns(mut self, columns: HashSet<String>) -> Self {
        self.allowed_columns = Some(columns);
        self
    }

    pub fn with_row_filter(mut self, filter: String) -> Self {
        self.row_filter = Some(filter);
        self
    }
}

/// Cedar-based authorization layer
pub struct CedarAuth {
    authorizer: Authorizer,
    policies: PolicySet,
}

impl CedarAuth {
    /// Create a new Cedar auth layer with the given policies
    pub fn new(policies: PolicySet) -> Self {
        Self {
            authorizer: Authorizer::new(),
            policies,
        }
    }

    /// Load policies from a Cedar policy string
    pub fn from_policy_str(policy_src: &str) -> Result<Self, AuthError> {
        let policies =
            PolicySet::from_str(policy_src).map_err(|e| AuthError::PolicyError(e.to_string()))?;
        Ok(Self::new(policies))
    }

    /// Check if an action is authorized
    pub fn is_authorized(
        &self,
        principal: &Principal,
        action: Action,
        resource_type: &str,
        resource_id: &str,
        entities: &Entities,
    ) -> Result<bool, AuthError> {
        let principal_uid = principal.to_entity_uid();
        let action_uid = action.to_entity_uid();
        let resource_uid = EntityUid::from_type_name_and_id(
            EntityTypeName::from_str(resource_type).map_err(|e| AuthError::InvalidEntity(e.to_string()))?,
            EntityId::from_str(resource_id).map_err(|e| AuthError::InvalidEntity(e.to_string()))?,
        );

        let request = Request::new(
            principal_uid,
            action_uid,
            resource_uid,
            Context::empty(),
            None,
        )
        .map_err(|e| AuthError::CedarError(e.to_string()))?;

        let response = self.authorizer.is_authorized(&request, &self.policies, entities);

        Ok(response.decision() == Decision::Allow)
    }

    /// Authorize a table read and return allowed columns
    pub fn authorize_table_read(
        &self,
        principal: &Principal,
        table: &str,
        requested_columns: &[String],
        entities: &Entities,
    ) -> Result<AuthDecision, AuthError> {
        // Check table-level access
        if !self.is_authorized(principal, Action::Read, "Table", table, entities)? {
            return Err(AuthError::TableNotAllowed(table.to_string()));
        }

        // Check column-level access
        let mut allowed_columns = HashSet::new();
        for col in requested_columns {
            let resource_id = format!("{}::{}", table, col);
            if self.is_authorized(principal, Action::ReadColumn, "Column", &resource_id, entities)? {
                allowed_columns.insert(col.clone());
            }
        }

        if allowed_columns.is_empty() && !requested_columns.is_empty() {
            return Err(AuthError::Denied);
        }

        Ok(AuthDecision::allow().with_columns(allowed_columns))
    }
}

/// Builder for Cedar entities
pub struct EntityBuilder {
    entities: Vec<Entity>,
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
        }
    }

    pub fn add_role(mut self, role_name: &str) -> Self {
        let uid = EntityUid::from_type_name_and_id(
            EntityTypeName::from_str("Role").unwrap(),
            EntityId::from_str(role_name).unwrap(),
        );
        self.entities
            .push(Entity::new_no_attrs(uid, HashSet::new()));
        self
    }

    pub fn add_table(mut self, table_name: &str) -> Self {
        let uid = EntityUid::from_type_name_and_id(
            EntityTypeName::from_str("Table").unwrap(),
            EntityId::from_str(table_name).unwrap(),
        );
        self.entities
            .push(Entity::new_no_attrs(uid, HashSet::new()));
        self
    }

    pub fn add_column(mut self, table_name: &str, column_name: &str) -> Self {
        let uid = EntityUid::from_type_name_and_id(
            EntityTypeName::from_str("Column").unwrap(),
            EntityId::from_str(&format!("{}::{}", table_name, column_name)).unwrap(),
        );
        // Column belongs to table
        let parent = EntityUid::from_type_name_and_id(
            EntityTypeName::from_str("Table").unwrap(),
            EntityId::from_str(table_name).unwrap(),
        );
        let mut parents = HashSet::new();
        parents.insert(parent);

        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(column_name.to_string()),
        );
        attrs.insert(
            "table".to_string(),
            RestrictedExpression::new_string(table_name.to_string()),
        );

        self.entities.push(Entity::new(uid, attrs, parents).unwrap());
        self
    }

    pub fn add_principal(mut self, principal: &Principal) -> Self {
        self.entities.push(principal.to_entity());
        self
    }

    pub fn add_action(mut self, action: Action) -> Self {
        let uid = action.to_entity_uid();
        self.entities
            .push(Entity::new_no_attrs(uid, HashSet::new()));
        self
    }

    pub fn build(self) -> Result<Entities, AuthError> {
        Entities::from_entities(self.entities, None)
            .map_err(|e| AuthError::CedarError(e.to_string()))
    }
}

impl Default for EntityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Example Cedar policies for a data query system
pub const EXAMPLE_POLICIES: &str = r#"
// Admin can do anything
permit(
    principal in Role::"admin",
    action,
    resource
);

// Users can read tables they have access to
permit(
    principal in Role::"user",
    action == Action::"read",
    resource is Table
) when {
    resource in principal.allowed_tables
};

// Users can read specific columns
permit(
    principal in Role::"user",
    action == Action::"read_column",
    resource is Column
) when {
    resource.table in principal.allowed_tables
};
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_basic_auth() -> (CedarAuth, Entities) {
        let policies = r#"
            permit(
                principal in Role::"admin",
                action,
                resource
            );

            permit(
                principal in Role::"chat_user",
                action == Action::"read",
                resource == Table::"chat_room"
            );

            permit(
                principal in Role::"chat_user",
                action == Action::"read_column",
                resource is Column
            ) when {
                resource.table == "chat_room"
            };
        "#;

        let auth = CedarAuth::from_policy_str(policies).unwrap();

        let principal = Principal::new("user_123").with_role("chat_user");

        let entities = EntityBuilder::new()
            .add_role("admin")
            .add_role("chat_user")
            .add_table("chat_room")
            .add_table("secret_table")
            .add_column("chat_room", "room_name")
            .add_column("chat_room", "type")
            .add_column("chat_room", "members")
            .add_column("secret_table", "secret_data")
            .add_action(Action::Read)
            .add_action(Action::ReadColumn)
            .add_principal(&principal)
            .build()
            .unwrap();

        (auth, entities)
    }

    #[test]
    fn test_table_access_allowed() {
        let (auth, entities) = setup_basic_auth();
        let principal = Principal::new("user_123").with_role("chat_user");

        let result = auth.is_authorized(&principal, Action::Read, "Table", "chat_room", &entities);
        assert!(result.unwrap());
    }

    #[test]
    fn test_table_access_denied() {
        let (auth, entities) = setup_basic_auth();
        let principal = Principal::new("user_123").with_role("chat_user");

        let result =
            auth.is_authorized(&principal, Action::Read, "Table", "secret_table", &entities);
        assert!(!result.unwrap());
    }

    #[test]
    fn test_admin_full_access() {
        let policies = r#"
            permit(
                principal in Role::"admin",
                action,
                resource
            );
        "#;

        let auth = CedarAuth::from_policy_str(policies).unwrap();
        let principal = Principal::new("admin_user").with_role("admin");

        let entities = EntityBuilder::new()
            .add_role("admin")
            .add_table("any_table")
            .add_action(Action::Read)
            .add_action(Action::Write)
            .add_action(Action::Delete)
            .add_principal(&principal)
            .build()
            .unwrap();

        assert!(auth
            .is_authorized(&principal, Action::Read, "Table", "any_table", &entities)
            .unwrap());
        assert!(auth
            .is_authorized(&principal, Action::Write, "Table", "any_table", &entities)
            .unwrap());
        assert!(auth
            .is_authorized(&principal, Action::Delete, "Table", "any_table", &entities)
            .unwrap());
    }

    #[test]
    fn test_column_access() {
        let (auth, entities) = setup_basic_auth();
        let principal = Principal::new("user_123").with_role("chat_user");

        let result = auth.is_authorized(
            &principal,
            Action::ReadColumn,
            "Column",
            "chat_room::room_name",
            &entities,
        );
        assert!(result.unwrap());
    }

    #[test]
    fn test_authorize_table_read() {
        let (auth, entities) = setup_basic_auth();
        let principal = Principal::new("user_123").with_role("chat_user");

        let columns = vec![
            "room_name".to_string(),
            "type".to_string(),
            "members".to_string(),
        ];

        let decision = auth
            .authorize_table_read(&principal, "chat_room", &columns, &entities)
            .unwrap();

        assert!(decision.allowed);
        assert!(decision.allowed_columns.is_some());
        let allowed = decision.allowed_columns.unwrap();
        assert!(allowed.contains("room_name"));
        assert!(allowed.contains("type"));
        assert!(allowed.contains("members"));
    }

    #[test]
    fn test_authorize_table_read_denied() {
        let (auth, entities) = setup_basic_auth();
        let principal = Principal::new("user_123").with_role("chat_user");

        let columns = vec!["secret_data".to_string()];

        let result = auth.authorize_table_read(&principal, "secret_table", &columns, &entities);
        assert!(matches!(result, Err(AuthError::TableNotAllowed(_))));
    }
}
