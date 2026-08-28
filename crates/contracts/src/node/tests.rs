use super::*;
use crate::{
    NodeResourceClaimBinding, NodeResourceClaimPrepare, NodeResourceClaimPrepared,
    NodeResourceClaimRelease, NodeResourceClaimReleased, ResourceAllocation, ResourceKind,
    ResourceSlotBinding, ResourceUnit,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceControl, ResourceLimits, RestartPolicy,
    RuntimeApplyRequest, RuntimeCapabilities, RuntimeEvidence, RuntimeFeature,
    RuntimeHealthObservation, RuntimeHealthState, RuntimeLogChunk, RuntimeLogDiscontinuityReason,
    RuntimeLogStream, RuntimeNetworkSpec, RuntimeObservation, RuntimeProcessSpec, RuntimeUnitClass,
    RuntimeUnitSpec, RuntimeUnitState,
};
use chrono::{Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("a3s-box").expect("valid A3S Box provider ID"),
        provider_build: "a3s-box-test".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Sandbox],
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
            RuntimeFeature::ServiceTcp,
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

fn resource_bound_runtime_spec() -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: "workload:resource-bound:revision:1".into(),
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: format!(
                "oci://registry.example/a3s/resource-bound@sha256:{}",
                "a".repeat(64)
            ),
            digest: format!("sha256:{}", "a".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: RuntimeProcessSpec {
            command: vec!["/bin/service".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Container,
        health: None,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    }
}

fn resource_bound_runtime_observation(spec: &RuntimeUnitSpec) -> RuntimeObservation {
    let spec_digest = spec.digest().expect("Runtime spec digest");
    RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec_digest.clone(),
        class: spec.class,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("provider-resource-1".into()),
        provider_build: Some("provider-build-1".into()),
        observed_at_ms: 1_000,
        started_at_ms: Some(1_000),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: RuntimeHealthState::Healthy,
            checked_at_ms: 1_000,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "provider-build-1".into(),
            spec_digest,
            semantics_profile_digest: None,
            claims: BTreeMap::new(),
        }),
        provider_attestation: None,
        failure: None,
    }
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
        .insert("provider".into(), json!("a3s-box"));
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
fn resource_claim_commands_bind_inventory_runtime_and_exact_agent_evidence() {
    let node_id = Uuid::now_v7();
    let agent_instance_id = Uuid::now_v7();
    let claim_id = Uuid::now_v7();
    let now = Utc::now();
    let inventory = NodeResourceInventory::new(
        node_id,
        agent_instance_id,
        7,
        now,
        vec![
            NodeResourceSlot::new(
                ResourceKind::Cpu,
                "cpu/shared",
                ResourceAllocation::Scalar {
                    amount: 4_000,
                    unit: ResourceUnit::MilliCpu,
                },
            )
            .expect("CPU inventory"),
            NodeResourceSlot::new(
                ResourceKind::Memory,
                "memory/system",
                ResourceAllocation::Scalar {
                    amount: 8 * 1024 * 1024,
                    unit: ResourceUnit::Byte,
                },
            )
            .expect("memory inventory"),
        ],
    )
    .expect("resource inventory");
    let spec = resource_bound_runtime_spec();
    let binding = NodeResourceClaimBinding {
        schema: NodeResourceClaimBinding::SCHEMA.into(),
        claim_id,
        node_id,
        agent_instance_id,
        inventory_generation: inventory.generation,
        inventory_digest: inventory.digest.clone(),
        runtime_unit_id: spec.unit_id.clone(),
        runtime_generation: spec.generation,
        topology_digest: format!("sha256:{}", "b".repeat(64)),
        slots: vec![
            ResourceSlotBinding {
                kind: ResourceKind::Cpu,
                stable_resource_id: "cpu/shared".into(),
                allocation: ResourceAllocation::Scalar {
                    amount: spec.resources.cpu_millis,
                    unit: ResourceUnit::MilliCpu,
                },
                slot_generation: 3,
                fence_token: Uuid::now_v7(),
            },
            ResourceSlotBinding {
                kind: ResourceKind::Memory,
                stable_resource_id: "memory/system".into(),
                allocation: ResourceAllocation::Scalar {
                    amount: spec.resources.memory_bytes,
                    unit: ResourceUnit::Byte,
                },
                slot_generation: 4,
                fence_token: Uuid::now_v7(),
            },
        ],
    };
    binding
        .validate_inventory(&inventory)
        .expect("inventory-bound claim");
    binding
        .validate_runtime_spec(&spec)
        .expect("Runtime-bound claim");

    let prepare = NodeResourceClaimPrepare {
        schema: NodeResourceClaimPrepare::SCHEMA.into(),
        claim_generation: 1,
        claim_digest: format!("sha256:{}", "c".repeat(64)),
        binding: binding.clone(),
    };
    let mut prepare_metadata = metadata(1);
    prepare_metadata.node_id = node_id;
    prepare_metadata.aggregate_id = claim_id;
    let prepare_command = NodeCommandEnvelope::new(
        prepare_metadata,
        NodeCommandPayload::ResourceClaimPrepare {
            request: Box::new(prepare.clone()),
        },
    )
    .expect("resource claim prepare command");
    let prepared = NodeResourceClaimPrepared::new(
        &prepare,
        prepare_command.issued_at + Duration::milliseconds(1),
    )
    .expect("prepared evidence");
    let prepare_ack = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: prepare_command.command_id,
        lease_id: prepare_command.lease_id,
        node_id,
        sequence: prepare_command.sequence,
        payload_digest: prepare_command.payload_digest.clone(),
        completed_at: prepared.prepared_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::ResourceClaimPrepared {
                prepared: prepared.clone(),
            }),
        },
    };
    prepare_ack
        .validate_against(&prepare_command)
        .expect("exact prepare acknowledgement");

    let mut observation = resource_bound_runtime_observation(&spec);
    binding
        .bind_runtime_observation(&mut observation)
        .expect("bind Runtime observation");
    binding
        .validate_runtime_observation(&observation)
        .expect("allocation-binding evidence");
    let mut apply_metadata = metadata(2);
    apply_metadata.node_id = node_id;
    let apply_command = NodeCommandEnvelope::new(
        apply_metadata,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: "resource-bound-apply".into(),
                deadline_at_ms: None,
                spec,
            }),
            resource_claim: Some(Box::new(binding.clone())),
        },
    )
    .expect("resource-bound Runtime apply");
    let apply_ack = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: apply_command.command_id,
        lease_id: apply_command.lease_id,
        node_id,
        sequence: apply_command.sequence,
        payload_digest: apply_command.payload_digest.clone(),
        completed_at: apply_command.issued_at + Duration::milliseconds(1),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeApplied {
                observation: Box::new(observation),
            }),
        },
    };
    apply_ack
        .validate_against(&apply_command)
        .expect("resource-bound Runtime acknowledgement");

    let release = NodeResourceClaimRelease {
        schema: NodeResourceClaimRelease::SCHEMA.into(),
        claim_generation: 2,
        claim_digest: format!("sha256:{}", "d".repeat(64)),
        binding,
    };
    let mut release_metadata = metadata(3);
    release_metadata.node_id = node_id;
    release_metadata.aggregate_id = claim_id;
    let release_command = NodeCommandEnvelope::new(
        release_metadata,
        NodeCommandPayload::ResourceClaimRelease {
            request: Box::new(release.clone()),
        },
    )
    .expect("resource claim release command");
    let released = NodeResourceClaimReleased::new(
        &release,
        release_command.issued_at + Duration::milliseconds(1),
    )
    .expect("released evidence");
    let release_ack = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: release_command.command_id,
        lease_id: release_command.lease_id,
        node_id,
        sequence: release_command.sequence,
        payload_digest: release_command.payload_digest.clone(),
        completed_at: released.released_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::ResourceClaimReleased {
                released: released.clone(),
            }),
        },
    };
    release_ack
        .validate_against(&release_command)
        .expect("exact release acknowledgement");

    let mut changed = released;
    changed.slots[0].slot_generation += 1;
    let mut changed_ack = release_ack;
    changed_ack.outcome = NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::ResourceClaimReleased { released: changed }),
    };
    assert!(changed_ack.validate_against(&release_command).is_err());
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
fn resource_inventory_is_canonical_digest_addressed_and_versions_heartbeat_explicitly() {
    let node_id = Uuid::now_v7();
    let instance_id = Uuid::now_v7();
    let observed_at = Utc::now();
    let inventory = NodeResourceInventory::new(
        node_id,
        instance_id,
        1,
        observed_at,
        vec![
            NodeResourceSlot::new(
                ResourceKind::Memory,
                "memory/system",
                ResourceAllocation::Scalar {
                    amount: 8 * 1024 * 1024 * 1024,
                    unit: ResourceUnit::Byte,
                },
            )
            .expect("memory slot"),
            NodeResourceSlot::new(
                ResourceKind::Cpu,
                "cpu/shared",
                ResourceAllocation::Scalar {
                    amount: 4_000,
                    unit: ResourceUnit::MilliCpu,
                },
            )
            .expect("CPU slot"),
        ],
    )
    .expect("resource inventory");
    assert_eq!(inventory.slots[0].kind, ResourceKind::Cpu);
    inventory.validate().expect("valid inventory");
    let same_content = NodeResourceInventory::new(
        Uuid::now_v7(),
        Uuid::now_v7(),
        7,
        observed_at + Duration::seconds(1),
        inventory.slots.clone(),
    )
    .expect("same inventory content");
    assert_eq!(same_content.digest, inventory.digest);

    let mut changed = inventory.clone();
    changed.slots[0].allocation = ResourceAllocation::Scalar {
        amount: 2_000,
        unit: ResourceUnit::MilliCpu,
    };
    assert_eq!(
        changed.validate().expect_err("stale digest"),
        "node inventory digest does not match its canonical slots"
    );

    let batch = NodeObservationBatchV2 {
        schema: NodeObservationBatchV2::SCHEMA.into(),
        node_id,
        agent_instance_id: instance_id,
        sent_at: observed_at,
        heartbeat: NodeHeartbeatV2 {
            schema: NodeHeartbeatV2::SCHEMA.into(),
            node_id,
            agent_instance_id: instance_id,
            observed_at,
            agent_version: "0.1.0".into(),
            runtime_capabilities: capabilities(),
            inventory: inventory.reference(),
        },
        observations: Vec::new(),
    };
    batch.validate().expect("v2 observation batch");
    let encoded = serde_json::to_value(&batch).expect("encode v2 batch");
    assert!(matches!(
        serde_json::from_value::<NodeObservationBatchEnvelope>(encoded).expect("decode v2 batch"),
        NodeObservationBatchEnvelope::V2(_)
    ));

    let mut inventory_value = serde_json::to_value(&inventory).expect("encode inventory");
    inventory_value
        .as_object_mut()
        .expect("inventory object")
        .insert("accelerators".into(), json!([]));
    assert!(serde_json::from_value::<NodeResourceInventory>(inventory_value).is_err());
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

mod agent_provider_tests;
mod code_agent_tests;
mod durable_cell_tests;
mod gateway_tests;
mod plugin_host_tests;
mod session_tests;
