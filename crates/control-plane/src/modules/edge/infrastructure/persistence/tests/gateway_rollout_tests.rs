use crate::modules::edge::domain::events::{GatewayRolloutStaged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
};
use crate::modules::edge::domain::services::{ResolvedRouteTarget, ResolvedRouteTargetSet};
use crate::modules::edge::domain::{
    DomainNamePattern, GatewayCertificateState, GatewayPublication, GatewayPublicationState,
    GatewayRollout, GatewayRolloutPolicy, GatewayRolloutRollbackState, GatewayRolloutState,
    GatewayScope, GatewayScopeState, RouteHostname, RoutePath, RoutePortName, RouteState,
    RouteTarget, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::infrastructure::{
    CompileGatewayRolloutRollback, CompileGatewayRouteRollout, GatewayMemberSnapshotContext,
    GatewayRollbackMemberSnapshotContext, GatewayRolloutRollbackCompiler,
    GatewayRolloutRollbackReconciler, GatewayRouteRolloutCompiler, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig,
};
use crate::modules::shared_kernel::domain::{DomainClaimId, EnvironmentId, RepositoryError};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, GatewaySnapshot, NodeGatewayAck,
};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

#[path = "gateway_rollout_tests/certificate_convergence_tests.rs"]
mod certificate_convergence_tests;

#[path = "gateway_rollout_tests/staging_tests.rs"]
mod staging_tests;

#[path = "gateway_rollout_tests/projection_tests.rs"]
mod projection_tests;

#[path = "gateway_rollout_tests/replica_and_rollback_tests.rs"]
mod replica_and_rollback_tests;

fn replicated_scope(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    members: [NodeId; 2],
    now: chrono::DateTime<Utc>,
) -> GatewayScope {
    GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(1, 1, members.len()).expect("rollout policy"),
        now,
    )
    .expect("replicated Gateway scope")
}

async fn persist_scope(repository: &InMemoryEdgeRepository, scope: &GatewayScope, key: &str) {
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "route-rollout-scopes",
                key,
                serde_json::to_vec(&scope.member_node_ids)
                    .expect("scope members")
                    .as_slice(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("persist Gateway scope");
}

fn route_rollout_bundle(
    scope: &GatewayScope,
    route_id: RouteId,
    generation: u64,
    hostname: &str,
    key: &str,
    now: chrono::DateTime<Utc>,
) -> StageGatewayRollout {
    route_rollout_bundle_with_contexts(
        scope,
        route_id,
        generation,
        hostname,
        key,
        scope
            .member_node_ids
            .iter()
            .map(|node_id| GatewayMemberSnapshotContext {
                scope: GatewayScopeState::empty(*node_id),
                active_routes: Vec::new(),
            })
            .collect(),
        now,
    )
}

#[allow(clippy::too_many_arguments)]
fn route_rollout_bundle_with_contexts(
    scope: &GatewayScope,
    route_id: RouteId,
    generation: u64,
    hostname: &str,
    key: &str,
    member_contexts: Vec<GatewayMemberSnapshotContext>,
    now: chrono::DateTime<Utc>,
) -> StageGatewayRollout {
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let targets = ResolvedRouteTargetSet::new(
        &scope.member_node_ids,
        scope
            .member_node_ids
            .iter()
            .enumerate()
            .map(|(index, node_id)| ResolvedRouteTarget {
                workload_id,
                node_id: *node_id,
                target: RouteTarget::new(
                    workload_id,
                    revision_id,
                    format!("workload:{workload_id}:revision:{revision_id}"),
                    1,
                    RoutePortName::parse("http").expect("port"),
                    UpstreamEndpoint::parse(format!(
                        "http://127.0.0.1:{}",
                        49_152 + u16::try_from(index).expect("member ordinal")
                    ))
                    .expect("upstream"),
                    now,
                )
                .expect("Route target"),
            })
            .collect(),
    )
    .expect("resolved target set");
    let compiler = GatewayRouteRolloutCompiler::new(
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })
        .expect("snapshot compiler"),
        Duration::minutes(3),
        Duration::hours(24),
    )
    .expect("rollout compiler");
    compiler
        .compile(CompileGatewayRouteRollout {
            scope: scope.clone(),
            rollout_id: GatewayRolloutId::new(),
            generation,
            correlation_id: Uuid::now_v7(),
            route_id,
            hostname: RouteHostname::parse(hostname).expect("hostname"),
            path_prefix: RoutePath::parse("/").expect("path"),
            domain_claim_id: DomainClaimId::new(),
            domain_pattern: DomainNamePattern::parse(hostname).expect("domain pattern"),
            target_set: targets,
            member_contexts,
            issued_at: now,
        })
        .expect("compiled Route rollout")
        .stage_bundle(
            IdempotencyRequest::new(
                format!("gateway-scopes/{}/route-rollouts", scope.id),
                key,
                hostname.as_bytes(),
            )
            .expect("Route rollout idempotency"),
        )
        .expect("Route rollout stage bundle")
}

fn publication(
    node_id: NodeId,
    correlation_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> GatewayPublication {
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::hours(1),
        format!("# rollout snapshot for {node_id}"),
    )
    .expect("snapshot");
    GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("publication")
}

fn acknowledgement(
    publication: &GatewayPublication,
    state: GatewayAckState,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: publication.command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        expires_at: publication.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "snapshot rejected".into()),
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}
