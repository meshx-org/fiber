use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;
use wash_runtime::{
    engine::{
        ctx::{ActiveCtx, SharedCtx, extract_active_ctx},
        workload::WorkloadItem,
    },
    plugin::HostPlugin,
    wit::{WitInterface, WitWorld},
};

use bindings::meshx::authz::engine::{self, Error as EngineError};
use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request, RestrictedExpression,
};
use std::str::FromStr;
use wasmtime::component::Resource;

/// Context data for authorization
#[derive(Debug, Clone)]
pub struct ContextData {
    policies: PolicySet,
}

/// Per-request principal information parsed from headers
#[derive(Debug, Clone)]
pub struct RequestPrincipal {
    /// User subject identifier (from x-user-sub header)
    pub sub: String,
    /// User email (from x-user-email header)
    pub email: Option<String>,
    /// Client IP address from the connection
    pub ip_addr: std::net::IpAddr,
}

mod bindings {
    wasmtime::component::bindgen!({
        world: "authz",
        imports: { default: async | trappable },
        with: {
            "meshx:authz/engine/context": crate::ContextData,
        },
    });
}

pub const MESHX_AUTHZ_ID: &str = "meshx:authz@0.1.0-draft";

/// Cedar authorization engine plugin
#[derive(Clone, Default)]
pub struct AuthzEngine {
    authorizer: Authorizer,
    /// Per-request principal storage, keyed by active_ctx.id
    request_principals: Arc<RwLock<HashMap<String, RequestPrincipal>>>,
}

impl AuthzEngine {
    /// Set the principal for a request (call at start of request handling)
    pub async fn set_request_principal(&self, request_id: &str, principal: RequestPrincipal) {
        debug!(request_id = %request_id, sub = %principal.sub, "Setting request principal");
        self.request_principals
            .write()
            .await
            .insert(request_id.to_string(), principal);
    }

    /// Get the principal for a request
    pub async fn get_request_principal(&self, request_id: &str) -> Option<RequestPrincipal> {
        self.request_principals
            .read()
            .await
            .get(request_id)
            .cloned()
    }

    /// Clear the principal after request completes (call at end of request handling)
    pub async fn clear_request_principal(&self, request_id: &str) {
        debug!(request_id = %request_id, "Clearing request principal");
        self.request_principals.write().await.remove(request_id);
    }
}

// Implementation for the store interface
impl<'a> bindings::meshx::authz::engine::Host for ActiveCtx<'a> {
    async fn validate(
        &mut self,
        context: Resource<bindings::meshx::authz::engine::Context>,
        action: engine::Entity,
        resource: engine::Entity,
    ) -> anyhow::Result<Result<bool, EngineError>> {
        debug!(
            "id={} component_id={} action={}::{} resource={}::{}",
            self.id, self.component_id, action.type_, action.id, resource.type_, resource.id
        );

        let Some(plugin) = self.get_plugin::<AuthzEngine>(MESHX_AUTHZ_ID) else {
            return Ok(Err(EngineError::Other(
                "authz engine plugin not available".to_string(),
            )));
        };

        // Get the principal from the request context using self.id
        let Some(request_principal) = plugin.get_request_principal(&self.id).await else {
            return Ok(Err(EngineError::Other(
                "no principal context for request".to_string(),
            )));
        };

        // Build principal entity from the request context
        let p_eid = EntityId::from_str(&request_principal.sub)?;
        let p_name = EntityTypeName::from_str("User")?;
        let p = EntityUid::from_type_name_and_id(p_name, p_eid);

        // Build action entity from the provided action parameter
        let a_eid = EntityId::from_str(&action.id)?;
        let a_name = EntityTypeName::from_str(&action.type_)?;
        let a = EntityUid::from_type_name_and_id(a_name, a_eid);

        // Build resource entity from the provided resource parameter
        let r_eid = EntityId::from_str(&resource.id)?;
        let r_name = EntityTypeName::from_str(&resource.type_)?;
        let r = EntityUid::from_type_name_and_id(r_name, r_eid);

        let c = Context::empty();

        let request = Request::new(p.clone(), a.clone(), r.clone(), c, None)?;

        // Build attributes for the principal entity
        let mut principal_attrs = HashMap::new();
        // Add IP address as a Cedar extension value
        let ip_expr = RestrictedExpression::new_ip(request_principal.ip_addr.to_string());
        principal_attrs.insert("ip_addr".to_string(), ip_expr);

        // Create entities for the authorization request
        let entities = vec![
            Entity::new(
                p, // Principal entity with attributes
                principal_attrs,
                HashSet::new(),
            )?,
            Entity::new(
                a, // Action entity from request
                HashMap::new(),
                HashSet::new(),
            )?,
            Entity::new(
                r, // Resource entity from request
                HashMap::new(),
                HashSet::new(),
            )?,
        ];

        let context_data = self.table.get::<ContextData>(&context)?;

        let entities = Entities::from_entities(entities, None).expect("entity error");
        let response = plugin
            .authorizer
            .is_authorized(&request, &context_data.policies, &entities);

        match response.decision() {
            Decision::Allow => Ok(Ok(true)),
            Decision::Deny => Ok(Err(EngineError::Forbidden)),
        }
    }
}

// Resource host trait implementations for context
impl<'a> bindings::meshx::authz::engine::HostContext for ActiveCtx<'a> {
    async fn init(&mut self) -> anyhow::Result<Resource<ContextData>> {
        // create a policy
        let s = r#"
permit (
  principal == User::"alice",
  action == Action::"view",
  resource == Album::"trip"
)
when { principal.ip_addr.isIpv4() };
"#;
        let policies = PolicySet::from_str(s).expect("policy error");

        let context_data = ContextData { policies };
        let resource = self.table.push(context_data)?;
        Ok(resource)
    }

    async fn drop(&mut self, rep: Resource<ContextData>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl HostPlugin for AuthzEngine {
    fn id(&self) -> &'static str {
        MESHX_AUTHZ_ID
    }

    fn world(&self) -> WitWorld {
        WitWorld {
            imports: HashSet::from([WitInterface::from("meshx:authz/engine@0.1.0-draft")]),
            ..Default::default()
        }
    }

    async fn on_workload_item_bind<'a>(
        &self,
        component_handle: &mut WorkloadItem<'a>,
        interfaces: HashSet<WitInterface>,
    ) -> anyhow::Result<()> {
        // Check if any of the interfaces are meshx:authz related
        let has_authz = interfaces
            .iter()
            .any(|i| i.namespace == "meshx" && i.package == "authz");

        if !has_authz {
            tracing::warn!(
                "Authz engine plugin requested for non-meshx:authz interface(s): {:?}",
                interfaces
            );
            return Ok(());
        }

        // Add authz interfaces to the workload's linker
        tracing::debug!(
            workload_id = component_handle.id(),
            "Adding authz interfaces to linker for workload"
        );
        let linker = component_handle.linker();

        bindings::meshx::authz::engine::add_to_linker::<_, SharedCtx>(linker, extract_active_ctx)?;

        let id = component_handle.workload_id();

        tracing::debug!("Authz engine bound to workload '{id}'");
        Ok(())
    }

    async fn on_workload_unbind(
        &self,
        workload_id: &str,
        _interfaces: HashSet<WitInterface>,
    ) -> anyhow::Result<()> {
        tracing::debug!("Authz engine unbound from workload '{workload_id}'");
        Ok(())
    }
}
