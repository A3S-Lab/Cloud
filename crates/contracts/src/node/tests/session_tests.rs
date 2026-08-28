use super::*;
use chrono::DateTime;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-28T00:00:00Z")
        .expect("fixed timestamp")
        .with_timezone(&Utc)
        + Duration::seconds(seconds)
}

fn contracts() -> NodeProtocolContractSet {
    NodeProtocolContractSet {
        agent_readable: vec![
            NodeCommandLeaseResponse::SCHEMA.into(),
            NodeCommandEnvelope::SCHEMA.into(),
        ],
        agent_writable: vec![
            NodeCommandAck::SCHEMA.into(),
            NodeObservationBatchV2::SCHEMA.into(),
        ],
    }
}

fn hello(previous_selection: Option<NodeSessionSelectionReference>) -> NodeSessionHello {
    NodeSessionHello {
        schema: NodeSessionHello::SCHEMA.into(),
        node_id: Uuid::from_u128(1),
        agent_instance_id: Uuid::from_u128(2),
        session_epoch: Uuid::from_u128(3),
        hello_sequence: 1,
        offered_at: at(0),
        agent_version: "0.1.0".into(),
        contracts: contracts(),
        previous_selection,
    }
}

fn selection(hello: &NodeSessionHello) -> NodeSessionSelection {
    NodeSessionSelection {
        schema: NodeSessionSelection::SCHEMA.into(),
        node_id: hello.node_id,
        agent_instance_id: hello.agent_instance_id,
        session_epoch: hello.session_epoch,
        hello_sequence: hello.hello_sequence,
        session_id: Uuid::from_u128(4),
        generation: hello
            .previous_selection
            .as_ref()
            .map_or(1, |previous| previous.generation + 1),
        selected_at: at(1),
        expires_at: at(3_601),
        contracts: NodeProtocolContractSet {
            agent_readable: vec![NodeCommandEnvelope::SCHEMA.into()],
            agent_writable: vec![NodeObservationBatchV2::SCHEMA.into()],
        },
        previous_selection: hello.previous_selection.clone(),
    }
}

#[test]
fn exact_selection_is_subset_bound_and_digest_chained() {
    let first_hello = hello(None);
    first_hello.validate_at(at(1)).expect("fresh session hello");
    let first = selection(&first_hello);
    first
        .validate_for(&first_hello, at(2))
        .expect("valid first selection");
    let reference = first.reference().expect("selection reference");
    reference.validate().expect("valid reference");

    let next_hello = hello(Some(reference.clone()));
    let next = selection(&next_hello);
    next.validate_for(&next_hello, at(2))
        .expect("valid chained selection");
    assert_eq!(next.generation, 2);
    assert_eq!(next.previous_selection, Some(reference));
}

#[test]
fn contract_sets_are_closed_sorted_and_canonical() {
    let set = contracts();
    set.validate().expect("valid contract set");
    assert!(set
        .digest()
        .expect("contract digest")
        .starts_with("sha256:"));

    let mut unordered = set.clone();
    unordered.agent_writable.reverse();
    assert!(unordered.validate().is_err());
    let mut duplicate = set.clone();
    duplicate
        .agent_readable
        .push(duplicate.agent_readable[1].clone());
    assert!(duplicate.validate().is_err());
    let mut noncanonical = set;
    noncanonical.agent_writable = vec!["A3S.cloud.node-observation-batch.v2".into()];
    assert!(noncanonical.validate().is_err());
}

#[test]
fn selection_rejects_downgrade_unoffered_contracts_and_expiry() {
    let previous = NodeSessionSelectionReference {
        session_id: Uuid::from_u128(9),
        generation: 7,
        contracts_digest: format!("sha256:{}", "a".repeat(64)),
    };
    let chained_hello = hello(Some(previous));
    let mut invalid = selection(&chained_hello);
    invalid.generation = 7;
    assert!(invalid.validate_for(&chained_hello, at(2)).is_err());

    invalid = selection(&chained_hello);
    invalid.contracts.agent_writable = vec!["a3s.power.worker-observation.v1".into()];
    assert!(invalid.validate_for(&chained_hello, at(2)).is_err());

    invalid = selection(&chained_hello);
    invalid.expires_at = at(2);
    assert!(invalid.validate_for(&chained_hello, at(2)).is_err());

    let mut stale_hello = hello(None);
    stale_hello.offered_at = at(-301);
    assert!(stale_hello.validate_at(at(0)).is_err());
}

#[test]
fn session_contracts_reject_unknown_fields_and_are_send_sync() {
    let mut encoded = serde_json::to_value(hello(None)).expect("encoded hello");
    encoded["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NodeSessionHello>(encoded).is_err());

    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NodeSessionHello>();
    assert_send_sync::<NodeSessionSelection>();
}
