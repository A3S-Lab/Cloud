use super::super::validate_secret_bindings;
use super::{RollbackWorkloadDeployment, RollbackWorkloadDeploymentResult};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, WorkloadDesiredState,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RollbackWorkloadDeploymentHandler {
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
}

impl RollbackWorkloadDeploymentHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self { workloads, secrets }
    }
}

impl CommandHandler<RollbackWorkloadDeployment> for RollbackWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: RollbackWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RollbackWorkloadDeploymentResult>>,
    > {
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "workloadId": command.workload_id,
                "revisionId": command.source_revision_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workloads/{}/rollback",
                    command.organization_id, command.workload_id
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
                        && bundle.deployment.workload_id == command.workload_id
                        && bundle.deployment.revision_id == bundle.revision.id =>
                {
                    bundle.replayed = true;
                    return Ok(Ok(RollbackWorkloadDeploymentResult {
                        bundle,
                        source_revision_id: command.source_revision_id,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "workload rollback replay changed its identity".into(),
                    ))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

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
            if workload.desired_state != WorkloadDesiredState::Running {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running workload can be rolled back".into(),
                )));
            }
            let Some(active_revision_id) = workload.active_revision_id else {
                return Ok(Err(ApplicationError::Conflict(
                    "only an active running workload can be rolled back".into(),
                )));
            };
            if active_revision_id == command.source_revision_id {
                return Ok(Err(ApplicationError::Conflict(
                    "workload is already using the requested rollback revision".into(),
                )));
            }

            let source_revision = match workloads
                .find_revision(command.organization_id, command.source_revision_id)
                .await
            {
                Ok(revision) if revision.workload_id == workload.id => revision,
                Ok(_) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "rollback revision not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let active_revision = match workloads
                .find_revision(command.organization_id, active_revision_id)
                .await
            {
                Ok(revision) if revision.workload_id == workload.id => revision,
                Ok(_) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "active workload revision is unavailable".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if source_revision.generation >= active_revision.generation {
                return Ok(Err(ApplicationError::Conflict(
                    "rollback must select an older workload revision".into(),
                )));
            }

            let deployments = match workloads
                .list_deployments(command.organization_id, workload.id)
                .await
            {
                Ok(deployments) => deployments,
                Err(error) => return Ok(Err(error.into())),
            };
            if !deployments.iter().any(|deployment| {
                deployment.revision_id == source_revision.id
                    && deployment.status == DeploymentStatus::Active
                    && deployment.activated_at.is_some()
            }) {
                return Ok(Err(ApplicationError::Conflict(
                    "rollback revision was never activated successfully".into(),
                )));
            }

            let generation = match workloads
                .list_revisions(command.organization_id, workload.id)
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
            let revision = match source_revision.rollback_as(
                WorkloadRevisionId::new(),
                generation,
                command.requested_at,
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
                WorkflowIdentity::new("cloud.deployment", "2").map_err(BootError::Internal)?,
                serde_json::json!({
                    "deploymentId": deployment.id,
                    "organizationId": workload.organization_id,
                    "revisionId": revision.id,
                    "rollbackSourceRevisionId": source_revision.id,
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
            Ok(Ok(RollbackWorkloadDeploymentResult {
                bundle,
                source_revision_id: command.source_revision_id,
            }))
        })
    }
}
