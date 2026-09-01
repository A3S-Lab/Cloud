mod agent_release_admission;
mod bound_runtime_claim;
pub mod commands;
pub mod queries;
mod resource_access;
mod runtime_execution_admission;
mod runtime_projection;
mod workflow;

#[cfg(test)]
mod tests;

pub(crate) use agent_release_admission::admit_deployable_agent_release;
pub use bound_runtime_claim::{
    BoundRuntimeClaimQuery, BoundRuntimeClaimQueryService, IBoundRuntimeClaimQueryPort,
};
pub use commands::bind_skill_workload_deployment::{
    BindSkillWorkloadDeployment, BindSkillWorkloadDeploymentHandler,
};
pub use commands::cancel_deployment::{
    CancelDeployment, CancelDeploymentHandler, CancelDeploymentResult,
};
pub use commands::create_agent_workload_deployment::{
    CreateAgentWorkloadDeployment, CreateAgentWorkloadDeploymentHandler,
};
pub use commands::create_source_workload_deployment::{
    CreateSourceWorkloadDeployment, CreateSourceWorkloadDeploymentHandler,
    CreateSourceWorkloadDeploymentResult, SourceWorkloadTemplate,
};
pub use commands::create_workload_deployment::{
    CreateWorkloadDeployment, CreateWorkloadDeploymentHandler, CreateWorkloadDeploymentResult,
};
pub use commands::rollback_workload_deployment::{
    RollbackWorkloadDeployment, RollbackWorkloadDeploymentHandler, RollbackWorkloadDeploymentResult,
};
pub use commands::stop_workload::{StopWorkload, StopWorkloadHandler, StopWorkloadResult};
pub use commands::unbind_skill_workload_deployment::{
    UnbindSkillWorkloadDeployment, UnbindSkillWorkloadDeploymentHandler,
};
pub use commands::update_agent_workload_deployment::{
    UpdateAgentWorkloadDeployment, UpdateAgentWorkloadDeploymentHandler,
};
pub use commands::update_workload_deployment::{
    UpdateWorkloadDeployment, UpdateWorkloadDeploymentHandler, UpdateWorkloadDeploymentResult,
};
pub use queries::{
    DeploymentQueryResult, GetDeployment, GetDeploymentHandler, GetWorkload, GetWorkloadHandler,
    GetWorkloadLogs, GetWorkloadLogsHandler, ListWorkloads, ListWorkloadsHandler,
    WorkloadLogGapReason, WorkloadLogPage, WorkloadLogRecord, WorkloadQueryResult,
    WorkloadReplicaQueryResult,
};
pub use resource_access::WorkloadAccess;
pub(crate) use resource_access::{WorkloadAccessScope, WorkloadResourceResolver};
pub use runtime_execution_admission::{
    AdmittedWorkloadRuntimeExecution, DeploymentRuntimeExecutionAdmissionRequest,
    IWorkloadRuntimeExecutionAdmissionPort, NoWorkloadRuntimeExecutionAdmission,
};
pub(crate) use runtime_projection::{
    project_bound_runtime_spec_with_execution, project_placement_group_runtime_spec_with_execution,
    project_replica_runtime_spec_with_execution, project_runtime_secrets,
    project_runtime_spec_with_digest,
};
pub use runtime_projection::{project_replica_runtime_spec, project_runtime_spec};
pub use workflow::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION, LEGACY_DEPLOYMENT_WORKFLOW_VERSION,
    PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION, PREVIOUS_PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    RESOURCE_CLAIM_DEPLOYMENT_WORKFLOW_VERSION, STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION,
};
