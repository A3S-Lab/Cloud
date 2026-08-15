use super::metadata;
use crate::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeDurableCellOperatorBindingV1, NodeDurableCellOperatorObservationV1,
};
use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

fn digest(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn binding(application_id: Uuid) -> NodeDurableCellOperatorBindingV1 {
    NodeDurableCellOperatorBindingV1 {
        schema: NodeDurableCellOperatorBindingV1::SCHEMA.into(),
        application_id,
        application_revision_id: Uuid::now_v7(),
        application_revision_number: 7,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        runtime_unit_id: "workload:durable-cell:revision:7".into(),
        runtime_generation: 7,
        runtime_spec_digest: digest('a'),
        service_profile_digest: digest('b'),
        service_template_digest: digest('c'),
        provider_artifact_digest: digest('d'),
        internal_service_port_name: "operator".into(),
    }
}

#[test]
fn operator_observation_is_bound_to_one_exact_runtime_generation() {
    let application_id = Uuid::now_v7();
    let binding = binding(application_id);
    let mut command_metadata = metadata(1);
    command_metadata.aggregate_id = application_id;
    let issued_at = command_metadata.issued_at;
    let envelope = NodeCommandEnvelope::new(
        command_metadata,
        NodeCommandPayload::DurableCellOperatorObserve {
            binding: Box::new(binding.clone()),
        },
    )
    .expect("valid Durable Cell operator command");
    assert_eq!(envelope.payload.kind(), "durable_cell_operator_observe");
    assert_eq!(envelope.generation, binding.runtime_generation);

    let observed_at_ms =
        u64::try_from(issued_at.timestamp_millis() + 1).expect("positive observation time");
    let observation = NodeDurableCellOperatorObservationV1 {
        schema: NodeDurableCellOperatorObservationV1::SCHEMA.into(),
        binding_digest: binding.digest().expect("binding digest"),
        runtime_unit_id: binding.runtime_unit_id.clone(),
        runtime_generation: binding.runtime_generation,
        runtime_spec_digest: binding.runtime_spec_digest.clone(),
        occupied: 3,
        evicting: 1,
        restoring: 2,
        activating: 1,
        activation_waiting: 4,
        capacity_waiting: 5,
        observed_at_ms,
    };
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: envelope.command_id,
        lease_id: envelope.lease_id,
        node_id: envelope.node_id,
        sequence: envelope.sequence,
        payload_digest: envelope.payload_digest.clone(),
        completed_at: Utc
            .timestamp_millis_opt(i64::try_from(observed_at_ms).expect("timestamp"))
            .single()
            .expect("valid completion time"),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::DurableCellOperatorObserved {
                observation: observation.clone(),
            }),
        },
    };
    acknowledgement
        .validate_against(&envelope)
        .expect("exact operator acknowledgement");

    let mut stale = observation;
    stale.runtime_generation += 1;
    assert!(stale.validate_for(&binding).is_err());

    let mut legacy = acknowledgement;
    legacy.schema = NodeCommandAck::LEGACY_SCHEMA.into();
    assert!(legacy.validate_against(&envelope).is_err());
}

#[test]
fn operator_contract_cannot_carry_provider_cell_names_or_raw_state() {
    let binding = binding(Uuid::now_v7());
    let encoded = json!({
        "schema": NodeDurableCellOperatorObservationV1::SCHEMA,
        "binding_digest": binding.digest().expect("binding digest"),
        "runtime_unit_id": binding.runtime_unit_id.clone(),
        "runtime_generation": binding.runtime_generation,
        "runtime_spec_digest": binding.runtime_spec_digest.clone(),
        "occupied": 1,
        "evicting": 0,
        "restoring": 0,
        "activating": 0,
        "activation_waiting": 0,
        "capacity_waiting": 0,
        "observed_at_ms": 1,
        "residents": ["tenant-secret-cell-name"]
    });
    assert!(serde_json::from_value::<NodeDurableCellOperatorObservationV1>(encoded).is_err());

    let mut foreign_aggregate = metadata(2);
    foreign_aggregate.aggregate_id = Uuid::now_v7();
    assert!(NodeCommandEnvelope::new(
        foreign_aggregate,
        NodeCommandPayload::DurableCellOperatorObserve {
            binding: Box::new(binding),
        },
    )
    .is_err());
}
