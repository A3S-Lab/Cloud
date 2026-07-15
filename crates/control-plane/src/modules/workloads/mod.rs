pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CancelDeployment, CancelDeploymentHandler, CancelDeploymentResult, CreateWorkloadDeployment,
    CreateWorkloadDeploymentHandler, CreateWorkloadDeploymentResult, DeploymentQueryResult,
    GetDeployment, GetDeploymentHandler, GetWorkload, GetWorkloadHandler, ListWorkloads,
    ListWorkloadsHandler, StopWorkload, StopWorkloadHandler, StopWorkloadResult,
    WorkloadQueryResult,
};
pub use domain::entities::{
    Deployment, DeploymentStatus, HttpHealthCheck, OciArtifact, OciArtifactReference,
    RequestedServiceTemplate, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    Workload, WorkloadDesiredState, WorkloadRevision,
};
pub use domain::events::{
    DeploymentCancellationRequested, DeploymentRequested, WorkloadStopRequested,
};
pub use domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle, IWorkloadRepository,
    IWorkloadRuntimeTargetRepository, RequestDeploymentCancellationBundle,
    RequestWorkloadStopBundle, WorkloadStopBundle,
};
pub use domain::services::{IOciArtifactResolver, OciArtifactResolutionError};
pub use infrastructure::{
    DeploymentFlowConfig, DeploymentFlowRuntime, IWorkloadRuntimeControl,
    InMemoryWorkloadRepository, OciRegistryArtifactResolver, PostgresWorkloadRepository,
    WorkloadReconciliationFailure, WorkloadReconciliationReport, WorkloadRuntimeReconciler,
};
pub use presentation::WorkloadsModule;
