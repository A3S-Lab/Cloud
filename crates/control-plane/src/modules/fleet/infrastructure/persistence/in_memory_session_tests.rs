use super::*;
use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::{
    NodeCapabilities, NodeName, NodeProtocolPolicy,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeObservationBatchV2, NodeProtocolContractSet,
    NodeSessionHello, NodeSessionSelectionReference,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const POWER_OBSERVATION_SCHEMA: &str = "a3s.cloud.node-power-worker-observation-batch.v1";

fn contracts(include_power: bool) -> NodeProtocolContractSet {
    let mut agent_writable = vec![
        NodeCommandAck::SCHEMA.into(),
        NodeObservationBatchV2::SCHEMA.into(),
    ];
    if include_power {
        agent_writable.push(POWER_OBSERVATION_SCHEMA.into());
    }
    agent_writable.sort();
    NodeProtocolContractSet {
        agent_readable: vec![NodeCommandEnvelope::SCHEMA.into()],
        agent_writable,
    }
}

fn policy(include_power: bool) -> NodeProtocolPolicy {
    NodeProtocolPolicy::new(contracts(include_power), contracts(false)).expect("protocol policy")
}

fn hello(
    node_id: NodeId,
    agent_instance_id: Uuid,
    sequence: u64,
    previous_selection: Option<NodeSessionSelectionReference>,
    offered_at: DateTime<Utc>,
) -> NodeSessionHello {
    NodeSessionHello {
        schema: NodeSessionHello::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id,
        session_epoch: Uuid::from_u128(10),
        hello_sequence: sequence,
        offered_at,
        agent_version: "0.1.0".into(),
        contracts: contracts(true),
        previous_selection,
    }
}

fn negotiation(
    hello: NodeSessionHello,
    proposed_session_id: Uuid,
    received_at: DateTime<Utc>,
) -> NodeProtocolNegotiation {
    NodeProtocolNegotiation::new(
        hello,
        policy(true),
        received_at,
        Duration::hours(1),
        proposed_session_id,
    )
    .expect("protocol negotiation")
}

async fn repository_with_node() -> (Arc<InMemoryNodeRepository>, NodeId, Uuid) {
    let repository = Arc::new(InMemoryNodeRepository::new());
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let agent_instance_id = Uuid::now_v7();
    let enrolled_at = canonical_timestamp(Utc::now());
    let node = Node::enroll(
        node_id,
        organization_id,
        NodeName::new("session-worker").expect("node name"),
        agent_instance_id,
        "0.1.0",
        NodeCapabilities::new(
            "a3s-box",
            "session-test",
            json!({
                "schema": "a3s.runtime.capabilities.v4",
                "provider_id": "a3s-box",
                "provider_build": "session-test"
            }),
        )
        .expect("node capabilities"),
        enrolled_at,
    )
    .expect("node");
    repository
        .state
        .write()
        .await
        .nodes
        .insert((organization_id, node_id), node);
    (repository, node_id, agent_instance_id)
}

#[tokio::test]
async fn concurrent_exact_hellos_commit_one_session_and_replay_the_other() {
    let (repository, node_id, agent_instance_id) = repository_with_node().await;
    let offered_at = canonical_timestamp(Utc::now());
    let hello = hello(node_id, agent_instance_id, 1, None, offered_at);
    let first = negotiation(
        hello.clone(),
        Uuid::from_u128(100),
        offered_at + Duration::seconds(1),
    );
    let second = negotiation(
        hello,
        Uuid::from_u128(101),
        offered_at + Duration::seconds(1),
    );

    let (first, second) = tokio::join!(repository.negotiate(first), repository.negotiate(second));
    let first = first.expect("first negotiation");
    let second = second.expect("second negotiation");

    assert_ne!(first.replayed(), second.replayed());
    assert_eq!(first.selection(), second.selection());
    assert_eq!(repository.state.read().await.protocol_sessions.len(), 1);
}

#[tokio::test]
async fn stored_head_fences_stale_reconnects() {
    let (repository, node_id, agent_instance_id) = repository_with_node().await;
    let offered_at = canonical_timestamp(Utc::now());
    let first = repository
        .negotiate(negotiation(
            hello(node_id, agent_instance_id, 1, None, offered_at),
            Uuid::from_u128(100),
            offered_at + Duration::seconds(1),
        ))
        .await
        .expect("first negotiation");
    let first_reference = first.record().reference().expect("first reference");
    let second_hello = hello(
        node_id,
        agent_instance_id,
        2,
        Some(first_reference.clone()),
        offered_at + Duration::seconds(2),
    );
    let second = repository
        .negotiate(negotiation(
            second_hello.clone(),
            Uuid::from_u128(101),
            offered_at + Duration::seconds(3),
        ))
        .await
        .expect("second negotiation");
    assert_eq!(second.selection().generation, 2);

    let stale = repository
        .negotiate(negotiation(
            NodeSessionHello {
                session_epoch: Uuid::from_u128(11),
                hello_sequence: 1,
                previous_selection: Some(first_reference),
                ..second_hello
            },
            Uuid::from_u128(102),
            offered_at + Duration::seconds(4),
        ))
        .await;
    assert!(matches!(stale, Err(RepositoryError::Conflict(_))));
}

#[tokio::test]
async fn negotiation_requires_the_enrolled_live_agent_identity() {
    let (repository, node_id, agent_instance_id) = repository_with_node().await;
    let offered_at = canonical_timestamp(Utc::now());
    let wrong_agent = negotiation(
        hello(node_id, Uuid::now_v7(), 1, None, offered_at),
        Uuid::from_u128(100),
        offered_at + Duration::seconds(1),
    );
    assert!(matches!(
        repository.negotiate(wrong_agent).await,
        Err(RepositoryError::Forbidden(_))
    ));

    repository
        .state
        .write()
        .await
        .nodes
        .values_mut()
        .find(|node| node.id == node_id)
        .expect("stored node")
        .revoke();
    let revoked = negotiation(
        hello(node_id, agent_instance_id, 1, None, offered_at),
        Uuid::from_u128(101),
        offered_at + Duration::seconds(1),
    );
    assert_eq!(
        repository.negotiate(revoked).await,
        Err(RepositoryError::NotFound)
    );
}
