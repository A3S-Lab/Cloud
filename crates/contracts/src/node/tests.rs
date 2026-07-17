use super::*;
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeUnitClass,
};
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("docker").unwrap(),
        provider_build: "docker-test".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None, NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
            ResourceControl::EphemeralStorage,
            ResourceControl::ExecutionTimeout,
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
        ],
    }
}

fn metadata(sequence: u64) -> NodeCommandMetadata {
    let issued_at = Utc::now();
    NodeCommandMetadata {
        command_id: Uuid::now_v7(),
        lease_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        sequence,
        aggregate_id: Uuid::now_v7(),
        issued_at,
        not_after: issued_at + Duration::seconds(30),
        correlation_id: Uuid::now_v7(),
    }
}

fn inspect_command(sequence: u64) -> NodeCommandEnvelope {
    NodeCommandEnvelope::new(
        metadata(sequence),
        NodeCommandPayload::RuntimeInspect {
            unit_id: "unit-1".into(),
            generation: 4,
        },
    )
    .expect("valid command")
}

fn gateway_snapshot(revision: u64, expected_revision: Option<u64>) -> GatewaySnapshot {
    GatewaySnapshot::new(
        revision,
        expected_revision,
        r#"entrypoint "https" {
  address = "0.0.0.0:443"
}
"#,
    )
    .expect("valid Gateway snapshot")
}

#[test]
fn enrollment_is_closed_and_requires_a_real_token_shape() {
    let request = NodeEnrollmentRequest {
        schema: NodeEnrollmentRequest::SCHEMA.into(),
        enrollment_token: format!("a3sn_{}", "a".repeat(64)),
        node_name: "worker-1".into(),
        agent_instance_id: Uuid::now_v7(),
        agent_version: "0.1.0".into(),
        csr_pem: "-----BEGIN CERTIFICATE REQUEST-----\nYWJj\n-----END CERTIFICATE REQUEST-----\n"
            .into(),
        runtime_capabilities: capabilities(),
    };
    request.validate().expect("valid enrollment request");

    let mut invalid = request.clone();
    invalid.enrollment_token = format!("a3sn_{}", "A".repeat(64));
    assert!(invalid.validate().is_err());

    let mut encoded = serde_json::to_value(request).expect("encode enrollment request");
    encoded
        .as_object_mut()
        .expect("request object")
        .insert("provider".into(), json!("docker"));
    assert!(serde_json::from_value::<NodeEnrollmentRequest>(encoded).is_err());
}

#[test]
fn command_digest_generation_and_expiry_are_bound() {
    let command = inspect_command(1);
    command.validate().expect("valid command");
    assert!(!command.is_expired_at(command.issued_at));
    assert!(command.is_expired_at(command.not_after));

    let mut digest_conflict = command.clone();
    digest_conflict.payload = NodeCommandPayload::RuntimeInspect {
        unit_id: "different-unit".into(),
        generation: 4,
    };
    assert_eq!(
        digest_conflict.validate().expect_err("digest conflict"),
        "command payload digest does not match its payload"
    );

    let mut generation_conflict = command;
    generation_conflict.generation += 1;
    assert_eq!(
        generation_conflict
            .validate()
            .expect_err("generation conflict"),
        "command generation does not match its payload"
    );
}

#[test]
fn acknowledgements_and_leases_fail_closed_on_identity_changes() {
    let first = inspect_command(7);
    let ack = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: first.command_id,
        lease_id: first.lease_id,
        node_id: first.node_id,
        sequence: first.sequence,
        payload_digest: first.payload_digest.clone(),
        completed_at: first.issued_at + Duration::milliseconds(10),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeInspected {
                inspection: a3s_runtime::contract::RuntimeInspection::NotFound {
                    schema: a3s_runtime::contract::RuntimeInspection::SCHEMA.into(),
                    unit_id: "unit-1".into(),
                    last_generation: Some(4),
                },
            }),
        },
    };
    ack.validate_against(&first).expect("matching ack");

    let mut wrong_result = ack.clone();
    wrong_result.outcome = NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::RuntimeInspected {
            inspection: a3s_runtime::contract::RuntimeInspection::NotFound {
                schema: a3s_runtime::contract::RuntimeInspection::SCHEMA.into(),
                unit_id: "different-unit".into(),
                last_generation: Some(4),
            },
        }),
    };
    assert!(wrong_result.validate_against(&first).is_err());

    let mut wrong_node = ack;
    wrong_node.node_id = Uuid::now_v7();
    assert!(wrong_node.validate_against(&first).is_err());

    let second = NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            sequence: 8,
            ..metadata(8)
        },
        NodeCommandPayload::RuntimeInspect {
            unit_id: "unit-2".into(),
            generation: 1,
        },
    )
    .expect("second command");
    let response = NodeCommandLeaseResponse {
        schema: NodeCommandLeaseResponse::SCHEMA.into(),
        lease_id: first.lease_id,
        node_id: first.node_id,
        agent_instance_id: Uuid::now_v7(),
        leased_until: Utc::now() + Duration::seconds(30),
        commands: vec![first, second],
    };
    assert!(response.validate(Utc::now()).is_err());
}

#[test]
fn observation_batches_bind_agent_and_node_identity() {
    let node_id = Uuid::now_v7();
    let instance_id = Uuid::now_v7();
    let observed_at = Utc::now();
    let heartbeat = NodeHeartbeat {
        schema: NodeHeartbeat::SCHEMA.into(),
        node_id,
        agent_instance_id: instance_id,
        observed_at,
        agent_version: "0.1.0".into(),
        runtime_capabilities: capabilities(),
    };
    let mut batch = NodeObservationBatch {
        schema: NodeObservationBatch::SCHEMA.into(),
        node_id,
        agent_instance_id: instance_id,
        sent_at: observed_at,
        heartbeat,
        observations: Vec::new(),
    };
    batch.validate().expect("valid observation batch");
    batch.agent_instance_id = Uuid::now_v7();
    assert!(batch.validate().is_err());
}

#[test]
fn node_protocol_errors_are_versioned_and_strict() {
    let error = NodeProtocolError::new(
        Uuid::now_v7(),
        NodeProtocolErrorCode::Conflict,
        "command acknowledgement conflicts with its durable result",
        false,
    )
    .expect("protocol error");
    error.validate().expect("valid protocol error");
    let encoded = serde_json::to_value(&error).expect("serialize protocol error");
    assert_eq!(encoded["schema"], NodeProtocolError::SCHEMA);
    assert_eq!(encoded["code"], "conflict");
    let decoded: NodeProtocolError =
        serde_json::from_value(encoded).expect("decode protocol error");
    assert_eq!(decoded, error);
}

#[test]
fn gateway_snapshot_commands_bind_the_complete_snapshot_and_exact_acknowledgement() {
    let snapshot = gateway_snapshot(4, Some(3));
    snapshot.validate().expect("valid Gateway snapshot");

    let command = NodeCommandEnvelope::new(
        metadata(9),
        NodeCommandPayload::GatewaySnapshotInstall {
            snapshot: Box::new(snapshot.clone()),
        },
    )
    .expect("Gateway install command");
    assert_eq!(command.generation, snapshot.revision);
    assert_eq!(command.payload_schema, GatewaySnapshot::SCHEMA);

    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command.command_id,
        node_id: command.node_id,
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest.clone(),
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: command.issued_at + Duration::milliseconds(10),
    };
    acknowledgement
        .validate_for(command.command_id, command.node_id, &snapshot)
        .expect("exact Gateway acknowledgement");

    let command_acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: acknowledgement.acknowledged_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::GatewaySnapshotInstalled {
                acknowledgement: acknowledgement.clone(),
            }),
        },
    };
    command_acknowledgement
        .validate_against(&command)
        .expect("Gateway command acknowledgement");

    let mut wrong_revision = acknowledgement;
    wrong_revision.revision += 1;
    assert!(wrong_revision
        .validate_for(command.command_id, command.node_id, &snapshot)
        .is_err());

    let mut wrong_digest = snapshot.clone();
    wrong_digest.acl.push_str("# changed\n");
    assert!(wrong_digest.validate().is_err());

    let invalid_compare_and_swap = GatewaySnapshot::new(4, Some(4), "valid = true\n");
    assert!(invalid_compare_and_swap.is_err());
}
