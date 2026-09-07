mod plugin_assignment_repository;
mod plugin_plan_projection_repository;
mod plugin_registry_repository;

pub use plugin_assignment_repository::{
    CreatePluginAssignmentWrite, IPluginAssignmentRepository, UpdatePluginAssignmentWrite,
};
pub use plugin_plan_projection_repository::IPluginPlanProjectionRepository;
pub use plugin_registry_repository::{CreatePluginRegistryWrite, IPluginRegistryRepository};
