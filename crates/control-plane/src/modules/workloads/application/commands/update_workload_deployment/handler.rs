use super::super::validate_secret_bindings;
use super::{UpdateWorkloadDeployment, UpdateWorkloadDeploymentResult};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, WorkloadRevisionId,
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

pub struct UpdateWorkloadDeploymentHandler {
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
}

impl UpdateWorkloadDeploymentHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self { workloads, secrets }
    }
}

impl CommandHandler<UpdateWorkloadDeployment> for UpdateWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: UpdateWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<UpdateWorkloadDeploymentResult>>,
    > {
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            let workload = match workloads
                .find_workload(command.organization_id, command.workload_id)
                .await
            {
                Ok(workload) => workload,
                Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound("workload not found".into())))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if workload.desired_state != WorkloadDesiredState::Running
                || workload.active_revision_id.is_none()
            {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running workload can be updated".into(),
                )));
            }
            if let Err(error) = command.template.validate_request() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            if let Err(error) = validate_secret_bindings(
                secrets.as_ref(),
                workload.organization_id,
                workload.project_id,
                workload.environment_id,
                &command.template,
            )
            .await
            {
                return Ok(Err(error));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "workloadId": command.workload_id,
                "template": command.template,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workloads/{}/deployments",
                    command.organization_id, command.workload_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
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
            let revision = match WorkloadRevision::request(
                WorkloadRevisionId::new(),
                workload.id,
                generation,
                command.template,
                command.requested_at,
            ) {
                Ok(revision) => revision,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
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
                WorkflowIdentity::new("cloud.deployment", "2").map_err(BootError::Internal)?,
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
