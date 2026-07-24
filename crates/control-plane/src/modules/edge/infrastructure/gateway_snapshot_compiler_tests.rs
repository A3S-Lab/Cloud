use super::{GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata};
use crate::modules::edge::domain::{
    DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteTarget,
    UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, NodeId,
    OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use chrono::{Duration, Utc};

#[test]
fn snapshot_digest_binds_the_exact_runtime_generation() {
    let now = canonical_timestamp(Utc::now());
    let node_id = NodeId::new();
    let certificate_id = GatewayCertificateId::new();
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let route = Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        RouteHostname::parse("generation.example.com").expect("hostname"),
        RoutePath::parse("/").expect("path"),
        DomainClaimId::new(),
        DomainNamePattern::parse("generation.example.com").expect("domain pattern"),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").expect("port name"),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            now,
        )
        .expect("route target"),
        now,
    )
    .expect("route");
    let compiler = GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8443".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 5_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .expect("compiler");
    let metadata = GatewaySnapshotMetadata::new(node_id, 1, None, now, now + Duration::hours(24));
    let first = compiler
        .compile(metadata, certificate_id, std::slice::from_ref(&route))
        .expect("generation one snapshot");

    let mut newer = route;
    newer.target.runtime_generation = 2;
    let second = compiler
        .compile(metadata, certificate_id, std::slice::from_ref(&newer))
        .expect("generation two snapshot");

    assert_eq!(newer.target.upstream.as_str(), "http://127.0.0.1:49152/");
    assert_ne!(first.snapshot_digest, second.snapshot_digest);
    assert!(first.acl.contains("generation=1"));
    assert!(second.acl.contains("generation=2"));
}
