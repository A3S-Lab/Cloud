use super::controllers::{
    environment_queries_controller, environments_controller, project_queries_controller,
    projects_controller,
};
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectsModule;

impl Module for ProjectsModule {
    fn name(&self) -> &'static str {
        "projects"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let bus = module_ref.get::<CommandBus>()?;
        Ok(vec![
            projects_controller(bus.clone())?,
            environments_controller(bus)?,
            project_queries_controller(module_ref.get::<QueryBus>()?)?,
            environment_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
