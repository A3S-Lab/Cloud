use super::*;
use crate::modules::fleet::domain::value_objects::NodeProtocolSessionError;
use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeObservationBatchV2, NodeProtocolContractSet,
    NodeSessionHello,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};

const POWER_OBSERVATION_SCHEMA: &str = "a3s.cloud.node-power-worker-observation-batch.v1";

#[derive(Default)]
struct SessionRepositoryStub {
    calls: AtomicUsize,
}

#[async_trait]
impl INodeProtocolSessionRepository for SessionRepositoryStub {
    async fn negotiate(
        &self,
        negotiation: NodeProtocolNegotiation,
    ) -> Result<
        crate::modules::fleet::domain::value_objects::NodeProtocolNegotiationOutcome,
        RepositoryError,
    > {
        self.calls.fetch_add(1, Ordering::Relaxed);
        negotiation
            .apply(None)
            .map_err(|error| RepositoryError::Conflict(error.to_string()))
    }
}

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

fn policy() -> NodeProtocolPolicy {
    NodeProtocolPolicy::new(contracts(true), contracts(false)).expect("protocol policy")
}

fn hello(node_id: NodeId) -> NodeSessionHello {
    NodeSessionHello {
        schema: NodeSessionHello::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id: Uuid::now_v7(),
        session_epoch: Uuid::now_v7(),
        hello_sequence: 1,
        offered_at: Utc::now(),
        agent_version: "0.1.0".into(),
        contracts: contracts(true),
        previous_selection: None,
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

#[tokio::test]
async fn handler_selects_supported_contracts_for_the_authenticated_node() {
    let sessions = Arc::new(SessionRepositoryStub::default());
    let handler = NegotiateNodeSessionHandler::new(sessions.clone(), policy(), Duration::hours(1))
        .expect("handler");
    let node_id = NodeId::new();
    let result = handler
        .execute(
            NegotiateNodeSession {
                authenticated_node_id: node_id,
                hello: hello(node_id),
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .expect("command transport")
        .expect("selection");

    assert!(!result.replayed);
    assert!(result
        .selection
        .contracts
        .agent_writable
        .iter()
        .any(|schema| schema == POWER_OBSERVATION_SCHEMA));
    assert_eq!(sessions.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn handler_rejects_a_certificate_bound_to_another_node_before_storage() {
    let sessions = Arc::new(SessionRepositoryStub::default());
    let handler = NegotiateNodeSessionHandler::new(sessions.clone(), policy(), Duration::hours(1))
        .expect("handler");
    let result = handler
        .execute(
            NegotiateNodeSession {
                authenticated_node_id: NodeId::new(),
                hello: hello(NodeId::new()),
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .expect("command transport");

    assert!(matches!(result, Err(ApplicationError::Forbidden(_))));
    assert_eq!(sessions.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn handler_rejects_invalid_selection_lifetimes_at_composition_time() {
    let sessions = Arc::new(SessionRepositoryStub::default());
    assert!(NegotiateNodeSessionHandler::new(sessions, policy(), Duration::hours(25),).is_err());

    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NegotiateNodeSessionHandler>();
    assert_send_sync::<NodeProtocolSessionError>();
}
