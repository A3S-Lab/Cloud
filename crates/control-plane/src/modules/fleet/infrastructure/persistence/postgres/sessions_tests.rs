use super::*;
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeObservationBatchV2, NodeProtocolContractSet,
};
use chrono::{Duration, TimeZone, Utc};
use uuid::Uuid;

fn contracts() -> NodeProtocolContractSet {
    NodeProtocolContractSet {
        agent_readable: vec![NodeCommandEnvelope::SCHEMA.into()],
        agent_writable: vec![
            NodeCommandAck::SCHEMA.into(),
            NodeObservationBatchV2::SCHEMA.into(),
        ],
    }
}

#[test]
fn stored_session_restore_validates_the_digest_bound_contract() {
    let selected_at = Utc
        .with_ymd_and_hms(2026, 8, 28, 0, 0, 0)
        .single()
        .expect("timestamp");
    let hello = NodeSessionHello {
        schema: NodeSessionHello::SCHEMA.into(),
        node_id: Uuid::from_u128(1),
        agent_instance_id: Uuid::from_u128(2),
        session_epoch: Uuid::from_u128(3),
        hello_sequence: 1,
        offered_at: selected_at,
        agent_version: "0.1.0".into(),
        contracts: contracts(),
        previous_selection: None,
    };
    let selection = NodeSessionSelection {
        schema: NodeSessionSelection::SCHEMA.into(),
        node_id: hello.node_id,
        agent_instance_id: hello.agent_instance_id,
        session_epoch: hello.session_epoch,
        hello_sequence: hello.hello_sequence,
        session_id: Uuid::from_u128(4),
        generation: 1,
        selected_at,
        expires_at: selected_at + Duration::minutes(5),
        contracts: contracts(),
        previous_selection: None,
    };
    let digest = selection.reference().expect("reference").contracts_digest;
    let row = (
        digest,
        serde_json::to_value(&hello).expect("hello JSON"),
        serde_json::to_value(&selection).expect("selection JSON"),
    );
    assert_eq!(
        restore(row).expect("stored session").selection(),
        &selection
    );

    let mismatched = restore((
        format!("sha256:{}", "0".repeat(64)),
        serde_json::to_value(&hello).expect("hello JSON"),
        serde_json::to_value(&selection).expect("selection JSON"),
    ));
    assert!(matches!(mismatched, Err(RepositoryError::Storage(_))));
}
