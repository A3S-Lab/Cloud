pub mod commands;
pub mod queries;

mod plugin_assignment_operation_scheduler;
mod plugin_assignment_reconciler;
mod resource_access;

pub use commands::{
    ConfirmPluginPlanProjection, ConfirmPluginPlanProjectionHandler, EnrollPluginRegistry,
    EnrollPluginRegistryHandler, EnrollPluginRegistryResult, RecordPluginPlanProjection,
    RecordPluginPlanProjectionHandler, SetPluginAssignment, SetPluginAssignmentHandler,
    SetPluginAssignmentResult,
};
pub use plugin_assignment_operation_scheduler::{
    IPluginAssignmentOperationScheduler, PluginAssignmentOperationRequest,
    PluginAssignmentOperationScheduleOutcome,
};
pub use plugin_assignment_reconciler::{
    PLUGIN_ASSIGNMENT_WORKFLOW_NAME, PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
    PluginAssignmentReconcileReport, PluginAssignmentReconciler,
};
pub use queries::{
    GetPluginAssignment, GetPluginAssignmentHandler, GetPluginPlanProjection,
    GetPluginPlanProjectionHandler, GetPluginRegistry, GetPluginRegistryHandler,
    InspectCachedPluginCatalog, InspectCachedPluginCatalogHandler, InspectPluginCatalog,
    InspectPluginCatalogHandler, ListPluginAssignments, ListPluginAssignmentsHandler,
    ListPluginRegistries, ListPluginRegistriesHandler, SearchCachedPluginCatalog,
    SearchCachedPluginCatalogHandler, SearchPluginCatalog, SearchPluginCatalogHandler,
};

#[cfg(test)]
mod tests;

pub use resource_access::PluginAccess;
pub(crate) use resource_access::PluginAccessScope;
