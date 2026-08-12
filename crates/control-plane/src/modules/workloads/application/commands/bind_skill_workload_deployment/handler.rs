use super::BindSkillWorkloadDeployment;
use crate::modules::assets::domain::IAssetRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::application::commands::skill_release::load_deployable_skill_release;
use crate::modules::workloads::application::resource_access::WorkloadResourceAccess;
use crate::modules::workloads::application::{
    commands::validate_secret_bindings, UpdateWorkloadDeploymentResult, DEPLOYMENT_WORKFLOW_NAME,
    DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{
    Deployment, WorkloadControlSpec, WorkloadDesiredState,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct BindSkillWorkloadDeploymentHandler {
    assets: Arc<dyn IAssetRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
}

impl BindSkillWorkloadDeploymentHandler {
    pub fn new(
        assets: Arc<dyn IAssetRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self {
            assets,
            workloads,
            secrets,
        }
    }
}

impl CommandHandler<BindSkillWorkloadDeployment> for BindSkillWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: BindSkillWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<UpdateWorkloadDeploymentResult>>,
    > {
        let assets = Arc::clone(&self.assets);
        let workloads = Arc::clone(&self.workloads);
        let resource_access = WorkloadResourceAccess::new(Arc::clone(&workloads));
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            let workload = match resource_access
                .workload(
                    command.organization_id,
                    command.workload_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(workload) => workload,
                Err(error) => return Ok(Err(error)),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "workloadId": command.workload_id,
                "skillAssetId": command.skill_asset_id,
                "skillAssetReleaseId": command.skill_asset_release_id,
                "action": "bind",
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workloads/{}/skills/{}/releases/{}/bindings",
                    command.organization_id,
                    command.workload_id,
                    command.skill_asset_id,
                    command.skill_asset_release_id,
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
                        && bundle.revision.workload_id == command.workload_id
                        && bundle
                            .revision
                            .skill_binding(command.skill_asset_id)
                            .is_some_and(|binding| {
                                binding.asset_release_id() == command.skill_asset_release_id
                            }) =>
                {
                    bundle.replayed = true;
                    return Ok(Ok(UpdateWorkloadDeploymentResult { bundle }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "Skill Workload binding replay changed its identity".into(),
                    ))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            if workload.desired_state != WorkloadDesiredState::Running {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running Agent Workload can bind a Skill".into(),
                )));
            }
            let Some(active_revision_id) = workload.active_revision_id else {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running Agent Workload can bind a Skill".into(),
                )));
            };
            let active_revision = match workloads
                .find_revision(command.organization_id, active_revision_id)
                .await
            {
                Ok(revision)
                    if revision.workload_id == workload.id
                        && revision.agent_binding().is_some() =>
                {
                    revision
                }
                Ok(_) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "active Agent Workload revision is unavailable".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let deployable = match load_deployable_skill_release(
                assets.as_ref(),
                command.organization_id,
                command.skill_asset_id,
                command.skill_asset_release_id,
            )
            .await
            {
                Ok(deployable) => deployable,
                Err(error) => return Ok(Err(error)),
            };
            let generation = match workloads
                .list_revisions(command.organization_id, command.workload_id)
                .await
            {
                Ok(revisions) => revisions
                    .into_iter()
                    .map(|revision| revision.generation)
                    .max()
                    .unwrap_or_default()
                    .checked_add(1),
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(generation) = generation else {
                return Ok(Err(ApplicationError::Conflict(
                    "workload revision generation is exhausted".into(),
                )));
            };
            let revision = match active_revision.with_skill_release_as(
                WorkloadRevisionId::new(),
                generation,
                command.requested_at,
                &workload,
                &deployable.asset,
                &deployable.release,
            ) {
                Ok(revision) => revision,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
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
                    "skillAssetId": command.skill_asset_id,
                    "skillAssetReleaseId": command.skill_asset_release_id,
                    "skillBindingAction": "bind",
                    "workloadId": workload.id,
                }),
                command.requested_at,
            );
            let event = DeploymentRequested::envelope(&deployment, &revision, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let bundle = match workloads
                .create_deployment(CreateDeploymentBundle {
                    workload,
                    control: WorkloadControlSpec::unmanaged_single_replica(),
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
