use super::super::validate_secret_bindings;
use super::{CreateWorkloadDeployment, CreateWorkloadDeploymentResult};
use crate::modules::fleet::domain::repositories::INodePoolRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    commands::validate_node_pool_selection, DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
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

pub struct CreateWorkloadDeploymentHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
    node_pools: Arc<dyn INodePoolRepository>,
}

impl CreateWorkloadDeploymentHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
        node_pools: Arc<dyn INodePoolRepository>,
    ) -> Self {
        Self {
            environments,
            workloads,
            secrets,
            node_pools,
        }
    }
}

impl CommandHandler<CreateWorkloadDeployment> for CreateWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: CreateWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateWorkloadDeploymentResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
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
            let name = match ResourceName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = command.template.validate_request() {
                return Ok(Err(ApplicationError::Invalid(error)));
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
            if let Err(error) = validate_secret_bindings(
                secrets.as_ref(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                &command.template,
            )
            .await
            {
                return Ok(Err(error));
            }
            let mut canonical_document = serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "name": name.as_str(),
                "template": command.template,
            });
            if let Some(node_pool_id) = command.node_pool_id {
                canonical_document["nodePoolId"] = serde_json::json!(node_pool_id);
            }
            let canonical = serde_json::to_vec(&canonical_document)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/workloads",
                    command.organization_id, command.project_id, command.environment_id
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
            let revision = match WorkloadRevision::request(
                WorkloadRevisionId::new(),
                workload.id,
                1,
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
