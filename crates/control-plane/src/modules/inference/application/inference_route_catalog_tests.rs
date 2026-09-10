//! First-principles Inference route catalog authority tests (I0.2b brick).

use crate::modules::inference::application::{
    IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope,
    PermitInferenceEdgeRouteBindingAdmission, PermitInferenceGrantCredentialAdmission,
    PublishInferenceRoute, PublishInferenceRouteHandler, RetireInferenceRoute,
    RetireInferenceRouteHandler, ReviseInferenceRoute, ReviseInferenceRouteHandler,
};
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
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
    let retire = RetireInferenceRouteHandler::new(routes.clone());
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
    let retire = RetireInferenceRouteHandler::new(routes.clone());
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
    let retire = RetireInferenceRouteHandler::new(routes.clone());
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
    let retire = RetireInferenceRouteHandler::new(routes);
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
    let retire = RetireInferenceRouteHandler::new(routes);
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
