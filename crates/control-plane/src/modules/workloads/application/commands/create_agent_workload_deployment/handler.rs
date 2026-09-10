use super::CreateAgentWorkloadDeployment;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, ResourceName, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    commands::{validate_node_pool_selection, validate_secret_bindings},
    CreateWorkloadDeploymentResult, IWorkloadAgentReleaseAdmissionPort, IWorkloadsEnvironmentAccess,
    IWorkloadsNodePoolAccess, IWorkloadsSecretBindingAccess, WorkloadAgentReleaseAdmissionRequest,
    WorkloadsEnvironmentScope, DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{
    Deployment, Workload, WorkloadControlSpec, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateAgentWorkloadDeploymentHandler {
    environments: Arc<dyn IWorkloadsEnvironmentAccess>,
    agent_releases: Arc<dyn IWorkloadAgentReleaseAdmissionPort>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn IWorkloadsSecretBindingAccess>,
    node_pools: Arc<dyn IWorkloadsNodePoolAccess>,
}

impl CreateAgentWorkloadDeploymentHandler {
    pub fn new(
        environments: Arc<dyn IWorkloadsEnvironmentAccess>,
        agent_releases: Arc<dyn IWorkloadAgentReleaseAdmissionPort>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn IWorkloadsSecretBindingAccess>,
        node_pools: Arc<dyn IWorkloadsNodePoolAccess>,
    ) -> Self {
        Self {
            environments,
            agent_releases,
            workloads,
            secrets,
            node_pools,
        }
    }
}

impl CommandHandler<CreateAgentWorkloadDeployment> for CreateAgentWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: CreateAgentWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateWorkloadDeploymentResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let agent_releases = Arc::clone(&self.agent_releases);
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
            let name = match ResourceName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let mut canonical_document = serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "assetId": command.asset_id,
                "assetReleaseId": command.asset_release_id,
                "name": name.as_str(),
                "template": &command.template,
            });
            if let Some(node_pool_id) = command.node_pool_id {
                canonical_document["nodePoolId"] = serde_json::json!(node_pool_id);
            }
            let canonical = serde_json::to_vec(&canonical_document)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/assets/{}/releases/{}/workloads",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.asset_id,
                    command.asset_release_id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match workloads.replay_deployment(&idempotency).await {
                Ok(Some(mut bundle))
                    if bundle.workload.organization_id == command.organization_id
                        && bundle.workload.project_id == command.project_id
                        && bundle.workload.environment_id == command.environment_id
                        && bundle.revision.agent_binding().is_some_and(|binding| {
                            binding.asset_id() == command.asset_id
                                && binding.asset_release_id() == command.asset_release_id
                        }) =>
                {
                    bundle.replayed = true;
                    return Ok(Ok(CreateWorkloadDeploymentResult { bundle }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "Agent Workload deployment replay changed its identity".into(),
                    ))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let environment_scope = match WorkloadsEnvironmentScope::new(
                command.organization_id,
                command.project_id,
                command.environment_id,
            ) {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match environments.environment_exists(environment_scope).await {
                Ok(true) => {}
                Ok(false) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if let Err(error) = validate_node_pool_selection(
                node_pools.as_ref(),
                command.organization_id,
                command.node_pool_id,
            )
            .await
            {
                return Ok(Err(error));
            }
            let admission = match agent_releases
                .admit(WorkloadAgentReleaseAdmissionRequest {
                    organization_id: command.organization_id,
                    asset_id: command.asset_id,
                    asset_release_id: command.asset_release_id,
                })
                .await
            {
                Ok(admission) => admission,
                Err(error) => return Ok(Err(error)),
            };
            let workload = Workload::create(
                WorkloadId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                name,
                command.requested_at,
            );
            let mut revision = match WorkloadRevision::create(
                WorkloadRevisionId::new(),
                workload.id,
                1,
                match command.template.resolve_agent(&admission) {
                    Ok(template) => template,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                command.requested_at,
            ) {
                Ok(revision) => revision,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = revision.bind_agent_release(&workload, &admission) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            if let Err(error) = validate_secret_bindings(
                secrets.as_ref(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                &revision.request,
            )
            .await
            {
                return Ok(Err(error));
            }
            let deployment = Deployment::create(
                DeploymentId::new(),
                workload.organization_id,
                workload.id,
                revision.id,
                OperationId::new(),
                command.requested_at,
            );
            let operation = OperationRequest::new(
                deployment.operation_id,
                workload.organization_id,
                OperationSubject::new("deployment", deployment.id.as_uuid())
                    .map_err(BootError::Internal)?,
                WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)
                    .map_err(BootError::Internal)?,
                serde_json::json!({
                    "deploymentId": deployment.id,
                    "organizationId": workload.organization_id,
                    "revisionId": revision.id,
                    "workloadId": workload.id,
                }),
                command.requested_at,
            );
            let event = DeploymentRequested::envelope(&deployment, &revision, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let control = match WorkloadControlSpec::unmanaged_replica_set_in_pool(
                1,
                1,
                command.node_pool_id,
            ) {
                Ok(control) => control,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let bundle = match workloads
                .create_deployment(CreateDeploymentBundle {
                    workload,
                    control,
                    revision,
                    deployment,
                    operation,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(bundle) => bundle,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateWorkloadDeploymentResult { bundle }))
        })
    }
}
