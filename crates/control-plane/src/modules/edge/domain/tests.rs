use super::*;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayAckState, GatewaySnapshot, NodeGatewayAck};
use chrono::{Duration, Utc};
use uuid::Uuid;

fn route(now: chrono::DateTime<Utc>) -> Route {
    Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        NodeId::new(),
        RouteHostname::parse("API.Example.COM").expect("hostname"),
        RoutePath::parse("/v1").expect("path"),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        RoutePortName::parse("http").expect("port"),
        UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
        now,
    )
}

#[test]
fn normalizes_route_ownership_and_rejects_ambiguous_values() {
    assert_eq!(
        RouteHostname::parse(" API.Example.COM ")
            .expect("hostname")
            .as_str(),
        "api.example.com"
    );
    assert!(RouteHostname::parse("127.0.0.1").is_err());
    assert!(RouteHostname::parse("api..example.com").is_err());
    assert!(RoutePath::parse("v1").is_err());
    assert!(RoutePath::parse("/v1//chat").is_err());
    assert!(RoutePath::parse("/v1/../admin").is_err());
    assert!(RoutePath::parse("/v1%2").is_err());
    assert!(UpstreamEndpoint::parse("http://10.0.0.8:8080").is_err());
    assert!(UpstreamEndpoint::parse("https://127.0.0.1:8080").is_err());
}

#[test]
fn route_activates_only_for_the_exact_gateway_publication() {
    let now = Utc::now();
    let mut route = route(now);
    let command_id = NodeCommandId::new();
    let snapshot =
        GatewaySnapshot::new(3, Some(2), "management { enabled = true }\n").expect("snapshot");
    route
        .stage(
            snapshot.revision,
            command_id,
            snapshot.snapshot_digest.clone(),
            now + Duration::seconds(1),
        )
        .expect("stage");
    let wrong = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: route.gateway_node_id.as_uuid(),
        revision: 4,
        snapshot_digest: snapshot.snapshot_digest.clone(),
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: now + Duration::seconds(2),
    };
    assert!(route.apply_gateway_acknowledgement(&wrong).is_err());
    assert_eq!(route.state, RouteState::Publishing);

    let applied = NodeGatewayAck {
        revision: 3,
        ..wrong
    };
    route
        .apply_gateway_acknowledgement(&applied)
        .expect("apply exact acknowledgement");
    assert_eq!(route.state, RouteState::Active);
    assert_eq!(
        route.activated_at,
        Some(canonical_timestamp(applied.acknowledged_at))
    );
}

#[test]
fn rejected_publication_preserves_failure_without_false_activation() {
    let now = Utc::now();
    let mut route = route(now);
    let command_id = NodeCommandId::new();
    let snapshot =
        GatewaySnapshot::new(1, None, "management { enabled = true }\n").expect("snapshot");
    route
        .stage(1, command_id, snapshot.snapshot_digest.clone(), now)
        .expect("stage");
    route
        .apply_gateway_acknowledgement(&NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: command_id.as_uuid(),
            node_id: route.gateway_node_id.as_uuid(),
            revision: 1,
            snapshot_digest: snapshot.snapshot_digest,
            state: GatewayAckState::Rejected,
            message: Some("validation failed".into()),
            acknowledged_at: now + Duration::seconds(1),
        })
        .expect("reject");
    assert_eq!(route.state, RouteState::Rejected);
    assert_eq!(route.failure.as_deref(), Some("validation failed"));
    assert_eq!(route.activated_at, None);
}
