use super::controllers::{
    advertisement_controller, asset_commands_controller, asset_queries_controller,
    mcp_service_profile_commands_controller, mcp_service_profile_queries_controller,
    receive_pack_controller, upload_pack_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy)]
pub struct AssetsModule {
    maximum_rpc_body_bytes: usize,
}

impl AssetsModule {
    pub fn new(maximum_rpc_body_bytes: usize) -> Result<Self> {
        if maximum_rpc_body_bytes == 0 {
            return Err(a3s_boot::BootError::Internal(
                "Asset Git RPC body bound must be positive".into(),
            ));
        }
        Ok(Self {
            maximum_rpc_body_bytes,
        })
    }
}

impl Module for AssetsModule {
    fn name(&self) -> &'static str {
        "assets"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            asset_commands_controller(module_ref.get::<CommandBus>()?)?,
            asset_queries_controller(module_ref.get::<QueryBus>()?)?,
            mcp_service_profile_commands_controller(module_ref.get::<CommandBus>()?)?,
            mcp_service_profile_queries_controller(module_ref.get::<QueryBus>()?)?,
            advertisement_controller(module_ref.get::<QueryBus>()?)?,
            upload_pack_controller(module_ref.get::<QueryBus>()?, self.maximum_rpc_body_bytes)?,
            receive_pack_controller(module_ref.get::<CommandBus>()?, self.maximum_rpc_body_bytes)?,
        ])
    }
}
