use super::*;
use crate::modules::edge::domain::{
    DomainNamePattern, RouteHostname, RoutePath, RoutePortName, RouteTarget, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, OrganizationId, ProjectId,
    RouteId, WorkloadId, WorkloadRevisionId,
};
use chrono::{Duration, Utc};

fn compiler() -> GatewaySnapshotCompiler {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .expect("compiler")
}

fn route(node_id: NodeId, hostname: &str, path: &str, port: u16) -> Route {
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let now = Utc::now();
    Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse(hostname).expect("hostname"),
        RoutePath::parse(path).expect("path"),
        DomainClaimId::new(),
        DomainNamePattern::parse(hostname).expect("domain pattern"),
        GatewayCertificateId::new(),
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).expect("upstream"),
            now,
        )
        .expect("target"),
        now,
    )
    .expect("route")
}

#[test]
fn compiles_every_owned_route_into_one_deterministic_snapshot() {
    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut first = route(node_id, "z.example.com", "/", 49152);
    first.state = RouteState::Active;
    let mut second = route(node_id, "api.example.com", "/v1", 49153);
    second.gateway_certificate_id = Some(certificate_id);
    let expected_targets = [
        (
            first.target.workload_revision_id,
            first.target.runtime_unit_id.clone(),
            first.target.runtime_generation,
        ),
        (
            second.target.workload_revision_id,
            second.target.runtime_unit_id.clone(),
            second.target.runtime_generation,
        ),
    ];
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let forward = compiler()
        .compile(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            certificate_id,
            &[first.clone(), second.clone()],
        )
        .expect("snapshot");
    let reverse = compiler()
        .compile(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            certificate_id,
            &[second, first],
        )
        .expect("snapshot");
    assert_eq!(forward, reverse);
    assert_eq!(forward.acl.matches("routers \"").count(), 2);
    assert_eq!(forward.acl.matches("services \"").count(), 2);
    assert!(forward
        .acl
        .contains("Host(`api.example.com`) && PathPrefix(`/v1`)"));
    assert!(forward.acl.contains("http://127.0.0.1:49152/"));
    assert!(forward.acl.contains("mode { kind = \"cloud-managed\" }"));
    assert!(forward.acl.contains(&node_id.to_string()));
    assert_eq!(forward.acl.matches("target = {").count(), 2);
    for (target_id, unit_id, generation) in expected_targets {
        assert!(forward
            .acl
            .contains(&format!("target_id = \"{target_id}\"")));
        assert!(forward.acl.contains(&format!("unit_id = \"{unit_id}\"")));
        assert!(forward.acl.contains(&format!("generation = {generation}")));
    }
    assert_inference_policy_shell(&forward.acl, expires_at);
}

#[test]
fn compiles_certificate_convergence_without_mutating_active_routes() {
    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut active = route(node_id, "api.example.com", "/", 49152);
    let previous_certificate_id = active.gateway_certificate_id.expect("previous certificate");
    active.state = RouteState::Active;
    let issued_at = Utc::now();

    let snapshot = compiler()
        .compile_certificate_convergence(
            GatewaySnapshotMetadata::new(
                node_id,
                2,
                Some(1),
                issued_at,
                issued_at + Duration::minutes(10),
            ),
            Some(certificate_id),
            std::slice::from_ref(&active),
        )
        .expect("certificate convergence snapshot");

    assert_eq!(
        active.gateway_certificate_id,
        Some(previous_certificate_id),
        "the replacement is not authoritative before acknowledgement"
    );
    assert_eq!(
        snapshot
            .certificate_request
            .as_ref()
            .map(|request| request.certificate_id),
        Some(certificate_id.as_uuid())
    );
    assert!(snapshot.acl.contains("api.example.com"));
}

#[test]
fn compiles_route_less_revocation_snapshot_without_a_certificate() {
    let node_id = NodeId::new();
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let snapshot = compiler()
        .compile_certificate_convergence(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            None,
            &[],
        )
        .expect("route-less revocation snapshot");

    assert!(snapshot.certificate_request.is_none());
    assert!(!snapshot.acl.contains("entrypoints \"a3s-cloud-https\""));
    assert!(snapshot.acl.contains("management {"));
    assert_inference_policy_shell(&snapshot.acl, expires_at);
}

#[test]
fn projects_identity_inference_credentials_into_managed_snapshot_acl() {
    use crate::modules::shared_kernel::domain::canonical_timestamp;
    use a3s_cloud_contracts::{
        render_inference_policy_acl, InferenceCredentialAclProjection,
        INFERENCE_CREDENTIAL_AUDIENCE,
    };
    use uuid::Uuid;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut owned = route(node_id, "api.example.com", "/v1", 49152);
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        7,
        expires_at + Duration::hours(1),
        false,
    )
    .expect("credential projection");
    let revoked = InferenceCredentialAclProjection::new(
        Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
        Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_revoked001",
        VERIFIER,
        1,
        expires_at + Duration::hours(1),
        true,
    )
    .expect("revoked credential projection");

    let snapshot = compiler()
        .compile_certificate_convergence_with_inference_credentials(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned],
            &[credential.clone(), revoked.clone()],
        )
        .expect("credential-aware snapshot");

    let expected =
        render_inference_policy_acl(canonical_timestamp(expires_at), &[credential, revoked])
            .expect("expected inference ACL");
    assert!(
        snapshot.acl.contains(expected.trim()),
        "compiled ACL missing Identity-projected inference credentials"
    );
    assert!(snapshot.acl.contains("audience = \"cloud-inference\""));
    assert!(snapshot.acl.contains("prefix = \"a3s_inf_abc12345\""));
    assert!(snapshot.acl.contains("prefix = \"a3s_inf_revoked001\""));
    assert!(snapshot.acl.contains("revoked = true"));
    assert!(snapshot.acl.contains("revoked = false"));
    assert_eq!(snapshot.acl.matches("inference {").count(), 1);
    assert!(!snapshot.acl.contains("\n  routes "));
    assert!(!snapshot.acl.contains("\n  workers "));
}

#[test]
fn projects_inference_route_grants_into_managed_snapshot_acl_without_workers() {
    use crate::modules::shared_kernel::domain::canonical_timestamp;
    use a3s_cloud_contracts::{
        render_inference_policy_acl_with_routes, InferenceCredentialAclProjection,
        InferenceEndpointAcl, InferenceGrantAclProjection, InferenceLimitsAclProjection,
        InferenceModelAclProjection, InferenceRouteAclProjection, InferenceTargetAclProjection,
        INFERENCE_CREDENTIAL_AUDIENCE,
    };
    use uuid::Uuid;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut owned = route(node_id, "api.example.com", "/v1", 49152);
    owned.state = RouteState::Active;
    owned.gateway_certificate_id = Some(certificate_id);
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let environment_id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        environment_id,
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .expect("credential projection");
    let inference_route = InferenceRouteAclProjection {
        route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
        router: "inference".into(),
        environment_id,
        policy_revision: 11,
        models: vec![InferenceModelAclProjection {
            alias: "chat-model".into(),
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
            targets: vec![InferenceTargetAclProjection {
                target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
                service: "model-service".into(),
                upstream_model: "internal/model-v1".into(),
                priority: 0,
                weight: 100,
            }],
        }],
        grants: vec![InferenceGrantAclProjection {
            credential_id: credential.credential_id,
            credential_generation: 3,
            models: vec!["chat-model".into()],
            endpoints: vec![InferenceEndpointAcl::Models],
            limits: InferenceLimitsAclProjection {
                max_concurrent_requests: 2,
                requests_per_minute: 60,
                request_burst: 2,
                tokens_per_minute: 10_000,
            },
        }],
    };

    let snapshot = compiler()
        .compile_certificate_convergence_with_inference_policy(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            Some(certificate_id),
            &[owned],
            &[credential.clone()],
            &[inference_route.clone()],
        )
        .expect("route-aware snapshot");

    let expected = render_inference_policy_acl_with_routes(
        canonical_timestamp(expires_at),
        &[credential],
        &[inference_route],
    )
    .expect("expected inference ACL with routes");
    assert!(
        snapshot.acl.contains(expected.trim()),
        "compiled ACL missing Inference-projected route grants"
    );
    assert!(snapshot
        .acl
        .contains("routes \"44444444-4444-4444-8444-444444444444\""));
    assert!(snapshot.acl.contains("models \"chat-model\""));
    assert!(snapshot
        .acl
        .contains("targets \"66666666-6666-4666-8666-666666666666\""));
    assert!(snapshot
        .acl
        .contains("grants \"33333333-3333-4333-8333-333333333333\""));
    assert!(snapshot.acl.contains("max_concurrent_requests = 2"));
    assert!(snapshot.acl.contains("requests_per_minute = 60"));
    assert!(!snapshot.acl.contains("\n  workers "));
    assert_eq!(snapshot.acl.matches("inference {").count(), 1);
}

#[tokio::test]
async fn empty_worker_port_keeps_managed_route_grants_without_inventing_workers() {
    use crate::modules::inference::application::{
        EmptyInferenceWorkerAclProjectionPort, IInferenceWorkerAclProjectionPort,
        InferenceRouteEnvironmentScope,
    };
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, InferenceEndpointAcl, InferenceGrantAclProjection,
        InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceRouteAclProjection,
        InferenceTargetAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
    };
    use uuid::Uuid;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut owned = route(node_id, "api.example.com", "/v1", 49152);
    owned.state = RouteState::Pending;
    owned.gateway_certificate_id = Some(certificate_id);
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let scope = InferenceRouteEnvironmentScope::new(organization_id, project_id, environment_id)
        .expect("worker scope");
    let inference_workers = EmptyInferenceWorkerAclProjectionPort
        .list_inference_worker_acl_projections(std::slice::from_ref(&scope), issued_at)
        .await
        .expect("intentional empty workers");
    assert!(inference_workers.is_empty());

    let environment_uuid = environment_id.as_uuid();
    let credential = InferenceCredentialAclProjection::new(
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        environment_uuid,
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .expect("credential projection");
    let inference_route = InferenceRouteAclProjection {
        route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
        router: "inference".into(),
        environment_id: environment_uuid,
        policy_revision: 11,
        models: vec![InferenceModelAclProjection {
            alias: "chat-model".into(),
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
            targets: vec![InferenceTargetAclProjection {
                target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
                service: "model-service".into(),
                upstream_model: "internal/model-v1".into(),
                priority: 0,
                weight: 100,
            }],
        }],
        grants: vec![InferenceGrantAclProjection {
            credential_id: credential.credential_id,
            credential_generation: 3,
            models: vec!["chat-model".into()],
            endpoints: vec![InferenceEndpointAcl::Models],
            limits: InferenceLimitsAclProjection {
                max_concurrent_requests: 2,
                requests_per_minute: 60,
                request_burst: 2,
                tokens_per_minute: 10_000,
            },
        }],
    };

    let snapshot = compiler()
        .compile_with_inference_policy_and_workers(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            certificate_id,
            &[owned],
            &[credential],
            &[inference_route],
            &inference_workers,
        )
        .expect("routes without workers");

    assert!(snapshot
        .acl
        .contains("routes \"44444444-4444-4444-8444-444444444444\""));
    assert!(snapshot.acl.contains(&format!(
        "tokenizer_revision = \"{}\"",
        a3s_cloud_contracts::INFERENCE_TOKENIZER_REVISION_V1
    )));
    assert!(!snapshot.acl.contains("\n  workers "));
}

#[test]
fn projects_inference_owned_workers_through_managed_compiler() {
    use a3s_cloud_contracts::{
        InferenceCredentialAclProjection, InferenceEndpointAcl, InferenceGrantAclProjection,
        InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceRouteAclProjection,
        InferenceServingPhase, InferenceTargetAclProjection, InferenceWorkerAclProjection,
        PowerTransferHealth, INFERENCE_CREDENTIAL_AUDIENCE, POWER_WORKER_OBSERVATION_SCHEMA,
    };
    use uuid::Uuid;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut owned = route(node_id, "api.example.com", "/v1", 49152);
    owned.state = RouteState::Pending;
    owned.gateway_certificate_id = Some(certificate_id);
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    let environment_id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let credential_id = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let target_id = Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap();
    let credential = InferenceCredentialAclProjection::new(
        credential_id,
        environment_id,
        INFERENCE_CREDENTIAL_AUDIENCE,
        "a3s_inf_abc12345",
        VERIFIER,
        3,
        expires_at + Duration::hours(1),
        false,
    )
    .expect("credential");
    let route_projection = InferenceRouteAclProjection {
        route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
        router: "inference".into(),
        environment_id,
        policy_revision: 11,
        models: vec![InferenceModelAclProjection {
            alias: "chat-model".into(),
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
            targets: vec![InferenceTargetAclProjection {
                target_id,
                service: "model-service".into(),
                upstream_model: "internal/model-v1".into(),
                priority: 0,
                weight: 100,
            }],
        }],
        grants: vec![InferenceGrantAclProjection {
            credential_id,
            credential_generation: 3,
            models: vec!["chat-model".into()],
            endpoints: vec![InferenceEndpointAcl::Models],
            limits: InferenceLimitsAclProjection {
                max_concurrent_requests: 2,
                requests_per_minute: 60,
                request_burst: 2,
                tokens_per_minute: 10_000,
            },
        }],
    };
    let worker = InferenceWorkerAclProjection {
        unit_id: "power-unit-1".into(),
        target_id,
        generation: 5,
        schema: POWER_WORKER_OBSERVATION_SCHEMA.into(),
        worker_epoch: Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        execution_profile_sha256: None,
        observation_generation: 9,
        observed_at: issued_at - Duration::seconds(1),
        expires_at: issued_at + Duration::seconds(14),
        phases: vec![InferenceServingPhase::Aggregated],
        prompt_cache_capable: true,
        state_transfer_capable: false,
        ready_phases: vec![InferenceServingPhase::Aggregated],
        active_limit: Some(8),
        active: 2,
        waiting: 1,
        prompt_cache_supported: true,
        prompt_cache_entries: 2,
        prompt_cache_capacity: 8,
        prompt_cache_pressure_basis_points: 2500,
        transfer_health: PowerTransferHealth::Unsupported,
        certified_latency_ms: Some(42),
    };
    let snapshot = compiler()
        .compile_with_inference_policy_and_workers(
            GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
            certificate_id,
            &[owned],
            &[credential],
            &[route_projection],
            &[worker],
        )
        .expect("worker-aware snapshot");
    assert!(snapshot
        .acl
        .contains("routes \"44444444-4444-4444-8444-444444444444\""));
    assert!(snapshot.acl.contains("workers \"power-unit-1\""));
    assert!(snapshot
        .acl
        .contains("grants \"33333333-3333-4333-8333-333333333333\""));
}

#[test]
fn rejects_cross_scope_and_duplicate_route_ownership() {
    let node_id = NodeId::new();
    let first = route(node_id, "api.example.com", "/v1", 49152);
    let duplicate = route(node_id, "api.example.com", "/v1", 49153);
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(10);
    assert!(compiler()
        .compile(
            GatewaySnapshotMetadata::new(node_id, 1, None, issued_at, expires_at),
            GatewayCertificateId::new(),
            &[first, duplicate],
        )
        .is_err());
    let foreign = route(NodeId::new(), "other.example.com", "/", 49154);
    assert!(compiler()
        .compile(
            GatewaySnapshotMetadata::new(node_id, 1, None, issued_at, expires_at),
            GatewayCertificateId::new(),
            &[foreign],
        )
        .is_err());
}

#[test]
fn installed_gateway_validates_compiled_snapshot() {
    let Ok(binary) = std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN") else {
        return;
    };
    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let mut route = route(node_id, "api.example.com", "/v1", 49152);
    route.gateway_certificate_id = Some(certificate_id);
    let issued_at = Utc::now();
    let snapshot = compiler()
        .compile(
            GatewaySnapshotMetadata::new(
                node_id,
                1,
                None,
                issued_at,
                issued_at + Duration::minutes(10),
            ),
            certificate_id,
            &[route],
        )
        .expect("snapshot");
    let directory = tempfile::tempdir().expect("Gateway validation directory");
    let path = directory.path().join("gateway.acl");
    let local_path = |name: &str| {
        directory
            .path()
            .join(name)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let certificate = snapshot
        .certificate_request
        .as_ref()
        .expect("compiled certificate request");
    let acl = snapshot
        .acl
        .replace(
            &acl_string(&certificate.certificate_file),
            &acl_string(&local_path("certificate.pem")),
        )
        .replace(
            &acl_string(&certificate.private_key_file),
            &acl_string(&local_path("private-key.pem")),
        )
        .replace(
            &acl_string("/var/lib/a3s-gateway/managed-snapshot.json"),
            &acl_string(&local_path("managed-snapshot.json")),
        );
    std::fs::write(&path, acl).expect("write compiled Gateway snapshot");
    let output = std::process::Command::new(binary)
        .arg("validate")
        .arg("--config")
        .arg(path)
        .output()
        .expect("run installed Gateway validator");
    assert!(
        output.status.success(),
        "installed Gateway rejected compiled snapshot: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_inference_policy_shell(acl: &str, expires_at: chrono::DateTime<Utc>) {
    use crate::modules::shared_kernel::domain::canonical_timestamp;
    use a3s_cloud_contracts::{
        render_inference_policy_shell_acl, require_inference_tokenizer_revision,
    };

    let expires_at = canonical_timestamp(expires_at);
    require_inference_tokenizer_revision(acl).expect("frozen tokenizer_revision");
    let expected = render_inference_policy_shell_acl(expires_at).expect("shell");
    assert!(
        acl.contains(expected.trim()),
        "compiled ACL missing grant-empty inference shell aligned to snapshot expiry"
    );
    assert_eq!(acl.matches("inference {").count(), 1);
    assert!(!acl.contains("credentials "));
    assert!(!acl.contains("\n  routes "));
    assert!(!acl.contains("\n  workers "));
}
