//! First-principles Inference route catalog authority tests (I0.2b brick).

use crate::modules::inference::application::{
    IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope,
    PermitInferenceEdgeRouteBindingAdmission, PermitInferenceGrantCredentialAdmission,
    PublishInferenceRoute, PublishInferenceRouteHandler, RetireInferenceRoute,
    RetireInferenceRouteHandler, ReviseInferenceRoute, ReviseInferenceRouteHandler,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::inference::domain::repositories::IInferenceRouteRepository;
use crate::modules::inference::infrastructure::{
    InferenceRouteAclProjectionAdapter, InMemoryInferenceRouteRepository,
};
use crate::modules::inference::EmptyInferenceRouteAclProjectionPort;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayScopeId, IdempotencyRequest, IdempotentWrite,
    InferenceRouteId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    DomainEventEnvelope, InferenceEndpointAcl, InferenceGrantAclProjection,
    InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceTargetAclProjection,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

struct AlwaysPresentEnvironmentRepository;

#[async_trait]
impl IEnvironmentRepository for AlwaysPresentEnvironmentRepository {
    async fn create(
        &self,
        environment: Environment,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        Ok(IdempotentWrite {
            value: environment,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        Ok(Some(Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("default").expect("environment name"),
            Utc::now(),
        )))
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        Ok(Vec::new())
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn sample_model() -> InferenceModelAclProjection {
    InferenceModelAclProjection {
        alias: "chat-model".into(),
        model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
        targets: vec![InferenceTargetAclProjection {
            target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
            service: "model-service".into(),
            upstream_model: "internal/model-v1".into(),
            priority: 0,
            weight: 100,
        }],
    }
}

fn sample_grant() -> InferenceGrantAclProjection {
    InferenceGrantAclProjection {
        credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        credential_generation: 3,
        models: vec!["chat-model".into()],
        endpoints: vec![
            InferenceEndpointAcl::Models,
            InferenceEndpointAcl::ChatCompletions,
        ],
        limits: InferenceLimitsAclProjection {
            max_concurrent_requests: 2,
            requests_per_minute: 60,
            request_burst: 2,
            tokens_per_minute: 10_000,
        },
    }
}

fn sample_binding() -> EdgeRouteBindingRef {
    EdgeRouteBindingRef::new(
        DomainClaimId::new(),
        GatewayScopeId::new(),
        "api.example.com",
        "/v1",
        1,
    )
    .unwrap()
}

fn publish_command(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    key: &str,
) -> PublishInferenceRoute {
    PublishInferenceRoute {
        organization_id,
        project_id,
        environment_id,
        router: "inference".into(),
        models: vec![sample_model()],
        grants: vec![sample_grant()],
        binding: sample_binding(),
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now(),
    }
}

fn publish_handler(
    routes: Arc<InMemoryInferenceRouteRepository>,
) -> PublishInferenceRouteHandler {
    PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    )
}

fn revise_handler(routes: Arc<InMemoryInferenceRouteRepository>) -> ReviseInferenceRouteHandler {
    ReviseInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes,
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    )
}

fn revised_model() -> InferenceModelAclProjection {
    InferenceModelAclProjection {
        alias: "chat-model-v2".into(),
        model_id: Uuid::parse_str("77777777-7777-4777-8777-777777777777").unwrap(),
        targets: vec![InferenceTargetAclProjection {
            target_id: Uuid::parse_str("88888888-8888-4888-8888-888888888888").unwrap(),
            service: "model-service-v2".into(),
            upstream_model: "internal/model-v2".into(),
            priority: 0,
            weight: 100,
        }],
    }
}

fn revised_grant() -> InferenceGrantAclProjection {
    InferenceGrantAclProjection {
        credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        credential_generation: 3,
        models: vec!["chat-model-v2".into()],
        endpoints: vec![
            InferenceEndpointAcl::Models,
            InferenceEndpointAcl::ChatCompletions,
        ],
        limits: InferenceLimitsAclProjection {
            max_concurrent_requests: 2,
            requests_per_minute: 60,
            request_burst: 2,
            tokens_per_minute: 10_000,
        },
    }
}

#[tokio::test]
async fn publish_lists_projection_with_models_and_grants_without_workers() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let route = handler
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-1"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].route_id, route.id.as_uuid());
    assert_eq!(projections[0].models.len(), 1);
    assert_eq!(projections[0].grants.len(), 1);
    let rendered =
        a3s_cloud_contracts::render_inference_route_acl_blocks(&projections).unwrap();
    assert!(rendered.contains("models \"chat-model\""));
    assert!(rendered.contains("grants \"33333333-3333-4333-8333-333333333333\""));
    assert!(!rendered.contains("workers "));
}

#[tokio::test]
async fn empty_repository_projects_nothing_so_edge_invents_no_catalog_facts() {
    let adapter =
        InferenceRouteAclProjectionAdapter::new(Arc::new(InMemoryInferenceRouteRepository::default()));
    let scope = InferenceRouteEnvironmentScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
    )
    .unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert!(projections.is_empty());

    let empty = EmptyInferenceRouteAclProjectionPort;
    assert!(empty
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn retire_removes_route_from_projection() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let route = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-retire"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: route.id,
                expected_aggregate_version: route.aggregate_version(),
                idempotency_key: "retire-1".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    assert!(adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn publish_is_idempotent_for_same_key_and_body() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let binding = sample_binding();
    let command = PublishInferenceRoute {
        organization_id,
        project_id,
        environment_id,
        router: "inference".into(),
        models: vec![sample_model()],
        grants: vec![sample_grant()],
        binding,
        idempotency_key: "same-key".into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now(),
    };
    let first = handler
        .execute(command.clone(), context())
        .await
        .unwrap()
        .unwrap();
    let second = handler
        .execute(command, context())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.aggregate_version(), second.aggregate_version());
}

#[tokio::test]
async fn adapter_sorts_projections_stably_by_route_id() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let handler = publish_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let first = handler
        .execute(
            publish_command(organization_id, project_id, environment_id, "sort-a"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    let second = handler
        .execute(
            publish_command(organization_id, project_id, environment_id, "sort-b"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope, scope])
        .await
        .unwrap();
    assert_eq!(projections.len(), 2);
    assert!(projections[0].route_id < projections[1].route_id);
    let ids = [first.id.as_uuid(), second.id.as_uuid()];
    assert!(ids.contains(&projections[0].route_id));
    assert!(ids.contains(&projections[1].route_id));
}

#[tokio::test]
async fn revise_advances_policy_revision_and_updates_projection_without_workers() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = revise_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-revise"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let revised = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![revised_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-1".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revised.id, published.id);
    assert_eq!(revised.policy_revision(), 2);
    assert_eq!(revised.aggregate_version(), published.aggregate_version() + 1);
    assert_eq!(revised.models()[0].alias, "chat-model-v2");

    let adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].policy_revision, 2);
    assert_eq!(projections[0].models[0].alias, "chat-model-v2");
    let rendered =
        a3s_cloud_contracts::render_inference_route_acl_blocks(&projections).unwrap();
    assert!(rendered.contains("models \"chat-model-v2\""));
    assert!(!rendered.contains("workers "));
}

#[tokio::test]
async fn publish_projection_succeeds_edge_managed_snapshot_acl_without_workers() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
    };

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();

    let empty_projections = adapter
        .list_inference_route_acl_projections(&[scope.clone()])
        .await
        .unwrap();
    assert!(empty_projections.is_empty());

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .unwrap();

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let baseline = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &[credential.clone()],
            &empty_projections,
        )
        .unwrap();
    assert!(!baseline.acl.contains("models \"chat-model\""));
    assert!(!baseline.acl.contains("\n  routes "));
    assert!(!baseline.acl.contains("\n  workers "));
    assert!(baseline.acl.contains("prefix = \"a3s_inf_abc12345\""));

    let published = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-for-edge-empty-baseline",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(published.policy_revision(), 1);

    let successor_projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert_eq!(successor_projections.len(), 1);
    assert_eq!(successor_projections[0].policy_revision, 1);
    assert_eq!(successor_projections[0].models[0].alias, "chat-model");

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                3,
                Some(2),
                issued_at + Duration::seconds(1),
                expires_at + Duration::seconds(1),
            ),
            Some(certificate_id),
            &[owned],
            &[credential],
            &successor_projections,
        )
        .unwrap();
    assert!(successor.acl.contains("models \"chat-model\""));
    assert!(successor.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(successor
        .acl
        .contains("grants \"33333333-3333-4333-8333-333333333333\""));
    assert!(!successor.acl.contains("\n  workers "));
    assert_eq!(successor.acl.matches("inference {").count(), 1);
}

#[tokio::test]
async fn create_key_and_publish_route_succeed_joint_edge_managed_snapshot_acl_succession() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::identity::application::commands::create_inference_key::{
        CreateInferenceKey, CreateInferenceKeyHandler,
    };
    use crate::modules::identity::application::{
        IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
    };
    use crate::modules::identity::domain::repositories::IInferenceCredentialLifecycleRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::{
        InferenceCredentialAclProjectionAdapter, InferenceCredentialIssuer,
    };
    use crate::modules::identity::IdentityInferenceGrantCredentialAdmissionAdapter;
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;
    use sha2::{Digest, Sha256};

    struct TestEncryption;

    #[async_trait]
    impl ISecretEncryptionService for TestEncryption {
        async fn encrypt(
            &self,
            plaintext: &[u8],
            context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            let context_digest = format!("{:x}", Sha256::digest(context));
            EncryptedSecretValue::new(
                "test:base64",
                format!("v1:{context_digest}:{}", STANDARD_NO_PAD.encode(plaintext)),
            )
            .map_err(SecretEncryptionError::Rejected)
        }

        async fn decrypt(
            &self,
            value: &EncryptedSecretValue,
            context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            let mut parts = value.ciphertext().splitn(3, ':');
            let version = parts.next();
            let context_digest = parts.next();
            let encoded = parts.next();
            let expected_context_digest = format!("{:x}", Sha256::digest(context));
            if version != Some("v1") || context_digest != Some(expected_context_digest.as_str()) {
                return Err(SecretEncryptionError::Rejected(
                    "test ciphertext context mismatch".into(),
                ));
            }
            STANDARD_NO_PAD
                .decode(encoded.unwrap_or_default())
                .map_err(|error| SecretEncryptionError::Rejected(error.to_string()))
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let requested_at = Utc::now();

    let credential_adapter =
        InferenceCredentialAclProjectionAdapter::new(credentials.clone());
    let route_adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let credential_scope =
        InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
            .unwrap();
    let route_scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();

    let empty_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope.clone()])
        .await
        .unwrap();
    let empty_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope.clone()])
        .await
        .unwrap();
    assert!(empty_credentials.is_empty());
    assert!(empty_routes.is_empty());

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let baseline = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &empty_credentials,
            &empty_routes,
        )
        .unwrap();
    assert!(!baseline.acl.contains("prefix = \"a3s_inf_"));
    assert!(!baseline.acl.contains("models \"chat-model\""));
    assert!(!baseline.acl.contains("\n  routes "));
    assert!(!baseline.acl.contains("\n  workers "));

    let created = CreateInferenceKeyHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
        InferenceCredentialIssuer::new(),
        Arc::new(TestEncryption),
    )
    .execute(
        CreateInferenceKey {
            organization_id,
            project_id,
            environment_id,
            expires_at: requested_at + Duration::hours(1),
            idempotency_key: "create-for-joint-edge".into(),
            request_id: Uuid::now_v7(),
            requested_at,
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("create");
    let prefix = created
        .credential
        .gateway_projection()
        .expect("projection")
        .prefix
        .clone();
    let credential_id = created.credential.id.as_uuid();
    let credential_generation = created.credential.generation();

    let publish = PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(IdentityInferenceGrantCredentialAdmissionAdapter::new(
            credentials.clone(),
        )),
    );
    let published = publish
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![InferenceGrantAclProjection {
                    credential_id,
                    credential_generation,
                    models: vec!["chat-model".into()],
                    endpoints: vec![
                        InferenceEndpointAcl::Models,
                        InferenceEndpointAcl::ChatCompletions,
                    ],
                    limits: InferenceLimitsAclProjection {
                        max_concurrent_requests: 2,
                        requests_per_minute: 60,
                        request_burst: 2,
                        tokens_per_minute: 10_000,
                    },
                }],
                binding: sample_binding(),
                idempotency_key: "publish-for-joint-edge".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(published.policy_revision(), 1);

    let successor_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope])
        .await
        .unwrap();
    let successor_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope])
        .await
        .unwrap();
    assert_eq!(successor_credentials.len(), 1);
    assert_eq!(successor_credentials[0].credential_id, credential_id);
    assert_eq!(successor_credentials[0].prefix, prefix);
    assert_eq!(successor_credentials[0].generation, credential_generation);
    assert!(!successor_credentials[0].revoked);
    assert_eq!(successor_routes.len(), 1);
    assert_eq!(successor_routes[0].models[0].alias, "chat-model");

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                3,
                Some(2),
                issued_at + Duration::seconds(1),
                expires_at + Duration::seconds(1),
            ),
            Some(certificate_id),
            &[owned],
            &successor_credentials,
            &successor_routes,
        )
        .unwrap();
    assert!(successor.acl.contains(&format!("prefix = \"{prefix}\"")));
    assert!(successor.acl.contains("revoked = false"));
    assert!(successor.acl.contains("models \"chat-model\""));
    assert!(successor.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(successor
        .acl
        .contains(&format!("grants \"{credential_id}\"")));
    assert!(successor.acl.contains(&format!(
        "credential_generation = {credential_generation}"
    )));
    assert!(!successor.acl.contains("\n  workers "));
    assert_eq!(successor.acl.matches("inference {").count(), 1);
    assert!(
        !successor.acl.contains(created.bearer_credential()),
        "joint managed-snapshot ACL must never embed the one-time bearer secret"
    );
}

#[tokio::test]
async fn create_publish_route_then_revoke_key_succeeds_joint_edge_managed_snapshot_acl_succession() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::identity::application::commands::create_inference_key::{
        CreateInferenceKey, CreateInferenceKeyHandler,
    };
    use crate::modules::identity::application::commands::revoke_inference_key::{
        RevokeInferenceKey, RevokeInferenceKeyHandler,
    };
    use crate::modules::identity::application::{
        IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
    };
    use crate::modules::identity::domain::repositories::IInferenceCredentialLifecycleRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::{
        InferenceCredentialAclProjectionAdapter, InferenceCredentialIssuer,
    };
    use crate::modules::identity::IdentityInferenceGrantCredentialAdmissionAdapter;
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;
    use sha2::{Digest, Sha256};

    struct TestEncryption;

    #[async_trait]
    impl ISecretEncryptionService for TestEncryption {
        async fn encrypt(
            &self,
            plaintext: &[u8],
            context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            let context_digest = format!("{:x}", Sha256::digest(context));
            EncryptedSecretValue::new(
                "test:base64",
                format!("v1:{context_digest}:{}", STANDARD_NO_PAD.encode(plaintext)),
            )
            .map_err(SecretEncryptionError::Rejected)
        }

        async fn decrypt(
            &self,
            value: &EncryptedSecretValue,
            context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            let mut parts = value.ciphertext().splitn(3, ':');
            let version = parts.next();
            let context_digest = parts.next();
            let encoded = parts.next();
            let expected_context_digest = format!("{:x}", Sha256::digest(context));
            if version != Some("v1") || context_digest != Some(expected_context_digest.as_str()) {
                return Err(SecretEncryptionError::Rejected(
                    "test ciphertext context mismatch".into(),
                ));
            }
            STANDARD_NO_PAD
                .decode(encoded.unwrap_or_default())
                .map_err(|error| SecretEncryptionError::Rejected(error.to_string()))
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let requested_at = Utc::now();

    let credential_adapter = InferenceCredentialAclProjectionAdapter::new(credentials.clone());
    let route_adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let credential_scope =
        InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
            .unwrap();
    let route_scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let created = CreateInferenceKeyHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
        InferenceCredentialIssuer::new(),
        Arc::new(TestEncryption),
    )
    .execute(
        CreateInferenceKey {
            organization_id,
            project_id,
            environment_id,
            expires_at: requested_at + Duration::hours(1),
            idempotency_key: "create-for-joint-revoke-edge".into(),
            request_id: Uuid::now_v7(),
            requested_at,
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("create");
    let prefix = created
        .credential
        .gateway_projection()
        .expect("projection")
        .prefix
        .clone();
    let credential_id = created.credential.id.as_uuid();
    let credential_generation = created.credential.generation();

    let published = PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(IdentityInferenceGrantCredentialAdmissionAdapter::new(
            credentials.clone(),
        )),
    )
    .execute(
        PublishInferenceRoute {
            organization_id,
            project_id,
            environment_id,
            router: "inference".into(),
            models: vec![sample_model()],
            grants: vec![InferenceGrantAclProjection {
                credential_id,
                credential_generation,
                models: vec!["chat-model".into()],
                endpoints: vec![
                    InferenceEndpointAcl::Models,
                    InferenceEndpointAcl::ChatCompletions,
                ],
                limits: InferenceLimitsAclProjection {
                    max_concurrent_requests: 2,
                    requests_per_minute: 60,
                    request_burst: 2,
                    tokens_per_minute: 10_000,
                },
            }],
            binding: sample_binding(),
            idempotency_key: "publish-for-joint-revoke-edge".into(),
            request_id: Uuid::now_v7(),
            requested_at: Utc::now(),
        },
        context(),
    )
    .await
    .unwrap()
    .unwrap();

    let active_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope.clone()])
        .await
        .unwrap();
    let active_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope.clone()])
        .await
        .unwrap();
    let active = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &active_credentials,
            &active_routes,
        )
        .unwrap();
    assert!(active.acl.contains(&format!("prefix = \"{prefix}\"")));
    assert!(active.acl.contains("revoked = false"));
    assert!(active.acl.contains(&format!("grants \"{credential_id}\"")));
    assert!(active.acl.contains(&format!(
        "credential_generation = {credential_generation}"
    )));
    assert!(!active.acl.contains("\n  workers "));

    let revoked = RevokeInferenceKeyHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
    )
    .execute(
        RevokeInferenceKey {
            organization_id,
            project_id,
            environment_id,
            credential_id: created.credential.id,
            expected_aggregate_version: created.credential.aggregate_version(),
            idempotency_key: "revoke-for-joint-edge".into(),
            request_id: Uuid::now_v7(),
            requested_at: created.credential.updated_at() + Duration::seconds(1),
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("revoke");
    assert!(revoked
        .credential
        .gateway_projection()
        .expect("projection")
        .revoked);

    let successor_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope])
        .await
        .unwrap();
    let successor_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope])
        .await
        .unwrap();
    assert_eq!(successor_credentials.len(), 1);
    assert!(successor_credentials[0].revoked);
    assert_eq!(successor_credentials[0].prefix, prefix);
    assert_eq!(successor_credentials[0].generation, credential_generation);
    assert_eq!(successor_routes.len(), 1);
    assert_eq!(successor_routes[0].grants.len(), 1);
    assert_eq!(
        successor_routes[0].grants[0].credential_id,
        credential_id,
        "revoke must not silently rewrite Inference route grants"
    );
    assert_eq!(
        successor_routes[0].grants[0].credential_generation,
        credential_generation
    );
    assert_eq!(successor_routes[0].models[0].alias, "chat-model");

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                3,
                Some(2),
                issued_at + Duration::seconds(1),
                expires_at + Duration::seconds(1),
            ),
            Some(certificate_id),
            &[owned],
            &successor_credentials,
            &successor_routes,
        )
        .unwrap();
    assert!(successor.acl.contains(&format!("prefix = \"{prefix}\"")));
    assert!(successor.acl.contains("revoked = true"));
    assert!(!successor.acl.contains("revoked = false"));
    assert!(successor.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(successor.acl.contains("models \"chat-model\""));
    assert!(successor
        .acl
        .contains(&format!("grants \"{credential_id}\"")));
    assert!(successor.acl.contains(&format!(
        "credential_generation = {credential_generation}"
    )));
    assert!(!successor.acl.contains("\n  workers "));
    assert_eq!(successor.acl.matches("inference {").count(), 1);
    assert!(
        !successor.acl.contains(created.bearer_credential()),
        "joint revoke successor ACL must never embed the bearer secret"
    );
}

#[tokio::test]
async fn expired_credential_with_published_route_succeeds_joint_edge_managed_snapshot_acl() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
    };

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PREFIX: &str = "a3s_inf_abc12345";

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish_handler(routes.clone())
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-for-expired-joint-edge",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let route_adapter = InferenceRouteAclProjectionAdapter::new(routes);
    let route_scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let route_projections = route_adapter
        .list_inference_route_acl_projections(&[route_scope])
        .await
        .unwrap();
    assert_eq!(route_projections.len(), 1);
    assert_eq!(route_projections[0].grants.len(), 1);

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let snapshot_expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let credential_expires_at = issued_at - Duration::hours(1);
    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        PREFIX,
        VERIFIER,
        3,
        credential_expires_at,
        false,
    )
    .unwrap();
    assert!(credential.expires_at < issued_at);
    assert!(!credential.revoked);

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let snapshot = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, snapshot_expires_at),
            Some(certificate_id),
            &[owned],
            &[credential],
            &route_projections,
        )
        .unwrap();
    let credential_expires_acl = credential_expires_at.to_rfc3339_opts(
        chrono::SecondsFormat::Micros,
        true,
    );
    assert!(snapshot
        .acl
        .contains(&format!("prefix = \"{PREFIX}\"")));
    assert!(snapshot.acl.contains(&format!(
        "expires_at = \"{credential_expires_acl}\""
    )));
    assert!(snapshot.acl.contains("revoked = false"));
    assert!(!snapshot.acl.contains("revoked = true"));
    assert!(snapshot.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(snapshot.acl.contains("models \"chat-model\""));
    assert!(snapshot
        .acl
        .contains("grants \"33333333-3333-4333-8333-333333333333\""));
    assert!(snapshot.acl.contains("credential_generation = 3"));
    assert!(!snapshot.acl.contains("\n  workers "));
    assert_eq!(snapshot.acl.matches("inference {").count(), 1);
}

#[tokio::test]
async fn rotate_then_revise_route_bumps_grant_generation_in_edge_managed_snapshot_acl_succession() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::identity::application::commands::create_inference_key::{
        CreateInferenceKey, CreateInferenceKeyHandler,
    };
    use crate::modules::identity::application::commands::rotate_inference_key::{
        RotateInferenceKey, RotateInferenceKeyHandler,
    };
    use crate::modules::identity::application::{
        IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
    };
    use crate::modules::identity::domain::repositories::IInferenceCredentialLifecycleRepository;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::identity::infrastructure::{
        InferenceCredentialAclProjectionAdapter, InferenceCredentialIssuer,
    };
    use crate::modules::identity::IdentityInferenceGrantCredentialAdmissionAdapter;
    use crate::modules::secrets::domain::{
        EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine as _;
    use sha2::{Digest, Sha256};

    struct TestEncryption;

    #[async_trait]
    impl ISecretEncryptionService for TestEncryption {
        async fn encrypt(
            &self,
            plaintext: &[u8],
            context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            let context_digest = format!("{:x}", Sha256::digest(context));
            EncryptedSecretValue::new(
                "test:base64",
                format!("v1:{context_digest}:{}", STANDARD_NO_PAD.encode(plaintext)),
            )
            .map_err(SecretEncryptionError::Rejected)
        }

        async fn decrypt(
            &self,
            value: &EncryptedSecretValue,
            context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            let mut parts = value.ciphertext().splitn(3, ':');
            let version = parts.next();
            let context_digest = parts.next();
            let encoded = parts.next();
            let expected_context_digest = format!("{:x}", Sha256::digest(context));
            if version != Some("v1") || context_digest != Some(expected_context_digest.as_str()) {
                return Err(SecretEncryptionError::Rejected(
                    "test ciphertext context mismatch".into(),
                ));
            }
            STANDARD_NO_PAD
                .decode(encoded.unwrap_or_default())
                .map_err(|error| SecretEncryptionError::Rejected(error.to_string()))
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let requested_at = Utc::now();

    let credential_adapter = InferenceCredentialAclProjectionAdapter::new(credentials.clone());
    let route_adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let credential_scope =
        InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
            .unwrap();
    let route_scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let created = CreateInferenceKeyHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
        InferenceCredentialIssuer::new(),
        Arc::new(TestEncryption),
    )
    .execute(
        CreateInferenceKey {
            organization_id,
            project_id,
            environment_id,
            expires_at: requested_at + Duration::hours(1),
            idempotency_key: "create-for-rotate-revise-edge".into(),
            request_id: Uuid::now_v7(),
            requested_at,
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("create");
    let prior_prefix = created
        .credential
        .gateway_projection()
        .expect("projection")
        .prefix
        .clone();
    let credential_id = created.credential.id.as_uuid();
    let prior_generation = created.credential.generation();
    assert_eq!(prior_generation, 1);

    let grant_admission = Arc::new(IdentityInferenceGrantCredentialAdmissionAdapter::new(
        credentials.clone(),
    ));
    let publish = PublishInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::clone(&grant_admission)
            as Arc<dyn crate::modules::inference::application::IInferenceGrantCredentialAdmissionPort>,
    );
    let published = publish
        .execute(
            PublishInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![InferenceGrantAclProjection {
                    credential_id,
                    credential_generation: prior_generation,
                    models: vec!["chat-model".into()],
                    endpoints: vec![
                        InferenceEndpointAcl::Models,
                        InferenceEndpointAcl::ChatCompletions,
                    ],
                    limits: InferenceLimitsAclProjection {
                        max_concurrent_requests: 2,
                        requests_per_minute: 60,
                        request_burst: 2,
                        tokens_per_minute: 10_000,
                    },
                }],
                binding: sample_binding(),
                idempotency_key: "publish-for-rotate-revise-edge".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let baseline_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope.clone()])
        .await
        .unwrap();
    let baseline_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope.clone()])
        .await
        .unwrap();
    let baseline = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &baseline_credentials,
            &baseline_routes,
        )
        .unwrap();
    assert!(baseline.acl.contains(&format!("prefix = \"{prior_prefix}\"")));
    assert!(baseline.acl.contains("generation = 1"));
    assert!(baseline.acl.contains("credential_generation = 1"));
    assert!(baseline.acl.contains(&format!("grants \"{credential_id}\"")));
    assert!(!baseline.acl.contains("\n  workers "));

    let rotate_at = Utc::now() + Duration::seconds(1);
    let rotated = RotateInferenceKeyHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        Arc::clone(&credentials) as Arc<dyn IInferenceCredentialLifecycleRepository>,
        InferenceCredentialIssuer::new(),
        Arc::new(TestEncryption),
    )
    .execute(
        RotateInferenceKey {
            organization_id,
            project_id,
            environment_id,
            credential_id: created.credential.id,
            expected_aggregate_version: created.credential.aggregate_version(),
            expires_at: rotate_at + Duration::hours(2),
            idempotency_key: "rotate-for-revise-edge".into(),
            request_id: Uuid::now_v7(),
            requested_at: rotate_at,
        },
        context(),
    )
    .await
    .expect("boot")
    .expect("rotate");
    assert_eq!(rotated.credential.generation(), 2);
    let new_prefix = rotated
        .credential
        .gateway_projection()
        .expect("projection")
        .prefix
        .clone();
    assert_ne!(new_prefix, prior_prefix);

    let rotated_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope.clone()])
        .await
        .unwrap();
    let stale_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope.clone()])
        .await
        .unwrap();
    assert_eq!(rotated_credentials[0].generation, 2);
    assert_eq!(stale_routes[0].grants[0].credential_generation, 1);
    let mismatched = compiler.compile_certificate_convergence_with_inference_policy(
        GatewaySnapshotMetadata::new(
            node_id,
            3,
            Some(2),
            issued_at + Duration::seconds(1),
            expires_at + Duration::seconds(1),
        ),
        Some(certificate_id),
        &[owned.clone()],
        &rotated_credentials,
        &stale_routes,
    );
    let mismatch_error = mismatched.expect_err(
        "rotated credential generation must fail closed against stale grant generation",
    );
    assert!(
        mismatch_error.contains("credential_generation")
            && mismatch_error.contains("does not match"),
        "unexpected compile error: {mismatch_error}"
    );

    let revise = ReviseInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        grant_admission,
    );
    let revised = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![InferenceGrantAclProjection {
                    credential_id,
                    credential_generation: 2,
                    models: vec!["chat-model".into()],
                    endpoints: vec![
                        InferenceEndpointAcl::Models,
                        InferenceEndpointAcl::ChatCompletions,
                    ],
                    limits: InferenceLimitsAclProjection {
                        max_concurrent_requests: 2,
                        requests_per_minute: 60,
                        request_burst: 2,
                        tokens_per_minute: 10_000,
                    },
                }],
                binding: sample_binding(),
                idempotency_key: "revise-grant-generation-for-edge".into(),
                request_id: Uuid::now_v7(),
                requested_at: rotate_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revised.policy_revision(), 2);

    let successor_credentials = credential_adapter
        .list_inference_credential_acl_projections(&[credential_scope])
        .await
        .unwrap();
    let successor_routes = route_adapter
        .list_inference_route_acl_projections(&[route_scope])
        .await
        .unwrap();
    assert_eq!(successor_credentials[0].generation, 2);
    assert_eq!(successor_credentials[0].prefix, new_prefix);
    assert!(!successor_credentials[0].revoked);
    assert_eq!(successor_routes[0].grants[0].credential_generation, 2);

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                4,
                Some(3),
                issued_at + Duration::seconds(2),
                expires_at + Duration::seconds(2),
            ),
            Some(certificate_id),
            &[owned],
            &successor_credentials,
            &successor_routes,
        )
        .unwrap();
    assert!(successor.acl.contains(&format!("prefix = \"{new_prefix}\"")));
    assert!(!successor.acl.contains(&format!("prefix = \"{prior_prefix}\"")));
    assert!(successor.acl.contains("generation = 2"));
    assert!(successor.acl.contains("credential_generation = 2"));
    assert!(!successor.acl.contains("credential_generation = 1"));
    assert!(successor.acl.contains(&format!("grants \"{credential_id}\"")));
    assert!(successor.acl.contains("models \"chat-model\""));
    assert!(successor.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(successor.acl.contains("revoked = false"));
    assert!(!successor.acl.contains("\n  workers "));
    assert_eq!(successor.acl.matches("inference {").count(), 1);
    assert!(
        !successor.acl.contains(created.bearer_credential()),
        "successor ACL must not embed the pre-rotate bearer"
    );
    assert!(
        !successor.acl.contains(rotated.bearer_credential()),
        "successor ACL must not embed the rotated bearer"
    );
}

#[tokio::test]
async fn revise_projection_succeeds_edge_managed_snapshot_acl_without_workers() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
    };

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = revise_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-for-edge"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let baseline_projections = adapter
        .list_inference_route_acl_projections(&[scope.clone()])
        .await
        .unwrap();
    assert_eq!(baseline_projections.len(), 1);
    assert_eq!(baseline_projections[0].policy_revision, 1);
    assert_eq!(baseline_projections[0].models[0].alias, "chat-model");

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .unwrap();

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let baseline = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &[credential.clone()],
            &baseline_projections,
        )
        .unwrap();
    assert!(baseline.acl.contains("models \"chat-model\""));
    assert!(baseline.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(!baseline.acl.contains("models \"chat-model-v2\""));
    assert!(!baseline.acl.contains("\n  workers "));

    let revised = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![revised_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-for-edge".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revised.policy_revision(), 2);

    let successor_projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert_eq!(successor_projections.len(), 1);
    assert_eq!(successor_projections[0].policy_revision, 2);
    assert_eq!(successor_projections[0].models[0].alias, "chat-model-v2");

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                3,
                Some(2),
                issued_at + Duration::seconds(1),
                expires_at + Duration::seconds(1),
            ),
            Some(certificate_id),
            &[owned],
            &[credential],
            &successor_projections,
        )
        .unwrap();
    assert!(successor.acl.contains("models \"chat-model-v2\""));
    assert!(successor.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(!successor.acl.contains("models \"chat-model\""));
    assert!(!successor.acl.contains("targets \"66666666-6666-4666-8666-666666666666\""));
    assert!(successor
        .acl
        .contains("targets \"88888888-8888-4888-8888-888888888888\""));
    assert!(!successor.acl.contains("\n  workers "));
    assert_eq!(successor.acl.matches("inference {").count(), 1);
}

#[tokio::test]
async fn revise_withdraws_grants_succeeds_edge_managed_snapshot_acl_without_revoking_credential() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
    };

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const CREDENTIAL_ID: &str = "33333333-3333-4333-8333-333333333333";
    const PREFIX: &str = "a3s_inf_abc12345";

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = revise_handler(routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-for-grant-withdrawal",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let baseline_projections = adapter
        .list_inference_route_acl_projections(&[scope.clone()])
        .await
        .unwrap();
    assert_eq!(baseline_projections.len(), 1);
    assert_eq!(baseline_projections[0].grants.len(), 1);
    assert_eq!(
        baseline_projections[0].grants[0].credential_id.to_string(),
        CREDENTIAL_ID
    );

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str(CREDENTIAL_ID).unwrap(),
        environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        PREFIX,
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .unwrap();

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let baseline = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &[credential.clone()],
            &baseline_projections,
        )
        .unwrap();
    assert!(baseline.acl.contains(&format!("grants \"{CREDENTIAL_ID}\"")));
    assert!(baseline.acl.contains(&format!("prefix = \"{PREFIX}\"")));
    assert!(baseline.acl.contains("revoked = false"));
    assert!(baseline.acl.contains("generation = 3"));
    assert!(!baseline.acl.contains("revoked = true"));
    assert!(!baseline.acl.contains("\n  workers "));

    let revised = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![sample_model()],
                grants: vec![],
                binding: sample_binding(),
                idempotency_key: "revise-withdraw-grants-for-edge".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revised.policy_revision(), 2);

    let successor_projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert_eq!(successor_projections.len(), 1);
    assert_eq!(successor_projections[0].policy_revision, 2);
    assert!(
        successor_projections[0].grants.is_empty(),
        "grant withdrawal must clear route grants while keeping the route"
    );
    assert_eq!(successor_projections[0].models[0].alias, "chat-model");

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                3,
                Some(2),
                issued_at + Duration::seconds(1),
                expires_at + Duration::seconds(1),
            ),
            Some(certificate_id),
            &[owned],
            &[credential],
            &successor_projections,
        )
        .unwrap();
    assert!(successor.acl.contains(&format!(
        "routes \"{}\"",
        published.id.as_uuid()
    )));
    assert!(successor.acl.contains("models \"chat-model\""));
    assert!(
        !successor.acl.contains(&format!("grants \"{CREDENTIAL_ID}\"")),
        "successor ACL must withdraw grants without removing the authenticatable credential"
    );
    assert!(successor.acl.contains(&format!("prefix = \"{PREFIX}\"")));
    assert!(successor.acl.contains("generation = 3"));
    assert!(successor.acl.contains("revoked = false"));
    assert!(!successor.acl.contains("revoked = true"));
    assert!(!successor.acl.contains("\n  workers "));
    assert_eq!(successor.acl.matches("inference {").count(), 1);
}

#[tokio::test]
async fn retire_projection_omits_route_from_edge_managed_snapshot_acl() {
    use crate::modules::edge::domain::{
        DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
        UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata,
    };
    use crate::modules::shared_kernel::domain::{
        GatewayCertificateId, NodeId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
    };

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-for-retire-edge"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let adapter = InferenceRouteAclProjectionAdapter::new(routes.clone());
    let scope =
        InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id).unwrap();
    let baseline_projections = adapter
        .list_inference_route_acl_projections(&[scope.clone()])
        .await
        .unwrap();
    assert_eq!(baseline_projections.len(), 1);

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let now = issued_at;
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let mut owned = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse("api.example.com").unwrap(),
        RoutePath::parse("/v1").unwrap(),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").unwrap(),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").unwrap(),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").unwrap(),
            now,
        )
        .unwrap(),
        now,
    )
    .unwrap();
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);

    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .unwrap();

    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .unwrap();

    let route_acl_id = format!("routes \"{}\"", published.id.as_uuid());
    let baseline = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned.clone()],
            &[credential.clone()],
            &baseline_projections,
        )
        .unwrap();
    assert!(baseline.acl.contains(&route_acl_id));
    assert!(baseline.acl.contains("models \"chat-model\""));

    retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                idempotency_key: "retire-for-edge".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let successor_projections = adapter
        .list_inference_route_acl_projections(&[scope])
        .await
        .unwrap();
    assert!(
        successor_projections.is_empty(),
        "retired routes must leave the Edge ACL projection empty"
    );

    let successor = compiler
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(
                node_id,
                3,
                Some(2),
                issued_at + Duration::seconds(1),
                expires_at + Duration::seconds(1),
            ),
            Some(certificate_id),
            &[owned],
            &[credential],
            &successor_projections,
        )
        .unwrap();
    assert!(
        !successor.acl.contains(&route_acl_id),
        "successor managed snapshot must omit the retired inference route"
    );
    assert!(!successor.acl.contains("models \"chat-model\""));
    assert!(!successor.acl.contains("\n  routes "));
    assert!(!successor.acl.contains("\n  workers "));
    assert!(successor.acl.contains("prefix = \"a3s_inf_abc12345\""));
}

#[tokio::test]
async fn revise_retired_route_is_rejected() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes.clone());
    let revise = revise_handler(routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-then-retire"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                idempotency_key: "retire-before-revise".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let error = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version() + 1,
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![sample_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-retired".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(2),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn revise_missing_route_fails_closed_as_not_found() {
    let revise = revise_handler(Arc::new(InMemoryInferenceRouteRepository::default()));
    let error = revise
        .execute(
            ReviseInferenceRoute {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                route_id: InferenceRouteId::new(),
                expected_aggregate_version: 1,
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![sample_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-missing".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn revise_stale_and_zero_aggregate_version_fail_closed() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = revise_handler(routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-cas"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let zero = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: 0,
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![sample_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-zero".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(zero, ApplicationError::Invalid(_)));

    let stale = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version() + 1,
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![sample_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-stale".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(stale, ApplicationError::Conflict(_)));
}

#[tokio::test]
async fn revise_is_idempotent_for_same_key_and_body() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = revise_handler(routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-idempotent"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    let command = ReviseInferenceRoute {
        organization_id,
        project_id,
        environment_id,
        route_id: published.id,
        expected_aggregate_version: published.aggregate_version(),
        router: "inference".into(),
        models: vec![revised_model()],
        grants: vec![revised_grant()],
        binding: sample_binding(),
        idempotency_key: "revise-same".into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now() + Duration::seconds(1),
    };
    let first = revise
        .execute(command.clone(), context())
        .await
        .unwrap()
        .unwrap();
    let second = revise.execute(command, context()).await.unwrap().unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.policy_revision(), second.policy_revision());
    assert_eq!(first.aggregate_version(), second.aggregate_version());
}

#[tokio::test]
async fn retire_stale_and_zero_aggregate_version_fail_closed() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-retire-cas"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let zero = retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: 0,
                idempotency_key: "retire-zero".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(zero, ApplicationError::Invalid(_)));

    let stale = retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version() + 1,
                idempotency_key: "retire-stale".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(stale, ApplicationError::Conflict(_)));
}

#[tokio::test]
async fn retire_is_idempotent_for_same_key_and_cas() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), routes);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(organization_id, project_id, environment_id, "publish-retire-idem"),
            context(),
        )
        .await
        .unwrap()
        .unwrap();
    let command = RetireInferenceRoute {
        organization_id,
                project_id,
                environment_id,
        route_id: published.id,
        expected_aggregate_version: published.aggregate_version(),
        idempotency_key: "retire-same".into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now() + Duration::seconds(1),
    };
    let first = retire
        .execute(command.clone(), context())
        .await
        .unwrap()
        .unwrap();
    let second = retire.execute(command, context()).await.unwrap().unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.aggregate_version(), second.aggregate_version());
    assert!(first.retired_at().is_some());
    assert_eq!(first.retired_at(), second.retired_at());
}

#[tokio::test]
async fn retire_rejects_wrong_environment_path_as_not_found() {
    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire = RetireInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
    );
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let other_environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-retire-wrong-env",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let denied = retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id: other_environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                idempotency_key: "retire-wrong-env".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(denied, ApplicationError::NotFound(_)));

    let still_live = routes
        .find_inference_route(organization_id, published.id)
        .await
        .unwrap()
        .expect("route must remain unretired");
    assert!(still_live.retired_at().is_none());
    assert_eq!(still_live.aggregate_version(), published.aggregate_version());
}

#[tokio::test]
async fn retire_rejects_missing_environment_as_not_found() {
    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
        async fn create(
            &self,
            environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Ok(IdempotentWrite {
                value: environment,
                replayed: false,
            })
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let retire =
        RetireInferenceRouteHandler::new(Arc::new(MissingEnvironmentRepository), routes.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-retire-missing-env",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let denied = retire
        .execute(
            RetireInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                idempotency_key: "retire-missing-env".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(denied, ApplicationError::NotFound(_)));
    assert!(routes
        .find_inference_route(organization_id, published.id)
        .await
        .unwrap()
        .expect("route")
        .retired_at()
        .is_none());
}

#[tokio::test]
async fn revise_rejects_wrong_environment_path_as_not_found_before_admission() {
    use crate::modules::inference::application::{
        IInferenceEdgeRouteBindingAdmissionPort, InferenceEdgeRouteBindingAdmissionRequest,
    };
    use crate::modules::shared_kernel::application::ApplicationResult;

    struct RejectBindingAdmission;

    #[async_trait]
    impl IInferenceEdgeRouteBindingAdmissionPort for RejectBindingAdmission {
        async fn admit(
            &self,
            _request: InferenceEdgeRouteBindingAdmissionRequest,
        ) -> ApplicationResult<()> {
            Err(ApplicationError::Invalid(
                "binding admission must not run before path-scope resolution".into(),
            ))
        }
    }

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = ReviseInferenceRouteHandler::new(
        Arc::new(AlwaysPresentEnvironmentRepository),
        routes.clone(),
        Arc::new(RejectBindingAdmission),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    );
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let other_environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-revise-wrong-env",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let denied = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id: other_environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![sample_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-wrong-env".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(denied, ApplicationError::NotFound(_)),
        "wrong-environment revise must NotFound before binding admission, got {denied:?}"
    );

    let still_live = routes
        .find_inference_route(organization_id, published.id)
        .await
        .unwrap()
        .expect("route must remain unrevised");
    assert_eq!(still_live.aggregate_version(), published.aggregate_version());
    assert_eq!(still_live.policy_revision(), published.policy_revision());
}

#[tokio::test]
async fn revise_rejects_missing_environment_as_not_found() {
    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
        async fn create(
            &self,
            environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Ok(IdempotentWrite {
                value: environment,
                replayed: false,
            })
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = publish_handler(routes.clone());
    let revise = ReviseInferenceRouteHandler::new(
        Arc::new(MissingEnvironmentRepository),
        routes.clone(),
        Arc::new(PermitInferenceEdgeRouteBindingAdmission),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    );
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let published = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-revise-missing-env",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap();

    let denied = revise
        .execute(
            ReviseInferenceRoute {
                organization_id,
                project_id,
                environment_id,
                route_id: published.id,
                expected_aggregate_version: published.aggregate_version(),
                router: "inference".into(),
                models: vec![revised_model()],
                grants: vec![sample_grant()],
                binding: sample_binding(),
                idempotency_key: "revise-missing-env".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now() + Duration::seconds(1),
            },
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(denied, ApplicationError::NotFound(_)));
    let still_live = routes
        .find_inference_route(organization_id, published.id)
        .await
        .unwrap()
        .expect("route");
    assert_eq!(still_live.aggregate_version(), published.aggregate_version());
}

#[tokio::test]
async fn publish_rejects_missing_environment_as_not_found_before_admission() {
    use crate::modules::inference::application::{
        IInferenceEdgeRouteBindingAdmissionPort, InferenceEdgeRouteBindingAdmissionRequest,
    };
    use crate::modules::shared_kernel::application::ApplicationResult;

    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
        async fn create(
            &self,
            environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Ok(IdempotentWrite {
                value: environment,
                replayed: false,
            })
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    struct RejectBindingAdmission;

    #[async_trait]
    impl IInferenceEdgeRouteBindingAdmissionPort for RejectBindingAdmission {
        async fn admit(
            &self,
            _request: InferenceEdgeRouteBindingAdmissionRequest,
        ) -> ApplicationResult<()> {
            Err(ApplicationError::Invalid(
                "binding admission must not run before environment resolution".into(),
            ))
        }
    }

    let routes = Arc::new(InMemoryInferenceRouteRepository::default());
    let publish = PublishInferenceRouteHandler::new(
        Arc::new(MissingEnvironmentRepository),
        routes.clone(),
        Arc::new(RejectBindingAdmission),
        Arc::new(PermitInferenceGrantCredentialAdmission),
    );
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();

    let denied = publish
        .execute(
            publish_command(
                organization_id,
                project_id,
                environment_id,
                "publish-missing-env",
            ),
            context(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(denied, ApplicationError::NotFound(_)),
        "missing environment publish must NotFound before binding admission, got {denied:?}"
    );
    assert!(
        routes
            .list_inference_routes_by_environment(organization_id, project_id, environment_id)
            .await
            .unwrap()
            .is_empty(),
        "failed publish must not persist a route"
    );
}
