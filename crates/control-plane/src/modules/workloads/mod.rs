pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CancelDeployment, CancelDeploymentHandler, CancelDeploymentResult, CreateWorkloadDeployment,
    CreateWorkloadDeploymentHandler, CreateWorkloadDeploymentResult, DeploymentQueryResult,
    GetDeployment, GetDeploymentHandler, GetWorkload, GetWorkloadHandler, GetWorkloadLogs,
    GetWorkloadLogsHandler, ListWorkloads, ListWorkloadsHandler, StopWorkload, StopWorkloadHandler,
    StopWorkloadResult, WorkloadLogGapReason, WorkloadLogPage, WorkloadLogRecord,
    WorkloadQueryResult,
};
pub use domain::entities::{
    Deployment, DeploymentStatus, HttpHealthCheck, OciArtifact, OciArtifactReference,
    RequestedServiceTemplate, SecretBinding, SecretBindingTarget, ServicePort, ServiceProcess,
    ServiceResources, ServiceTemplate, Workload, WorkloadDesiredState, WorkloadRevision,
};
pub use domain::events::{
    DeploymentCancellationRequested, DeploymentRequested, WorkloadStopRequested,
};
pub use domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle,
    ISecretRotationRestartRepository, IWorkloadRepository, IWorkloadRuntimeTargetRepository,
    RequestDeploymentCancellationBundle, RequestWorkloadStopBundle, SecretRotation,
    SecretRotationCompletion, SecretRotationReconciliation, WorkloadStopBundle,
};
pub use domain::services::{
    IOciArtifactResolver, OciArtifactResolutionError, OciRegistryCredentialReference,
};
pub use infrastructure::{
    DeploymentFlowConfig, DeploymentFlowRuntime, IWorkloadRuntimeControl,
    InMemoryWorkloadRepository, OciRegistryArtifactResolver, PostgresWorkloadRepository,
    SecretRotationRestartFailure, SecretRotationRestartReconciler, SecretRotationRestartReport,
    WorkloadReconciliationFailure, WorkloadReconciliationReport, WorkloadRuntimeReconciler,
};
pub use presentation::WorkloadsModule;
