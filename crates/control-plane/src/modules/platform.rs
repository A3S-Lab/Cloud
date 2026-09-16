use crate::config::{CloudConfig, ProcessRole};
use a3s_boot::{
    controller, get, metadata, ControllerDefinition, Module, ModuleRef, Result,
    AUTH_PUBLIC_METADATA,
};
use serde::Serialize;
use std::sync::Arc;

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
                ProcessRole::Delivery => "delivery",
                ProcessRole::Worker => "worker",
                ProcessRole::Relay => "relay",
            },
        };
        Ok(vec![
            Arc::new(PlatformInfoController { response }).controller()?,
        ])
    }
}

#[derive(Debug, Clone, Serialize)]
struct PlatformResponse {
    name: &'static str,
    version: &'static str,
    role: &'static str,
}

#[derive(Debug, Clone)]
struct PlatformInfoController {
    response: PlatformResponse,
}

#[controller("/platform")]
#[metadata("auth.public", true)]
impl PlatformInfoController {
    #[get("/")]
    async fn info(&self) -> Result<PlatformResponse> {
        Ok(self.response.clone())
    }
}

#[cfg(test)]
mod nest_macro_platform_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn platform_info_controller_registers_public_get_root_via_nest_macros() {
        let controller = Arc::new(PlatformInfoController {
            response: PlatformResponse {
                name: "a3s-cloud",
                version: "0.0.0",
                role: "api",
            },
        })
        .controller()
        .expect("platform nest controller");

        assert_eq!(controller.prefix(), "/platform");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        // ControllerDefinition applies the prefix onto each route path.
        assert_eq!(routes[0].path(), "/platform");
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_PUBLIC_METADATA)
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }
}
