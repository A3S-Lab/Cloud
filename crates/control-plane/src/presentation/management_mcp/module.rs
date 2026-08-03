use super::handler::ManagementMcpHandler;
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct ManagementMcpModule;

impl Module for ManagementMcpModule {
    fn name(&self) -> &'static str {
        "management-mcp"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let handler = Arc::new(ManagementMcpHandler::new(
            module_ref.get::<CommandBus>()?,
            module_ref.get::<QueryBus>()?,
        ));
        let post_handler = Arc::clone(&handler);
        Ok(vec![ControllerDefinition::new("")?
            .post("/mcp", move |request| {
                let handler = Arc::clone(&post_handler);
                async move { handler.handle(request).await }
            })?
            .get("/mcp", move |_request| async move {
                super::protocol::method_not_allowed_response()
            })?
            .delete("/mcp", move |_request| async move {
                super::protocol::method_not_allowed_response()
            })?
            .hide_from_openapi()])
    }
}
