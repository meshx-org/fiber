use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, bail};
use tokio::sync::RwLock;
use tracing::{debug, instrument, warn};
use wash_runtime::engine::ctx::{ActiveCtx, SharedCtx, extract_active_ctx};
use wash_runtime::engine::workload::{ResolvedWorkload, WorkloadItem};
use wash_runtime::plugin::HostPlugin;
use wash_runtime::wit::{WitInterface, WitWorld};

use wash_runtime::plugin::WorkloadTracker;

mod bindings {
    wasmtime::component::bindgen!({
        world: "data",
        imports: { default: async | trappable },
        exports: { default: async },
    });
}

use bindings::meshx::data::types;

pub const MESHX_DATA_ID: &str = "meshx:data@0.1.0-draft";

#[derive(Clone, Default)]
pub struct DatastorePlugin {
    tracker: Arc<RwLock<WorkloadTracker<(), ()>>>,
}

impl<'a> types::Host for ActiveCtx<'a> {}

#[async_trait::async_trait]
impl HostPlugin for DatastorePlugin {
    fn id(&self) -> &'static str {
        MESHX_DATA_ID
    }

    fn world(&self) -> WitWorld {
        WitWorld {
            exports: HashSet::from([WitInterface::from("meshx:data/schema@0.1.0-draft")]),
            ..Default::default()
        }
    }

    async fn on_workload_item_bind<'a>(
        &self,
        component_handle: &mut WorkloadItem<'a>,
        interfaces: HashSet<WitInterface>,
    ) -> anyhow::Result<()> {
        let Some(interface) = interfaces
            .iter()
            .find(|i| i.namespace == "meshx" && i.package == "data")
        else {
            return Ok(());
        };

        bindings::meshx::data::types::add_to_linker::<_, SharedCtx>(
            component_handle.linker(),
            extract_active_ctx,
        )?;

        if interface.interfaces.iter().any(|i| i == "schema") {
            let WorkloadItem::Component(component_handle) = component_handle else {
                bail!("Service can not be tracked");
            };
        }

        Ok(())
    }

    async fn on_workload_resolved(
        &self,
        workload: &ResolvedWorkload,
        component_id: &str,
    ) -> anyhow::Result<()> {
        let instance_pre = workload.instantiate_pre(component_id).await?;

        let pre =
            bindings::DataPre::new(instance_pre).context("failed to instantiate messaging pre")?;

        let workload = workload.clone();
        let component_id = component_id.to_string();

        let mut store = match workload.new_store(&component_id).await {
            Err(e) => {
                bail!("failed to create store for component {component_id}: {e}");
            }
            Ok(s) => s,
        };

        let proxy = match pre.instantiate_async(&mut store).await {
            Err(e) => {
                bail!("failed to instantiate component {component_id}: {e}");
            }
            Ok(p) => p,
        };

        match proxy.meshx_data_schema().call_get(store).await {
            Ok(schema) => {
                debug!(schema = ?schema, "Schema retrieved successfully");
            }
            Err(e) => {
                warn!("Error handling message: {e}");
            }
        }

        Ok(())
    }

    async fn on_workload_unbind(
        &self,
        workload_id: &str,
        _interfaces: HashSet<WitInterface>,
    ) -> anyhow::Result<()> {
        tracing::debug!("Datastore plugin unbound from workload '{workload_id}'");
        Ok(())
    }
}
