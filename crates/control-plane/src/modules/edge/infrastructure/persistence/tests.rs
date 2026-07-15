use super::*;
use crate::modules::edge::domain::repositories::{IEdgeRepository, StageRoutePublication};
use crate::modules::edge::domain::{
    GatewayPublication, Route, RouteHostname, RoutePath, RoutePortName, RouteState,
    UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, GatewayAckState, GatewaySnapshot, NodeGatewayAck};
use chrono::{Duration, Utc};
use uuid::Uuid;

fn staged(
    node_id: NodeId,
    revision: u64,
    expected_revision: Option<u64>,
    hostname: &str,
    path: &str,
    key: &str,
) -> StageRoutePublication {
    let now = Utc::now();
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let snapshot = GatewaySnapshot::new(
        revision,
        expected_revision,
        format!("# {hostname}{path}\nmanagement {{ enabled = true }}\n"),
    )
    .expect("snapshot");
    let mut route = Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        RouteHostname::parse(hostname).expect("hostname"),
        RoutePath::parse(path).expect("path"),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        RoutePortName::parse("http").expect("port"),
        UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("endpoint"),
        now,
    );
    route
        .stage(revision, command_id, snapshot.snapshot_digest.clone(), now)
        .expect("stage route");
    let publication = GatewayPublication::stage(
        node_id,
        command_id,
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("publication");
    let canonical = format!("{hostname}{path}");
    StageRoutePublication {
        route: route.clone(),
        publication,
        expected_scope_version: 0,
        idempotency: IdempotencyRequest::new("routes", key, canonical.as_bytes())
            .expect("idempotency"),
        event: DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.route.publication-staged".into(),
            schema_version: 1,
            organization_id: route.organization_id.as_uuid(),
            aggregate_id: route.id.as_uuid(),
            aggregate_version: route.aggregate_version,
            occurred_at: now,
            correlation_id,
            causation_id: None,
            payload: serde_json::json!({ "route_id": route.id }),
        },
    }
}

fn acknowledgement(
    staged: &crate::modules::edge::domain::repositories::EdgeRoutePublicationResult,
    state: GatewayAckState,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: staged.publication.command_id.as_uuid(),
        node_id: staged.publication.node_id.as_uuid(),
        revision: staged.publication.revision,
        snapshot_digest: staged.publication.snapshot_digest.clone(),
        state,
        message: (state == GatewayAckState::Rejected).then(|| "invalid snapshot".into()),
        acknowledged_at: staged.publication.command_issued_at + Duration::seconds(1),
    }
}

#[tokio::test]
async fn enforces_one_owner_for_hostname_path_inside_gateway_scope() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = staged(node_id, 1, None, "api.example.com", "/v1", "first");
    let stored = repository
        .stage_route_publication(first)
        .await
        .expect("first route");
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(&stored, GatewayAckState::Applied),
            stored.publication.command_issued_at + Duration::seconds(2),
        )
        .await
        .expect("acknowledge first route");
    let mut duplicate = staged(node_id, 2, Some(1), "API.EXAMPLE.COM", "/v1", "duplicate");
    duplicate.expected_scope_version = 2;
    assert!(repository.stage_route_publication(duplicate).await.is_err());

    let other_scope = staged(
        NodeId::new(),
        1,
        None,
        "api.example.com",
        "/v1",
        "other-scope",
    );
    repository
        .stage_route_publication(other_scope)
        .await
        .expect("same tuple in another Gateway scope");
}

#[tokio::test]
async fn serializes_complete_snapshots_and_projects_exact_acknowledgements() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = repository
        .stage_route_publication(staged(node_id, 1, None, "api.example.com", "/v1", "first"))
        .await
        .expect("stage first");
    let second_pending = staged(node_id, 2, None, "web.example.com", "/", "second-pending");
    assert!(repository
        .stage_route_publication(second_pending)
        .await
        .is_err());

    let rejected = acknowledgement(&first, GatewayAckState::Rejected);
    assert!(repository
        .project_gateway_acknowledgement(&rejected, rejected.acknowledged_at + Duration::seconds(1))
        .await
        .expect("reject publication"));
    let stored = repository
        .find_route(first.route.organization_id, first.route.id)
        .await
        .expect("route");
    assert_eq!(stored.state, RouteState::Rejected);
    let scope = repository.gateway_scope(node_id).await.expect("scope");
    assert_eq!(scope.last_issued_revision, 1);
    assert_eq!(scope.installed_revision, None);
    assert_eq!(scope.aggregate_version, 1);

    let mut second = staged(node_id, 2, None, "web.example.com", "/", "second");
    second.expected_scope_version = 1;
    let second = repository
        .stage_route_publication(second)
        .await
        .expect("stage after rejection");
    let applied = acknowledgement(&second, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(&applied, applied.acknowledged_at + Duration::seconds(1))
        .await
        .expect("apply publication");
    let scope = repository.gateway_scope(node_id).await.expect("scope");
    assert_eq!(scope.last_issued_revision, 2);
    assert_eq!(scope.installed_revision, Some(2));
    assert_eq!(scope.aggregate_version, 3);
}
