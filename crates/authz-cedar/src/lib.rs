use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info};
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
    PolicySet, Request,
};
use std::str::FromStr;
use wasmtime::component::Resource;

/// Context data for authorization
#[derive(Debug, Clone)]
pub struct ContextData {
    policies: PolicySet,
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

const MESHX_AUTHZ_ID: &str = "meshx:authz@0.1.0-draft";

/// Cedar authorization engine plugin
#[derive(Clone, Default)]
pub struct AuthzEngine {
    authorizer: Authorizer,
}

// Implementation for the store interface
impl<'a> bindings::meshx::authz::engine::Host for ActiveCtx<'a> {
    async fn validate(
        &mut self,
        context: Resource<bindings::meshx::authz::engine::Context>,
        action: engine::Entity,
        resource: engine::Entity,
    ) -> anyhow::Result<Result<bool, EngineError>> {
        debug!("id= {} component_id= {}", self.id, self.component_id);

        let Some(plugin) = self.get_plugin::<AuthzEngine>(MESHX_AUTHZ_ID) else {
            return Ok(Err(EngineError::Other(
                "authz engine plugin not available".to_string(),
            )));
        };

        let p_eid = EntityId::from_str("alice")?;
        let p_name: EntityTypeName = EntityTypeName::from_str("User")?;
        let p = EntityUid::from_type_name_and_id(p_name, p_eid);

        let a_eid = EntityId::from_str("view")?;
        let a_name: EntityTypeName = EntityTypeName::from_str("Action")?;
        let a = EntityUid::from_type_name_and_id(a_name, a_eid);

        let r_eid = EntityId::from_str("trip")?;
        let r_name: EntityTypeName = EntityTypeName::from_str("Album")?;
        let r = EntityUid::from_type_name_and_id(r_name, r_eid);

        let c = Context::empty();

        let request = Request::new(p, a, r, c, None)?;

        // create entities
        let entities = vec![
            Entity::new(
                EntityUid::from_type_name_and_id(
                    EntityTypeName::from_str("User")?,
                    EntityId::from_str("alice")?,
                ),
                HashMap::new(),
                HashSet::new(),
            )?,
            Entity::new(
                EntityUid::from_type_name_and_id(
                    EntityTypeName::from_str("Action")?,
                    EntityId::from_str("view")?,
                ),
                HashMap::new(),
                HashSet::new(),
            )?,
            Entity::new(
                EntityUid::from_type_name_and_id(
                    EntityTypeName::from_str("Album")?,
                    EntityId::from_str("trip")?,
                ),
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
