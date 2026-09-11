pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub(crate) use application::PluginAccessScope;

#[cfg(test)]
pub(crate) mod test_support;

pub use application::{
    ConfirmPluginPlanProjection, ConfirmPluginPlanProjectionHandler, EnrollPluginRegistry,
    EnrollPluginRegistryHandler, EnrollPluginRegistryResult, GetPluginAssignment,
    GetPluginAssignmentHandler, GetPluginPlanProjection, GetPluginPlanProjectionHandler,
    GetPluginRegistry, GetPluginRegistryHandler, InspectCachedPluginCatalog,
    InspectCachedPluginCatalogHandler, InspectPluginCatalog, InspectPluginCatalogHandler,
    ListPluginAssignments, ListPluginAssignmentsHandler, ListPluginRegistries,
    ListPluginRegistriesHandler, PLUGIN_ASSIGNMENT_WORKFLOW_NAME,
    PLUGIN_ASSIGNMENT_WORKFLOW_VERSION, PluginAccess, PluginAssignmentReconcileReport,
    PluginAssignmentReconciler, RecordPluginPlanProjection, RecordPluginPlanProjectionHandler,
    SearchCachedPluginCatalog, SearchCachedPluginCatalogHandler, SearchPluginCatalog,
    SearchPluginCatalogHandler, SetPluginAssignment, SetPluginAssignmentHandler,
    SetPluginAssignmentResult,
};

pub use infrastructure::{
    A3sUsePluginRegistryCatalog, IdentityPluginRegistryEnrollmentAuthorizerAdapter,
    PluginAssignmentFlowConfig, PluginAssignmentFlowConfigOptions, PluginAssignmentFlowRuntime,
    PluginAssignmentFlowRuntimeDependencies, PluginPolicyObjectStore, PluginTrustRootObjectStore,
    persistence::{
        InMemoryPluginAssignmentRepository, InMemoryPluginPlanProjectionRepository,
        InMemoryPluginRegistryRepository, PostgresPluginAssignmentRepository,
        PostgresPluginPlanProjectionRepository, PostgresPluginRegistryRepository,
    },
};
pub use presentation::{
    ConfirmPluginPlanProjectionRequest, EnrollPluginRegistryRequest,
    PluginAssignmentMutationResponse, PluginAssignmentResponse, PluginCatalogInspectRequest,
    PluginCatalogSearchRequest, PluginPlanProjectionResponse, PluginRegistryMutationResponse,
    PluginRegistryResponse, PluginsModule, SetPluginAssignmentRequest,
};
