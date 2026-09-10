use super::{CreateSourceWorkloadDeployment, CreateSourceWorkloadDeploymentResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, ResourceName, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    commands::{validate_node_pool_selection, validate_secret_bindings},
    IWorkloadSourceBuildAdmissionPort, IWorkloadsEnvironmentAccess, IWorkloadsNodePoolAccess,
    IWorkloadsSecretBindingAccess, WorkloadSourceBuildAdmissionRequest, WorkloadsEnvironmentScope,
};
use crate::modules::workloads::domain::entities::{
    Deployment, Workload, WorkloadControlSpec, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use crate::modules::workloads::domain::WorkloadDeploymentOperationIntent;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateSourceWorkloadDeploymentHandler {
    environments: Arc<dyn IWorkloadsEnvironmentAccess>,
    source_builds: Arc<dyn IWorkloadSourceBuildAdmissionPort>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn IWorkloadsSecretBindingAccess>,
    node_pools: Arc<dyn IWorkloadsNodePoolAccess>,
}

impl CreateSourceWorkloadDeploymentHandler {
    pub fn new(
        environments: Arc<dyn IWorkloadsEnvironmentAccess>,
        source_builds: Arc<dyn IWorkloadSourceBuildAdmissionPort>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn IWorkloadsSecretBindingAccess>,
        node_pools: Arc<dyn IWorkloadsNodePoolAccess>,
    ) -> Self {
        Self {
            environments,
            source_builds,
            workloads,
            secrets,
            node_pools,
        }
    }
}

impl CommandHandler<CreateSourceWorkloadDeployment> for CreateSourceWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: CreateSourceWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateSourceWorkloadDeploymentResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let source_builds = Arc::clone(&self.source_builds);
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
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
            let admission = match source_builds
                .admit(WorkloadSourceBuildAdmissionRequest {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.environment_id,
                    source_revision_id: command.source_revision_id,
                })
                .await
            {
                Ok(admission) => admission,
                Err(error) => return Ok(Err(error)),
            };
            let name = match ResourceName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = validate_node_pool_selection(
                node_pools.as_ref(),
                command.organization_id,
                command.node_pool_id,
            )
            .await
            {
                return Ok(Err(error));
            }
            let mut canonical_document = serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "sourceRevisionId": admission.external_build().source_revision_id,
                "buildRunId": admission.external_build().build_run_id,
                "publishedArtifactDigest": admission.artifact().digest,
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
                    "organizations/{}/projects/{}/environments/{}/source-revisions/{}/workloads",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    admission.external_build().source_revision_id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let workload = Workload::create(
                WorkloadId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                name,
                command.requested_at,
            );
            let revision = match WorkloadRevision::create_from_external_build(
                WorkloadRevisionId::new(),
                workload.id,
                1,
                command.template.resolve(admission.artifact().clone()),
                admission.external_build().clone(),
                command.requested_at,
            ) {
                Ok(revision) => revision,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
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
            let operation = WorkloadDeploymentOperationIntent::new(
                deployment.operation_id,
                workload.organization_id,
                deployment.id,
                revision.id,
                workload.id,
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
            Ok(Ok(CreateSourceWorkloadDeploymentResult { bundle }))
        })
    }
}
