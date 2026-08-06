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
    let snapshot = compiler()
        .compile_certificate_convergence(
            GatewaySnapshotMetadata::new(
                node_id,
                2,
                Some(1),
                issued_at,
                issued_at + Duration::minutes(10),
            ),
            None,
            &[],
        )
        .expect("route-less revocation snapshot");

    assert!(snapshot.certificate_request.is_none());
    assert!(!snapshot.acl.contains("entrypoints \"a3s-cloud-https\""));
    assert!(snapshot.acl.contains("management {"));
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
    std::fs::write(&path, snapshot.acl).expect("write compiled Gateway snapshot");
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
