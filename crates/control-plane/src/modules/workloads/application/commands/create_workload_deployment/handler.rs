use super::super::validate_secret_bindings;
use super::{CreateWorkloadDeployment, CreateWorkloadDeploymentResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, ResourceName, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    IWorkloadsEnvironmentAccess, IWorkloadsNodePoolAccess, IWorkloadsSecretBindingAccess,
    WorkloadsEnvironmentScope, commands::validate_node_pool_selection,
};
use crate::modules::workloads::domain::WorkloadDeploymentOperationIntent;
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
    environments: Arc<dyn IWorkloadsEnvironmentAccess>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn IWorkloadsSecretBindingAccess>,
    node_pools: Arc<dyn IWorkloadsNodePoolAccess>,
}

impl CreateWorkloadDeploymentHandler {
    pub fn new(
        environments: Arc<dyn IWorkloadsEnvironmentAccess>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn IWorkloadsSecretBindingAccess>,
        node_pools: Arc<dyn IWorkloadsNodePoolAccess>,
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
            if !command
                .access
                .environment_is_visible(command.project_id, command.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found".into(),
                )));
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
                    )));
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
            Ok(Ok(CreateWorkloadDeploymentResult { bundle }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use crate::modules::workloads::application::{
        WorkloadAccess, WorkloadAccessScope, WorkloadsNodePoolScope, WorkloadsSecretBindingScope,
    };
    use crate::modules::workloads::domain::entities::{
        OciArtifactReference, RequestedServiceTemplate, ServiceProcess, ServiceResources,
    };
    use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    struct AllowEnvironment;

    #[async_trait]
    impl IWorkloadsEnvironmentAccess for AllowEnvironment {
        async fn environment_exists(
            &self,
            _scope: WorkloadsEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct RejectSecrets;

    #[async_trait]
    impl IWorkloadsSecretBindingAccess for RejectSecrets {
        async fn binding_is_admissible(
            &self,
            _scope: WorkloadsSecretBindingScope,
        ) -> Result<bool, RepositoryError> {
            Err(RepositoryError::Storage(
                "secret admission must not run for denied creates".into(),
            ))
        }
    }

    struct RejectNodePools;

    #[async_trait]
    impl IWorkloadsNodePoolAccess for RejectNodePools {
        async fn node_pool_exists(
            &self,
            _scope: WorkloadsNodePoolScope,
        ) -> Result<bool, RepositoryError> {
            Err(RepositoryError::Storage(
                "node-pool lookup must not run for denied creates".into(),
            ))
        }
    }

    fn denied_template() -> RequestedServiceTemplate {
        RequestedServiceTemplate {
            artifact: OciArtifactReference {
                uri: "oci://registry.example/cloud/denied@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                expected_digest: Some(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .into(),
                ),
            },
            process: ServiceProcess {
                command: vec!["/bin/true".into()],
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            secrets: Vec::new(),
            resources: ServiceResources {
                cpu_millis: 100,
                memory_bytes: 32 * 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: None,
            },
            ports: Vec::new(),
            health: None,
        }
    }

    #[tokio::test]
    async fn create_workload_deployment_fails_closed_before_creating_in_an_ungranted_environment() {
        let handler = CreateWorkloadDeploymentHandler::new(
            Arc::new(AllowEnvironment),
            Arc::new(InMemoryWorkloadRepository::new()),
            Arc::new(RejectSecrets),
            Arc::new(RejectNodePools),
        );
        let result = handler
            .execute(
                CreateWorkloadDeployment {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                    access: WorkloadAccess::restricted([WorkloadAccessScope::Environment {
                        project_id: ProjectId::new(),
                        environment_id: EnvironmentId::new(),
                    }]),
                    name: "denied-workload".into(),
                    node_pool_id: None,
                    template: denied_template(),
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
                    requested_at: Utc::now(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert_eq!(
            result,
            Err(ApplicationError::NotFound("environment not found".into()))
        );
    }
}
