use super::ResolveSecretMaterial;
use crate::modules::secrets::application::{encryption_error, SecretPlaintext};
use crate::modules::secrets::domain::{
    secret_encryption_context, ISecretEncryptionService, ISecretRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ProjectId, RepositoryError, SecretId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{DeploymentStatus, WorkloadDesiredState};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ResolveSecretMaterialHandler {
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl ResolveSecretMaterialHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            workloads,
            secrets,
            encryption,
        }
    }
}

impl QueryHandler<ResolveSecretMaterial> for ResolveSecretMaterialHandler {
    fn execute(
        &self,
        query: ResolveSecretMaterial,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SecretPlaintext>>> {
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        let encryption = Arc::clone(&self.encryption);
        Box::pin(async move {
            if let Err(error) = query.reference.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let revision_id = WorkloadRevisionId::from_uuid(query.reference.workload_revision_id);
            let secret_id = SecretId::from_uuid(query.reference.secret_id);
            let revision = match workloads
                .find_revision(query.organization_id, revision_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(authorization_repository_error(error))),
            };
            let workload = match workloads
                .find_workload(query.organization_id, revision.workload_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(authorization_repository_error(error))),
            };
            let deployments = match workloads
                .list_deployments(query.organization_id, workload.id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(authorization_repository_error(error))),
            };
            let assigned = deployments.iter().any(|deployment| {
                deployment.revision_id == revision.id
                    && deployment.node_id == Some(query.authenticated_node_id)
                    && match deployment.status {
                        DeploymentStatus::Scheduled
                        | DeploymentStatus::Applying
                        | DeploymentStatus::Verifying => true,
                        DeploymentStatus::Retiring | DeploymentStatus::Active => {
                            workload.desired_state == WorkloadDesiredState::Running
                                && workload.active_revision_id == Some(revision.id)
                        }
                        _ => false,
                    }
            });
            let bound = revision.request.secrets.iter().any(|binding| {
                binding.secret_id == secret_id && binding.version == query.reference.version
            });
            if !assigned || !bound {
                return Ok(Err(not_authorized()));
            }
            let secret = match secrets.find(query.organization_id, secret_id).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(authorization_repository_error(error))),
            };
            if !same_scope(
                secret.project_id,
                secret.environment_id,
                workload.project_id,
                workload.environment_id,
            ) {
                return Ok(Err(not_authorized()));
            }
            let version = match secrets
                .find_version(query.organization_id, secret_id, query.reference.version)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(authorization_repository_error(error))),
            };
            if !version.is_materializable(&secret) {
                return Ok(Err(not_authorized()));
            }
            let context = match secret_encryption_context(
                query.organization_id,
                secret_id,
                version.version,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let plaintext = match encryption
                .decrypt(&version.encrypted_value, &context)
                .await
                .map_err(encryption_error)
                .and_then(|value| SecretPlaintext::new(value).map_err(ApplicationError::Internal))
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            Ok(Ok(plaintext))
        })
    }
}

fn same_scope(
    secret_project_id: ProjectId,
    secret_environment_id: EnvironmentId,
    workload_project_id: ProjectId,
    workload_environment_id: EnvironmentId,
) -> bool {
    secret_project_id == workload_project_id && secret_environment_id == workload_environment_id
}

fn authorization_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => not_authorized(),
        other => other.into(),
    }
}

fn not_authorized() -> ApplicationError {
    ApplicationError::Forbidden("Secret material is not authorized for this node".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::operations::domain::entities::OperationRequest;
    use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
    use crate::modules::secrets::domain::{
        CreateSecretWrite, EncryptedSecretValue, Secret, SecretChanged, SecretEncryptionError,
    };
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{
        DeploymentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId, OrganizationId,
        ProjectId, ResourceName, SecretId, WorkloadId, WorkloadRevisionId,
    };
    use crate::modules::workloads::domain::entities::{
        Deployment, DeploymentStatus, HttpHealthCheck, OciArtifact, SecretBinding,
        SecretBindingTarget, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
        Workload, WorkloadRevision,
    };
    use crate::modules::workloads::domain::events::DeploymentRequested;
    use crate::modules::workloads::domain::repositories::CreateDeploymentBundle;
    use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
    use a3s_boot::QueryHandler;
    use a3s_cloud_contracts::CloudSecretReference;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::BTreeMap;

    struct FixedEncryption;

    #[async_trait]
    impl ISecretEncryptionService for FixedEncryption {
        async fn encrypt(
            &self,
            _plaintext: &[u8],
            _context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            Err(SecretEncryptionError::Rejected(
                "test encryption is not available".into(),
            ))
        }

        async fn decrypt(
            &self,
            _value: &EncryptedSecretValue,
            _context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            Ok(b"resolved-only-at-the-node-boundary".to_vec())
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn materializes_only_an_assigned_bound_active_version() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        let secret_id = SecretId::new();
        let encrypted =
            EncryptedSecretValue::new("test:key", "test:ciphertext").expect("encrypted value");
        let (secret, version) = Secret::create(
            secret_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse("database-password").expect("Secret name"),
            encrypted,
            now,
        )
        .expect("Secret");
        let secrets = Arc::new(InMemorySecretRepository::new());
        secrets
            .create(CreateSecretWrite {
                secret: secret.clone(),
                version: version.clone(),
                idempotency: IdempotencyRequest::new(
                    "test.secret",
                    "create",
                    secret_id.as_uuid().as_bytes(),
                )
                .expect("Secret idempotency"),
                event: SecretChanged::created(&secret, &version, uuid::Uuid::now_v7())
                    .expect("Secret event"),
            })
            .await
            .expect("store Secret");

        let workload_id = WorkloadId::new();
        let workload = Workload::create(
            workload_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse("api").expect("workload name"),
            now,
        );
        let revision = WorkloadRevision::create(
            WorkloadRevisionId::new(),
            workload_id,
            1,
            template(secret_id),
            now,
        )
        .expect("workload revision");
        let deployment = Deployment::create(
            DeploymentId::new(),
            organization_id,
            workload_id,
            revision.id,
            OperationId::new(),
            now,
        );
        let operation = OperationRequest::new(
            deployment.operation_id,
            organization_id,
            OperationSubject::new("deployment", deployment.id.as_uuid())
                .expect("operation subject"),
            WorkflowIdentity::new("cloud.deployment", "2").expect("workflow"),
            serde_json::json!({}),
            now,
        );
        let event = DeploymentRequested::envelope(&deployment, &revision, uuid::Uuid::now_v7())
            .expect("deployment event");
        let workloads = Arc::new(InMemoryWorkloadRepository::new());
        workloads
            .create_deployment(CreateDeploymentBundle {
                workload,
                revision: revision.clone(),
                deployment: deployment.clone(),
                operation,
                idempotency: IdempotencyRequest::new(
                    "test.workload",
                    "create",
                    deployment.id.as_uuid().as_bytes(),
                )
                .expect("workload idempotency"),
                event,
            })
            .await
            .expect("store workload");
        let resolving = workloads
            .mark_resolving(
                deployment.id,
                deployment.aggregate_version,
                now + chrono::Duration::milliseconds(1),
            )
            .await
            .expect("resolve deployment");
        let scheduled = workloads
            .assign_node(
                deployment.id,
                resolving.aggregate_version,
                node_id,
                now + chrono::Duration::milliseconds(2),
            )
            .await
            .expect("assign deployment");

        let handler = ResolveSecretMaterialHandler::new(
            workloads.clone(),
            secrets,
            Arc::new(FixedEncryption),
        );
        let reference =
            CloudSecretReference::new(revision.id.as_uuid(), secret_id.as_uuid(), version.version)
                .expect("Secret reference");
        let plaintext = handler
            .execute(
                ResolveSecretMaterial {
                    organization_id,
                    authenticated_node_id: node_id,
                    reference,
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .expect("query framework")
            .expect("authorized material");
        assert_eq!(plaintext.as_bytes(), b"resolved-only-at-the-node-boundary");

        let applying = workloads
            .mark_dispatched(
                deployment.id,
                scheduled.aggregate_version,
                NodeCommandId::new(),
                now + chrono::Duration::milliseconds(3),
            )
            .await
            .expect("dispatch deployment");
        let verifying = workloads
            .mark_verifying(
                deployment.id,
                applying.aggregate_version,
                now + chrono::Duration::milliseconds(4),
            )
            .await
            .expect("verify deployment");
        let (_, retiring) = workloads
            .activate(
                deployment.id,
                verifying.aggregate_version,
                true,
                now + chrono::Duration::milliseconds(5),
            )
            .await
            .expect("activate deployment before retirement");
        assert_eq!(retiring.status, DeploymentStatus::Retiring);
        let retiring_plaintext = handler
            .execute(
                ResolveSecretMaterial {
                    organization_id,
                    authenticated_node_id: node_id,
                    reference,
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .expect("query framework")
            .expect("retiring active material");
        assert_eq!(
            retiring_plaintext.as_bytes(),
            b"resolved-only-at-the-node-boundary"
        );

        let unauthorized = handler
            .execute(
                ResolveSecretMaterial {
                    organization_id,
                    authenticated_node_id: NodeId::new(),
                    reference,
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .expect("query framework");
        assert!(matches!(unauthorized, Err(ApplicationError::Forbidden(_))));
    }

    fn template(secret_id: SecretId) -> ServiceTemplate {
        let digest = format!("sha256:{}", "a".repeat(64));
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!("oci://registry.example/api@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ServiceProcess {
                command: Vec::new(),
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            secrets: vec![SecretBinding {
                name: "database-password".into(),
                secret_id,
                version: 1,
                target: SecretBindingTarget::Environment {
                    variable: "DATABASE_PASSWORD".into(),
                },
            }],
            resources: ServiceResources {
                cpu_millis: 100,
                memory_bytes: 32 * 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: None,
            },
            ports: vec![ServicePort {
                name: "http".into(),
                container_port: 8080,
            }],
            health: HttpHealthCheck {
                port_name: "http".into(),
                path: "/health".into(),
                interval_ms: 1_000,
                timeout_ms: 500,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 1_000,
            },
        }
    }
}
