use super::ResolveSecretMaterial;
use crate::modules::secrets::application::{
    ExactSecretMaterializer, ISecretMaterializationAuthorizer,
    SecretMaterializationAuthorizationError, SecretMaterializationAuthorizationRequest,
    SecretPlaintext,
};
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{SecretId, WorkloadRevisionId};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ResolveSecretMaterialHandler {
    authorizer: Arc<dyn ISecretMaterializationAuthorizer>,
    materializer: ExactSecretMaterializer,
}

impl ResolveSecretMaterialHandler {
    pub fn new(
        authorizer: Arc<dyn ISecretMaterializationAuthorizer>,
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            authorizer,
            materializer: ExactSecretMaterializer::new(secrets, encryption),
        }
    }
}

impl QueryHandler<ResolveSecretMaterial> for ResolveSecretMaterialHandler {
    fn execute(
        &self,
        query: ResolveSecretMaterial,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SecretPlaintext>>> {
        let authorizer = Arc::clone(&self.authorizer);
        let materializer = self.materializer.clone();
        Box::pin(async move {
            if let Err(error) = query.reference.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let revision_id = WorkloadRevisionId::from_uuid(query.reference.workload_revision_id);
            let secret_id = SecretId::from_uuid(query.reference.secret_id);
            let request = match SecretMaterializationAuthorizationRequest::new(
                query.organization_id,
                query.authenticated_node_id,
                revision_id,
                secret_id,
                query.reference.version,
            ) {
                Ok(request) => request,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let authorization = match authorizer.authorize(request).await {
                Ok(authorization) => authorization,
                Err(SecretMaterializationAuthorizationError::Forbidden) => {
                    return Ok(Err(not_authorized()))
                }
                Err(SecretMaterializationAuthorizationError::Unavailable(_)) => {
                    return Ok(Err(ApplicationError::Unavailable(
                        "Secret materialization authorization is unavailable".into(),
                    )))
                }
            };
            if authorization.validate_for(&request).is_err() {
                return Ok(Err(ApplicationError::Unavailable(
                    "Secret materialization authorization is inconsistent".into(),
                )));
            }
            let plaintext = match materializer
                .materialize(
                    authorization.organization_id(),
                    authorization.project_id(),
                    authorization.environment_id(),
                    authorization.secret_id(),
                    authorization.secret_version(),
                )
                .await
            {
                Ok(value) => value,
                Err(ApplicationError::Forbidden(_)) => return Ok(Err(not_authorized())),
                Err(error) => return Ok(Err(error)),
            };
            Ok(Ok(plaintext))
        })
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
    use crate::modules::secrets::infrastructure::WorkloadsSecretMaterializationAuthorizerAdapter;
    use crate::modules::shared_kernel::domain::{
        DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId,
        OrganizationId, ProjectId, ResourceName, SecretId, WorkloadId, WorkloadRevisionId,
    };
    use crate::modules::workloads::domain::entities::{
        Deployment, DeploymentStatus, HttpHealthCheck, OciArtifact, SecretBinding,
        SecretBindingTarget, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
        Workload, WorkloadRevision,
    };
    use crate::modules::workloads::domain::events::DeploymentRequested;
    use crate::modules::workloads::domain::repositories::{
        CreateDeploymentBundle, IWorkloadRepository,
    };
    use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
    use crate::modules::workloads::{
        IWorkloadSecretMaterializationAuthorizationQueryPort,
        WorkloadSecretMaterializationAuthorizationQueryService,
    };
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
                control: crate::modules::workloads::domain::entities::WorkloadControlSpec::unmanaged_single_replica(),
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

        let workload_authorization: Arc<dyn IWorkloadSecretMaterializationAuthorizationQueryPort> =
            Arc::new(WorkloadSecretMaterializationAuthorizationQueryService::new(
                workloads.clone(),
            ));
        let authorizer: Arc<dyn ISecretMaterializationAuthorizer> = Arc::new(
            WorkloadsSecretMaterializationAuthorizerAdapter::new(workload_authorization),
        );
        let handler =
            ResolveSecretMaterialHandler::new(authorizer, secrets, Arc::new(FixedEncryption));
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
            health: Some(HttpHealthCheck {
                port_name: "http".into(),
                path: "/health".into(),
                interval_ms: 1_000,
                timeout_ms: 500,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 1_000,
            }),
        }
    }
}
