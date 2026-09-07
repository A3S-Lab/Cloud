pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

#[cfg(test)]
pub(crate) mod test_support;

pub use application::{
    ConfirmPluginPlanProjection, ConfirmPluginPlanProjectionHandler, EnrollPluginRegistry,
    EnrollPluginRegistryHandler, EnrollPluginRegistryResult, GetPluginAssignment,
    GetPluginAssignmentHandler, GetPluginPlanProjection, GetPluginPlanProjectionHandler,
    GetPluginRegistry, GetPluginRegistryHandler, InspectCachedPluginCatalog,
    InspectCachedPluginCatalogHandler, InspectPluginCatalog, InspectPluginCatalogHandler,
    ListPluginAssignments, ListPluginAssignmentsHandler, ListPluginRegistries,
    ListPluginRegistriesHandler, PluginAssignmentReconcileReport, PluginAssignmentReconciler,
    RecordPluginPlanProjection, RecordPluginPlanProjectionHandler, SearchCachedPluginCatalog,
    SearchCachedPluginCatalogHandler, SearchPluginCatalog, SearchPluginCatalogHandler,
    SetPluginAssignment, SetPluginAssignmentHandler, SetPluginAssignmentResult,
    PLUGIN_ASSIGNMENT_WORKFLOW_NAME, PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
};

pub use infrastructure::{
    persistence::{
        InMemoryPluginAssignmentRepository, InMemoryPluginPlanProjectionRepository,
        InMemoryPluginRegistryRepository, PostgresPluginAssignmentRepository,
        PostgresPluginPlanProjectionRepository, PostgresPluginRegistryRepository,
    },
    A3sUsePluginRegistryCatalog, IdentityPluginRegistryEnrollmentAuthorizerAdapter,
    PluginAssignmentFlowConfig, PluginAssignmentFlowConfigOptions, PluginAssignmentFlowRuntime,
    PluginAssignmentFlowRuntimeDependencies, PluginPolicyObjectStore, PluginTrustRootObjectStore,
};
pub use presentation::{
    PluginAssignmentMutationResponse, PluginAssignmentResponse, PluginCatalogInspectRequest,
    PluginCatalogSearchRequest, PluginRegistryResponse, PluginsModule, SetPluginAssignmentRequest,
    ConfirmPluginPlanProjectionRequest, PluginPlanProjectionResponse,
};
