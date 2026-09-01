use super::UpdateAgentWorkloadDeployment;
use crate::modules::artifacts::IHostedArtifactQueryPort;
use crate::modules::assets::{load_deployable_agent_release, IAssetRepository};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, ResourceName,
    WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    admit_deployable_agent_release,
    commands::{
        load_direct_workload_control, require_acl_node_pool_selection, validate_secret_bindings,
    },
    UpdateWorkloadDeploymentResult, WorkloadResourceResolver, DEPLOYMENT_WORKFLOW_NAME,
    DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{
    Deployment, WorkloadDesiredState, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct UpdateAgentWorkloadDeploymentHandler {
    assets: Arc<dyn IAssetRepository>,
    artifacts: Arc<dyn IHostedArtifactQueryPort>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
}

impl UpdateAgentWorkloadDeploymentHandler {
    pub fn new(
        assets: Arc<dyn IAssetRepository>,
        artifacts: Arc<dyn IHostedArtifactQueryPort>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self {
            assets,
            artifacts,
            workloads,
            secrets,
        }
    }
}

impl CommandHandler<UpdateAgentWorkloadDeployment> for UpdateAgentWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: UpdateAgentWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<UpdateWorkloadDeploymentResult>>,
    > {
        let assets = Arc::clone(&self.assets);
        let artifacts = Arc::clone(&self.artifacts);
        let workloads = Arc::clone(&self.workloads);
        let resource_resolver = WorkloadResourceResolver::new(Arc::clone(&workloads));
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            let workload = match resource_resolver
                .workload(
                    command.organization_id,
                    command.workload_id,
                    &command.access,
                )
                .await
            {
                Ok(workload) => workload,
                Err(error) => return Ok(Err(error)),
            };
            let mut canonical_document = serde_json::json!({
                "organizationId": command.organization_id,
                "workloadId": command.workload_id,
                "assetId": command.asset_id,
                "assetReleaseId": command.asset_release_id,
                "expectedName": command.expected_name.as_deref(),
                "template": &command.template,
            });
            if let Some(node_pool_id) = command.expected_node_pool_id.flatten() {
                canonical_document["nodePoolId"] = serde_json::json!(node_pool_id);
            }
            let canonical = serde_json::to_vec(&canonical_document)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workloads/{}/assets/{}/releases/{}/deployments",
                    command.organization_id,
                    command.workload_id,
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
                        && bundle.workload.id == command.workload_id
                        && bundle.revision.agent_binding().is_some_and(|binding| {
                            binding.asset_id() == command.asset_id
                                && binding.asset_release_id() == command.asset_release_id
                        }) =>
                {
                    bundle.replayed = true;
                    return Ok(Ok(UpdateWorkloadDeploymentResult { bundle }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "Agent Workload update replay changed its identity".into(),
                    ))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            if workload.desired_state != WorkloadDesiredState::Running {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running Agent Workload can be updated".into(),
                )));
            }
            let control = match load_direct_workload_control(
                workloads.as_ref(),
                command.organization_id,
                command.workload_id,
            )
            .await
            {
                Ok(control) => control,
                Err(error) => return Ok(Err(error)),
            };
            if let Err(error) =
                require_acl_node_pool_selection(&control, command.expected_node_pool_id)
            {
                return Ok(Err(error));
            }
            let Some(active_revision_id) = workload.active_revision_id else {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running Agent Workload can be updated".into(),
                )));
            };
            if let Some(expected_name) = command.expected_name.as_deref() {
                let expected_name = match ResourceName::parse(expected_name) {
                    Ok(name) => name,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
                if expected_name.key() != workload.name.key() {
                    return Ok(Err(ApplicationError::Conflict(
                        "workload ACL name does not match the target workload".into(),
                    )));
                }
            }
            let active_revision = match workloads
                .find_revision(command.organization_id, active_revision_id)
                .await
            {
                Ok(revision) if revision.workload_id == workload.id => revision,
                Ok(_) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "active Agent Workload revision is unavailable".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if active_revision
                .agent_binding()
                .is_none_or(|binding| binding.asset_id() != command.asset_id)
            {
                return Ok(Err(ApplicationError::Conflict(
                    "Agent Workload updates must retain the same Asset identity".into(),
                )));
            }
            let deployable = match load_deployable_agent_release(
                assets.as_ref(),
                artifacts.as_ref(),
                command.organization_id,
                command.asset_id,
                command.asset_release_id,
            )
            .await
            {
                Ok(deployable) => deployable,
                Err(error) => return Ok(Err(error)),
            };
            let revisions = match workloads
                .list_revisions(command.organization_id, command.workload_id)
                .await
            {
                Ok(revisions) => revisions,
                Err(error) => return Ok(Err(error.into())),
            };
            let generation = match revisions
                .iter()
                .map(|revision| revision.generation)
                .max()
                .unwrap_or_default()
                .checked_add(1)
            {
                Some(generation) => generation,
                None => {
                    return Ok(Err(ApplicationError::Conflict(
                        "workload revision generation is exhausted".into(),
                    )))
                }
            };
            let admission = match admit_deployable_agent_release(&deployable) {
                Ok(admission) => admission,
                Err(error) => return Ok(Err(error)),
            };
            let mut revision = match WorkloadRevision::create(
                WorkloadRevisionId::new(),
                workload.id,
                generation,
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
            for binding in active_revision.skill_bindings() {
                if let Err(error) = revision.restore_skill_binding(binding.clone()) {
                    return Ok(Err(ApplicationError::Internal(error)));
                }
            }
            if let Err(error) = revision.validate_skill_bindings_for_workload(&workload) {
                return Ok(Err(ApplicationError::Internal(error)));
            }
            if let Err(error) = validate_secret_bindings(
                secrets.as_ref(),
                workload.organization_id,
                workload.project_id,
                workload.environment_id,
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
            Ok(Ok(UpdateWorkloadDeploymentResult { bundle }))
        })
    }
}
