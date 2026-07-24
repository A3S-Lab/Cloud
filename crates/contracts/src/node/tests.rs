use super::*;
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeLogChunk, RuntimeLogDiscontinuityReason, RuntimeLogStream, RuntimeUnitClass,
};
use chrono::{Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("docker").expect("valid Docker provider ID"),
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

fn gateway_snapshot(
    gateway_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    issued_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) -> GatewaySnapshot {
    GatewaySnapshot::new(
        gateway_id,
        revision,
        expected_revision,
        issued_at,
        expires_at,
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
fn log_batches_accept_gap_only_uploads_and_reject_cross_kind_sequence_conflicts() {
    let node_id = Uuid::now_v7();
    let mut batch = NodeLogChunkBatch {
        schema: NodeLogChunkBatch::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id,
        sent_at: Utc::now(),
        chunks: Vec::new(),
        gaps: vec![NodeLogGapReport {
            unit_id: "unit-1".into(),
            generation: 4,
            cursor: Some("provider-cursor".into()),
            sequence: 9,
            observed_at_ms: 1_000,
            reason: RuntimeLogDiscontinuityReason::CursorLost,
        }],
    };
    batch.validate().expect("valid gap-only batch");

    let receipt = NodeLogChunkReceipt {
        schema: NodeLogChunkReceipt::SCHEMA.into(),
        batch_id: batch.batch_id,
        node_id,
        accepted_chunks: 0,
        accepted_gaps: 1,
        replayed: false,
    };
    receipt.validate().expect("valid gap-only receipt");

    let data = "replacement log\n";
    batch.chunks.push(NodeLogChunkReport {
        unit_id: "unit-1".into(),
        generation: 4,
        chunk: RuntimeLogChunk {
            schema: RuntimeLogChunk::SCHEMA.into(),
            cursor: "replacement-cursor".into(),
            sequence: 9,
            observed_at_ms: 1_001,
            stream: RuntimeLogStream::Stdout,
            data: data.into(),
        },
        checksum: format!("sha256:{:x}", Sha256::digest(data.as_bytes())),
    });
    assert!(batch.validate().is_err());
}

#[test]
fn chunk_only_log_batches_keep_the_v1_wire_shape() {
    let data = "hello\n";
    let encoded = json!({
        "schema": NodeLogChunkBatch::SCHEMA,
        "batch_id": Uuid::now_v7(),
        "node_id": Uuid::now_v7(),
        "sent_at": Utc::now(),
        "chunks": [{
            "unit_id": "unit-1",
            "generation": 1,
            "chunk": {
                "schema": RuntimeLogChunk::SCHEMA,
                "cursor": "provider-cursor",
                "sequence": 1,
                "observed_at_ms": 1_000,
                "stream": "stdout",
                "data": data
            },
            "checksum": format!("sha256:{:x}", Sha256::digest(data.as_bytes()))
        }]
    });
    let batch: NodeLogChunkBatch =
        serde_json::from_value(encoded.clone()).expect("decode legacy chunk-only batch");
    batch.validate().expect("valid legacy chunk-only batch");
    assert!(batch.gaps.is_empty());
    assert_eq!(
        serde_json::to_value(batch).expect("encode chunk-only batch"),
        encoded
    );

    let receipt = json!({
        "schema": NodeLogChunkReceipt::SCHEMA,
        "batch_id": Uuid::now_v7(),
        "node_id": Uuid::now_v7(),
        "accepted_chunks": 1,
        "replayed": false
    });
    let decoded: NodeLogChunkReceipt =
        serde_json::from_value(receipt.clone()).expect("decode legacy chunk-only receipt");
    decoded.validate().expect("valid legacy chunk-only receipt");
    assert_eq!(decoded.accepted_gaps, 0);
    assert_eq!(
        serde_json::to_value(decoded).expect("encode chunk-only receipt"),
        receipt
    );
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
    let metadata = metadata(9);
    let snapshot = gateway_snapshot(
        metadata.node_id,
        4,
        Some(3),
        metadata.issued_at,
        metadata.not_after,
    );
    snapshot.validate().expect("valid Gateway snapshot");

    let command = NodeCommandEnvelope::new(
        metadata,
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
        gateway_id: snapshot.gateway_id,
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest.clone(),
        expires_at: snapshot.expires_at,
        state: GatewayAckState::Applied,
        ready: true,
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

    let invalid_compare_and_swap = GatewaySnapshot::new(
        snapshot.gateway_id,
        4,
        Some(4),
        snapshot.issued_at,
        snapshot.expires_at,
        "valid = true\n",
    );
    assert!(invalid_compare_and_swap.is_err());
}

#[test]
fn gateway_tls_snapshot_binds_one_closed_certificate_request() {
    let certificate_id = Uuid::now_v7();
    let certificate = GatewayCertificateRequest::new(
        certificate_id,
        vec!["*.example.com".into(), "api.internal.example.com".into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let acl = format!(
        r#"entrypoints "https" {{
  address = "0.0.0.0:443"
  tls {{
    cert_file = "{}"
    key_file = "{}"
  }}
}}
"#,
        certificate.certificate_file, certificate.private_key_file
    );
    let issued_at = Utc::now();
    let snapshot = GatewaySnapshot::new_with_certificate(
        Uuid::now_v7(),
        5,
        Some(4),
        issued_at,
        issued_at + Duration::minutes(10),
        acl,
        Some(certificate.clone()),
    )
    .expect("TLS snapshot");
    snapshot.validate().expect("valid TLS snapshot");

    let mut changed_certificate = snapshot.clone();
    changed_certificate
        .certificate_request
        .as_mut()
        .expect("certificate")
        .dns_names = vec!["other.example.com".into()];
    changed_certificate
        .validate()
        .expect("certificate remains structurally valid");
    let original_digest = NodeCommandPayload::GatewaySnapshotInstall {
        snapshot: Box::new(snapshot.clone()),
    }
    .digest()
    .expect("original payload digest");
    let changed_digest = NodeCommandPayload::GatewaySnapshotInstall {
        snapshot: Box::new(changed_certificate),
    }
    .digest()
    .expect("changed payload digest");
    assert_ne!(original_digest, changed_digest);

    let mut missing_reference = snapshot;
    missing_reference.acl = "management { enabled = true }\n".into();
    assert!(missing_reference.validate().is_err());
}

#[test]
fn gateway_certificate_request_rejects_ambiguous_names_and_paths() {
    let certificate_id = Uuid::now_v7();
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["*.example.com".into(), "api.example.com".into()],
        "/cert.pem",
        "/key.pem",
    )
    .is_ok());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["api.example.com".into(), "*.example.com".into()],
        "/cert.pem",
        "/key.pem",
    )
    .is_err());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["*.*.example.com".into()],
        "/cert.pem",
        "/key.pem",
    )
    .is_err());
    assert!(GatewayCertificateRequest::new(
        certificate_id,
        vec!["api.example.com".into()],
        "relative/cert.pem",
        "/key.pem",
    )
    .is_err());
}

#[test]
fn gateway_certificate_signing_contract_never_accepts_or_debugs_a_private_key() {
    let request = GatewayCertificateSigningRequest {
        schema: GatewayCertificateSigningRequest::SCHEMA.into(),
        certificate_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        csr_pem:
            "-----BEGIN CERTIFICATE REQUEST-----\ndGVzdA==\n-----END CERTIFICATE REQUEST-----\n"
                .into(),
        requested_at: Utc::now(),
    };
    request.validate().expect("signing request");
    let debug = format!("{request:?}");
    assert!(debug.contains("<redacted-csr>"));
    assert!(!debug.contains("dGVzdA"));

    let mut leaked = request;
    leaked.csr_pem =
        "-----BEGIN CERTIFICATE REQUEST-----\nPRIVATE KEY\n-----END CERTIFICATE REQUEST-----\n"
            .into();
    assert!(leaked.validate().is_err());
}
