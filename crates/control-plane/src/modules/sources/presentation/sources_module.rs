use super::controllers::{source_revision_queries_controller, source_revisions_controller};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct SourcesModule;

impl Module for SourcesModule {
    fn name(&self) -> &'static str {
        "sources"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            source_revisions_controller(module_ref.get::<CommandBus>()?)?,
            source_revision_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
