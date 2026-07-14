use crate::config::{CloudConfig, ProcessRole};
use a3s_boot::{
    BootResponse, ControllerDefinition, Module, ModuleRef, Result, AUTH_PUBLIC_METADATA,
};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct PlatformModule {
    role: ProcessRole,
}

impl PlatformModule {
    pub fn new(config: &CloudConfig) -> Self {
        Self {
            role: config.server.role,
        }
    }
}

impl Module for PlatformModule {
    fn name(&self) -> &'static str {
        "platform"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let response = PlatformResponse {
            name: "a3s-cloud",
            version: env!("CARGO_PKG_VERSION"),
            role: match self.role {
                ProcessRole::All => "all",
                ProcessRole::Api => "api",
                ProcessRole::Worker => "worker",
                ProcessRole::Relay => "relay",
            },
        };
        Ok(vec![ControllerDefinition::new("/platform")?
            .with_metadata(AUTH_PUBLIC_METADATA, true)?
            .get("/", move |_| {
                let response = response.clone();
                async move { BootResponse::json(&response) }
            })?])
    }
}

#[derive(Debug, Clone, Serialize)]
struct PlatformResponse {
    name: &'static str,
    version: &'static str,
    role: &'static str,
}
