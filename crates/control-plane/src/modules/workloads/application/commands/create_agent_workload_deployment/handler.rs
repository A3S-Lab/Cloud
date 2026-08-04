use super::CreateAgentWorkloadDeployment;
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::assets::{load_deployable_agent_release, IAssetRepository};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    commands::validate_secret_bindings, CreateWorkloadDeploymentResult, DEPLOYMENT_WORKFLOW_NAME,
    DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{
    Deployment, OciArtifact, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateAgentWorkloadDeploymentHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    assets: Arc<dyn IAssetRepository>,
    builds: Arc<dyn IBuildRunRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
}

impl CreateAgentWorkloadDeploymentHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        assets: Arc<dyn IAssetRepository>,
        builds: Arc<dyn IBuildRunRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self {
            environments,
            assets,
            builds,
            workloads,
            secrets,
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
        let assets = Arc::clone(&self.assets);
        let builds = Arc::clone(&self.builds);
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            let name = match ResourceName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "assetId": command.asset_id,
                "assetReleaseId": command.asset_release_id,
                "name": name.as_str(),
                "template": &command.template,
            }))
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
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let deployable = match load_deployable_agent_release(
                assets.as_ref(),
                builds.as_ref(),
                command.organization_id,
                command.asset_id,
                command.asset_release_id,
            )
            .await
            {
                Ok(deployable) => deployable,
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
                command.template.resolve(OciArtifact {
                    uri: deployable.artifact_uri.clone(),
                    digest: deployable.artifact_digest.clone(),
                    media_type: deployable.artifact_media_type.clone(),
                }),
                command.requested_at,
            ) {
                Ok(revision) => revision,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = revision.bind_agent_release(
                &workload,
                &deployable.asset,
                &deployable.release,
                &deployable.build,
            ) {
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
            let bundle = match workloads
                .create_deployment(CreateDeploymentBundle {
                    workload,
                    control: crate::modules::workloads::domain::entities::WorkloadControlSpec::unmanaged_single_replica(),
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
