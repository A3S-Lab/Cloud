use super::*;
use a3s_cloud_contracts::{NodeCommandAck, NodeCommandEnvelope, NodeObservationBatchV2};

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-28T00:00:00Z")
        .expect("fixed timestamp")
        .with_timezone(&Utc)
        + Duration::seconds(seconds)
}

fn set(include_power: bool) -> NodeProtocolContractSet {
    let mut writable = vec![
        NodeCommandAck::SCHEMA.into(),
        NodeObservationBatchV2::SCHEMA.into(),
    ];
    if include_power {
        writable.push("a3s.cloud.node-power-worker-observation-batch.v1".into());
    }
    writable.sort();
    NodeProtocolContractSet {
        agent_readable: vec![NodeCommandEnvelope::SCHEMA.into()],
        agent_writable: writable,
    }
}

fn policy(include_power: bool) -> NodeProtocolPolicy {
    NodeProtocolPolicy::new(set(include_power), set(false)).expect("protocol policy")
}

fn hello(
    epoch: u128,
    sequence: u64,
    previous_selection: Option<NodeSessionSelectionReference>,
    include_power: bool,
) -> NodeSessionHello {
    NodeSessionHello {
        schema: NodeSessionHello::SCHEMA.into(),
        node_id: Uuid::from_u128(1),
        agent_instance_id: Uuid::from_u128(2),
        session_epoch: Uuid::from_u128(epoch),
        hello_sequence: sequence,
        offered_at: at(i64::try_from(sequence).expect("test sequence")),
        agent_version: "0.1.0".into(),
        contracts: set(include_power),
        previous_selection,
    }
}

fn negotiate(
    hello: NodeSessionHello,
    policy: NodeProtocolPolicy,
    session_id: u128,
) -> NodeProtocolNegotiation {
    let received_at = hello.offered_at + Duration::seconds(1);
    NodeProtocolNegotiation::new(
        hello,
        policy,
        received_at,
        Duration::hours(1),
        Uuid::from_u128(session_id),
    )
    .expect("valid negotiation")
}

#[test]
fn first_selection_and_exact_replay_are_deterministic() {
    let first = negotiate(hello(10, 1, None, false), policy(false), 100)
        .apply(None)
        .expect("first selection");
    assert!(!first.replayed());
    assert_eq!(first.selection().generation, 1);

    let replay = negotiate(first.record().hello().clone(), policy(false), 999)
        .apply(Some(first.record()))
        .expect("exact replay");
    assert!(replay.replayed());
    assert_eq!(replay.selection().session_id, Uuid::from_u128(100));
}

#[test]
fn reconnect_and_process_restart_require_the_exact_selection_chain() {
    let first = negotiate(hello(10, 1, None, false), policy(false), 100)
        .apply(None)
        .expect("first selection");
    let reference = first.record().reference().expect("selection reference");
    let reconnect = negotiate(
        hello(10, 2, Some(reference.clone()), false),
        policy(false),
        101,
    )
    .apply(Some(first.record()))
    .expect("same-process reconnect");
    assert_eq!(reconnect.selection().generation, 2);

    let restart_reference = reconnect.record().reference().expect("restart reference");
    let restart = negotiate(
        hello(11, 1, Some(restart_reference), false),
        policy(false),
        102,
    )
    .apply(Some(reconnect.record()))
    .expect("process restart");
    assert_eq!(restart.selection().generation, 3);

    let skipped = negotiate(
        hello(
            11,
            3,
            Some(restart.record().reference().expect("head")),
            false,
        ),
        policy(false),
        103,
    );
    assert_eq!(
        skipped.apply(Some(restart.record())),
        Err(NodeProtocolSessionError::HelloSequenceConflict)
    );
}

#[test]
fn negotiation_adds_supported_power_transport_but_never_downgrades() {
    let first = negotiate(hello(10, 1, None, false), policy(false), 100)
        .apply(None)
        .expect("baseline selection");
    let upgraded = negotiate(
        hello(
            10,
            2,
            Some(first.record().reference().expect("baseline reference")),
            true,
        ),
        policy(true),
        101,
    )
    .apply(Some(first.record()))
    .expect("Power transport upgrade");
    assert!(upgraded
        .selection()
        .contracts
        .agent_writable
        .contains(&"a3s.cloud.node-power-worker-observation-batch.v1".into()));

    let downgrade = negotiate(
        hello(
            10,
            3,
            Some(upgraded.record().reference().expect("upgraded reference")),
            false,
        ),
        policy(true),
        102,
    );
    assert_eq!(
        downgrade.apply(Some(upgraded.record())),
        Err(NodeProtocolSessionError::ProtocolDowngrade)
    );
}

#[test]
fn protocol_session_domain_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<NodeProtocolPolicy>();
    assert_send_sync::<NodeProtocolNegotiation>();
    assert_send_sync::<NodeProtocolSessionRecord>();
}
