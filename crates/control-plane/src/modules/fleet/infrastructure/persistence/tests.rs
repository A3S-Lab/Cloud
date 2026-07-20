use super::InMemoryNodeRepository;
use crate::modules::fleet::domain::entities::{
    EnrollmentToken, NodeCertificate, NodeCertificateMaterial, NodeCommandDraft,
};
use crate::modules::fleet::domain::repositories::{
    ILogRetentionRepository, INodeControlRepository, INodeRepository,
    NodeCertificateRotationCompletion, NodeCertificateRotationDraft, NodeEnrollmentDraft,
    NodeHeartbeatUpdate, NodeLogBatchReceiptDraft, NodeLogBatchReplay, NodeLogChunkQuery,
    NodeLogChunkReceiptDraft, NodeStateChange,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeAvailability, NodeCapabilities, NodeName, NodeState,
};
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, IdempotencyRequest, NodeCertificateId, NodeCommandId, NodeId, OrganizationId,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, GatewaySnapshot, NodeCommandAck, NodeCommandFailure,
    NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeGatewayAck, NodeHeartbeat, NodeObservationBatch, RuntimeObservationReport,
};
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeObservation, RuntimeUnitClass, RuntimeUnitState,
};
use chrono::{Duration, Timelike, Utc};
use serde_json::json;
use uuid::Uuid;

mod support;

use support::*;

#[tokio::test]
async fn command_queue_is_sequenced_leased_redelivered_and_acknowledged_exactly() {
    let repository = InMemoryNodeRepository::new();
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 123)
        .expect("sub-microsecond command timestamp");
    let (node_id, agent_instance_id) = command_node(&repository, now).await;
    let aggregate_id = Uuid::now_v7();
    let first_draft = inspect_draft(
        NodeCommandId::new(),
        node_id,
        aggregate_id,
        "service-1",
        1,
        now,
    );
    let first = repository
        .enqueue_command(first_draft.clone())
        .await
        .expect("enqueue first command");
    assert_eq!(first.value.sequence, 1);
    assert!(!first.replayed);
    assert!(
        repository
            .enqueue_command(first_draft.clone())
            .await
            .expect("replay first command")
            .replayed
    );

    let mut conflicting_command_id = first_draft;
    conflicting_command_id.payload = NodeCommandPayload::RuntimeInspect {
        unit_id: "different-service".into(),
        generation: 1,
    };
    assert!(repository
        .enqueue_command(conflicting_command_id)
        .await
        .is_err());

    let second = repository
        .enqueue_command(inspect_draft(
            NodeCommandId::new(),
            node_id,
            aggregate_id,
            "service-1",
            1,
            now + Duration::milliseconds(1),
        ))
        .await
        .expect("enqueue second command");
    assert_eq!(second.value.sequence, 2);

    let request = NodeCommandLeaseRequest {
        schema: NodeCommandLeaseRequest::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id,
        after_sequence: 0,
        max_commands: 1,
        wait_ms: 0,
    };
    let first_lease_id = Uuid::now_v7();
    let first_lease = repository
        .lease_commands(
            &request,
            first_lease_id,
            now + Duration::seconds(1),
            now + Duration::seconds(11),
        )
        .await
        .expect("lease first command");
    assert_eq!(first_lease.commands.len(), 1);
    assert_eq!(first_lease.commands[0].sequence, 1);
    assert_eq!(first_lease.leased_until.nanosecond() % 1_000, 0);

    let blocked = repository
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(2),
            now + Duration::seconds(12),
        )
        .await
        .expect("respect active head lease");
    assert!(blocked.commands.is_empty());

    let mut wrong_lease_ack = inspected_ack(&first_lease.commands[0], now + Duration::seconds(3));
    wrong_lease_ack.lease_id = Uuid::now_v7();
    assert!(repository
        .acknowledge_command(wrong_lease_ack, now + Duration::seconds(3))
        .await
        .is_err());

    let first_ack = inspected_ack(&first_lease.commands[0], now + Duration::seconds(3));
    let accepted = repository
        .acknowledge_command(first_ack.clone(), now + Duration::seconds(3))
        .await
        .expect("acknowledge first command");
    assert!(!accepted.replayed);
    assert_eq!(accepted.value.completed_at.nanosecond() % 1_000, 0);
    let mut replay_ack = first_ack;
    replay_ack.completed_at = replay_ack
        .completed_at
        .with_nanosecond(replay_ack.completed_at.nanosecond() / 1_000 * 1_000 + 987)
        .expect("sub-microsecond acknowledgement replay timestamp");
    assert!(
        repository
            .acknowledge_command(replay_ack, now + Duration::seconds(4))
            .await
            .expect("replay acknowledgement")
            .replayed
    );
    assert_eq!(
        repository
            .command_acknowledgement(node_id, first.value.id)
            .await
            .expect("query command acknowledgement"),
        Some(accepted.value)
    );
    assert_eq!(
        repository
            .command_acknowledgement(NodeId::new(), first.value.id)
            .await
            .expect("isolate acknowledgement by node"),
        None
    );

    let second_lease = repository
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(4),
            now + Duration::seconds(14),
        )
        .await
        .expect("lease second command");
    assert_eq!(second_lease.commands[0].sequence, 2);
    let replacement_lease = repository
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(15),
            now + Duration::seconds(25),
        )
        .await
        .expect("redeliver expired lease");
    assert_eq!(
        replacement_lease.commands[0].command_id,
        second_lease.commands[0].command_id
    );
    assert_ne!(replacement_lease.lease_id, second_lease.lease_id);

    let stale_ack = inspected_ack(&second_lease.commands[0], now + Duration::seconds(13));
    assert!(repository
        .acknowledge_command(stale_ack, now + Duration::seconds(15))
        .await
        .is_err());
    let replacement_ack =
        inspected_ack(&replacement_lease.commands[0], now + Duration::seconds(16));
    repository
        .acknowledge_command(replacement_ack, now + Duration::seconds(16))
        .await
        .expect("acknowledge replacement lease");
}

#[tokio::test]
async fn expired_commands_are_leased_for_rejection_without_creating_a_sequence_gap() {
    let repository = InMemoryNodeRepository::new();
    let now = Utc::now();
    let (node_id, agent_instance_id) = command_node(&repository, now).await;
    let aggregate_id = Uuid::now_v7();
    let mut expired = inspect_draft(
        NodeCommandId::new(),
        node_id,
        aggregate_id,
        "expired-service",
        1,
        now - Duration::minutes(2),
    );
    expired.not_after = now - Duration::minutes(1);
    let expired = repository
        .enqueue_command(expired)
        .await
        .expect("enqueue expired command")
        .value;
    let following = repository
        .enqueue_command(inspect_draft(
            NodeCommandId::new(),
            node_id,
            aggregate_id,
            "following-service",
            1,
            now,
        ))
        .await
        .expect("enqueue following command")
        .value;

    let request = NodeCommandLeaseRequest {
        schema: NodeCommandLeaseRequest::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id,
        after_sequence: 0,
        max_commands: 10,
        wait_ms: 0,
    };
    let lease = repository
        .lease_commands(&request, Uuid::now_v7(), now, now + Duration::seconds(10))
        .await
        .expect("lease commands including expired head");
    assert_eq!(
        lease
            .commands
            .iter()
            .map(|command| command.sequence)
            .collect::<Vec<_>>(),
        vec![expired.sequence, following.sequence]
    );

    let expired_envelope = &lease.commands[0];
    repository
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: expired_envelope.command_id,
                lease_id: expired_envelope.lease_id,
                node_id: expired_envelope.node_id,
                sequence: expired_envelope.sequence,
                payload_digest: expired_envelope.payload_digest.clone(),
                completed_at: now + Duration::seconds(1),
                outcome: NodeCommandOutcome::Rejected {
                    failure: NodeCommandFailure {
                        code: "command_expired".into(),
                        message: "command expired before Runtime dispatch".into(),
                        retryable: false,
                    },
                },
            },
            now + Duration::seconds(1),
        )
        .await
        .expect("acknowledge expired command rejection");
}

#[tokio::test]
async fn observations_and_gateway_acknowledgements_are_atomic_and_replay_safe() {
    let repository = InMemoryNodeRepository::new();
    let now = Utc::now();
    let (node_id, agent_instance_id) = command_node(&repository, now).await;
    let observed_at = now + Duration::seconds(1);
    let observed_at = observed_at
        .with_nanosecond(observed_at.nanosecond() / 1_000 * 1_000 + 123)
        .expect("sub-microsecond observation timestamp");
    let report_id = Uuid::now_v7();
    let batch = NodeObservationBatch {
        schema: NodeObservationBatch::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id,
        sent_at: observed_at,
        heartbeat: NodeHeartbeat {
            schema: NodeHeartbeat::SCHEMA.into(),
            node_id: node_id.as_uuid(),
            agent_instance_id,
            observed_at,
            agent_version: "0.1.1".into(),
            runtime_capabilities: runtime_capabilities(),
        },
        observations: vec![RuntimeObservationReport {
            report_id,
            command_id: None,
            observed_at,
            observation: runtime_observation("service-1", 1, 1),
        }],
    };
    let accepted = repository
        .record_observations(batch.clone(), observed_at)
        .await
        .expect("record observation");
    assert_eq!(accepted.accepted_reports, 1);
    assert_eq!(accepted.replayed_reports, 0);
    let mut replay_batch = batch.clone();
    let replayed_at = observed_at
        .with_nanosecond(observed_at.nanosecond() / 1_000 * 1_000 + 987)
        .expect("sub-microsecond replay timestamp");
    replay_batch.sent_at = replayed_at;
    replay_batch.heartbeat.observed_at = replayed_at;
    replay_batch.observations[0].observed_at = replayed_at;
    let replayed = repository
        .record_observations(replay_batch, observed_at + Duration::milliseconds(1))
        .await
        .expect("replay observation");
    assert_eq!(replayed.accepted_reports, 0);
    assert_eq!(replayed.replayed_reports, 1);

    let mut conflict = batch;
    conflict.observations[0].observation.unit_id = "different-service".into();
    assert!(repository
        .record_observations(conflict, observed_at + Duration::milliseconds(2))
        .await
        .is_err());

    let snapshot =
        GatewaySnapshot::new(1, None, "management { enabled = true }\n").expect("Gateway snapshot");
    let gateway_command = repository
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id,
            aggregate_id: Uuid::now_v7(),
            payload: NodeCommandPayload::GatewaySnapshotInstall {
                snapshot: Box::new(snapshot.clone()),
            },
            issued_at: observed_at,
            not_after: observed_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        })
        .await
        .expect("enqueue Gateway command")
        .value;

    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: gateway_command.id.as_uuid(),
        node_id: node_id.as_uuid(),
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest,
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: observed_at + Duration::seconds(1),
    };
    assert!(
        !repository
            .record_gateway_acknowledgement(
                acknowledgement.clone(),
                observed_at + Duration::seconds(1),
            )
            .await
            .expect("record Gateway acknowledgement")
            .replayed
    );
    let mut acknowledgement_replay = acknowledgement.clone();
    acknowledgement_replay.acknowledged_at = acknowledgement_replay
        .acknowledged_at
        .with_nanosecond(acknowledgement_replay.acknowledged_at.nanosecond() / 1_000 * 1_000 + 987)
        .expect("sub-microsecond Gateway replay timestamp");
    assert!(
        repository
            .record_gateway_acknowledgement(
                acknowledgement_replay,
                observed_at + Duration::seconds(2),
            )
            .await
            .expect("replay Gateway acknowledgement")
            .replayed
    );
    let mut gateway_conflict = acknowledgement;
    gateway_conflict.state = GatewayAckState::Rejected;
    assert!(repository
        .record_gateway_acknowledgement(gateway_conflict, observed_at + Duration::seconds(3),)
        .await
        .is_err());
}

#[tokio::test]
async fn log_tombstone_compaction_is_bounded_coalesced_and_replay_safe() {
    let repository = InMemoryNodeRepository::new();
    let now = Utc::now();
    let (node_id, _) = command_node(&repository, now).await;
    let batch = NodeLogBatchReceiptDraft {
        batch_id: Uuid::now_v7(),
        node_id,
        payload_digest: format!("sha256:{}", "1".repeat(64)),
        sent_at: now,
        chunks: (0..=2)
            .map(|sequence| NodeLogChunkReceiptDraft {
                unit_id: "service-logs".into(),
                generation: 1,
                cursor: format!("cursor:{sequence}"),
                sequence,
                observed_at_ms: sequence,
                stream: "stdout".into(),
                checksum: format!("sha256:{}", "2".repeat(64)),
                object_key: format!("logs/{node_id}/{sequence}.json"),
            })
            .collect(),
        gaps: Vec::new(),
    };
    assert!(
        !repository
            .record_log_chunks(batch.clone(), now)
            .await
            .expect("record log batch")
            .replayed
    );
    let targets = repository
        .list_log_chunks_for_retention(now + Duration::seconds(1), 10)
        .await
        .expect("list log retention targets");
    assert_eq!(targets.len(), 3);
    for target in &targets {
        assert!(repository
            .mark_log_chunk_retained(target, now + Duration::seconds(1))
            .await
            .expect("mark log chunk retained"));
    }

    for through_sequence in 0_u64..=2 {
        let result = repository
            .compact_log_tombstones(now + Duration::seconds(2), now + Duration::seconds(3), 1)
            .await
            .expect("compact one bounded tombstone");
        assert_eq!(result.compacted_tombstones, 1);
        assert_eq!(result.created_ranges, 1);
        let ranges = repository
            .list_log_compaction_ranges(NodeLogChunkQuery {
                node_id,
                unit_id: "service-logs".into(),
                generation: 1,
                after_sequence: None,
                limit: 10,
                stream: Some(a3s_runtime::contract::RuntimeLogStream::Stderr),
            })
            .await
            .expect("list stream-independent compaction ranges");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].first_sequence, 0);
        assert_eq!(ranges[0].through_sequence, through_sequence);
    }

    let clipped = repository
        .list_log_compaction_ranges(NodeLogChunkQuery {
            node_id,
            unit_id: "service-logs".into(),
            generation: 1,
            after_sequence: Some(1),
            limit: 10,
            stream: None,
        })
        .await
        .expect("clip a compaction range after its cursor");
    assert_eq!(clipped.len(), 1);
    assert_eq!(clipped[0].first_sequence, 2);
    assert_eq!(clipped[0].through_sequence, 2);
    assert!(
        repository
            .replay_log_batch(NodeLogBatchReplay {
                batch_id: batch.batch_id,
                node_id,
                payload_digest: batch.payload_digest.clone(),
                sent_at: batch.sent_at,
                chunk_count: 3,
                gap_count: 0,
            })
            .await
            .expect("replay compacted batch")
            .expect("durable batch header")
            .replayed
    );
    assert!(
        repository
            .record_log_chunks(batch.clone(), now + Duration::seconds(10))
            .await
            .expect("record exact compacted batch replay")
            .replayed
    );

    let mut regressed = batch;
    regressed.batch_id = Uuid::now_v7();
    regressed.chunks.truncate(1);
    assert!(repository
        .record_log_chunks(regressed, now + Duration::seconds(11))
        .await
        .is_err());
}

#[tokio::test]
async fn enrollment_is_one_time_replayable_and_certificate_bound() {
    let repository = InMemoryNodeRepository::new();
    let organization_id = OrganizationId::new();
    let secret = format!("a3sn_{}", "a".repeat(64));
    let credential = EnrollmentTokenCredential::from_secret(&secret).expect("credential");
    let now = Utc::now();
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "worker enrollment",
        credential.clone(),
        now,
        now + Duration::minutes(10),
    )
    .expect("token");
    let idempotency = IdempotencyRequest::new("fleet/tokens", "token-1", b"worker enrollment")
        .expect("idempotency");
    let issued = repository
        .issue_enrollment_token(
            token.clone(),
            event(
                organization_id,
                token.id.as_uuid(),
                token.aggregate_version,
                "fleet.enrollment-token.issued",
            ),
            idempotency.clone(),
        )
        .await
        .expect("issue token");
    assert!(!issued.replayed);
    assert!(
        repository
            .issue_enrollment_token(
                token.clone(),
                event(
                    organization_id,
                    token.id.as_uuid(),
                    token.aggregate_version,
                    "fleet.enrollment-token.issued",
                ),
                idempotency,
            )
            .await
            .expect("replay token")
            .replayed
    );

    let draft = NodeEnrollmentDraft {
        proposed_node_id: NodeId::new(),
        name: NodeName::new("worker-1").expect("node name"),
        agent_instance_id: Uuid::now_v7(),
        agent_version: "0.1.0".into(),
        capabilities: capabilities("docker-test"),
        request_digest: format!("sha256:{}", "1".repeat(64)),
        requested_at: now + Duration::seconds(1),
    };
    let reserved = repository
        .reserve_enrollment(&credential, draft.clone())
        .await
        .expect("reserve enrollment");
    assert!(!reserved.replayed);
    assert!(reserved.certificate.is_none());

    let mut retry = draft.clone();
    retry.proposed_node_id = NodeId::new();
    let replay = repository
        .reserve_enrollment(&credential, retry)
        .await
        .expect("replay reservation");
    assert!(replay.replayed);
    assert_eq!(replay.node.id, reserved.node.id);

    let mut conflict = draft;
    conflict.request_digest = format!("sha256:{}", "2".repeat(64));
    assert!(repository
        .reserve_enrollment(&credential, conflict)
        .await
        .is_err());

    let leaf = certificate(reserved.node.id, 'b', now + Duration::seconds(1));
    let completed = repository
        .complete_enrollment(
            reserved.enrollment_token.id,
            reserved.node.id,
            &format!("sha256:{}", "1".repeat(64)),
            leaf.clone(),
            event(
                organization_id,
                reserved.node.id.as_uuid(),
                reserved.node.aggregate_version,
                "fleet.node.enrolled",
            ),
        )
        .await
        .expect("complete enrollment");
    assert_eq!(completed.certificate, Some(leaf.clone()));
    assert_eq!(
        repository
            .authenticate_certificate(&leaf.fingerprint, now + Duration::seconds(2))
            .await
            .expect("authenticate")
            .id,
        reserved.node.id
    );
}

#[tokio::test]
async fn heartbeat_draining_rotation_and_revocation_preserve_node_identity() {
    let repository = InMemoryNodeRepository::new();
    let organization_id = OrganizationId::new();
    let secret = format!("a3sn_{}", "c".repeat(64));
    let credential = EnrollmentTokenCredential::from_secret(&secret).expect("credential");
    let now = Utc::now();
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "worker",
        credential.clone(),
        now,
        now + Duration::minutes(10),
    )
    .expect("token");
    repository
        .issue_enrollment_token(
            token.clone(),
            event(
                organization_id,
                token.id.as_uuid(),
                token.aggregate_version,
                "fleet.enrollment-token.issued",
            ),
            IdempotencyRequest::new("fleet/tokens", "token-2", b"worker").expect("idempotency"),
        )
        .await
        .expect("issue token");
    let request_digest = format!("sha256:{}", "3".repeat(64));
    let reservation = repository
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new("worker-2").expect("name"),
                agent_instance_id: Uuid::now_v7(),
                agent_version: "0.1.0".into(),
                capabilities: capabilities("build-1"),
                request_digest: request_digest.clone(),
                requested_at: now,
            },
        )
        .await
        .expect("reserve");
    let first = certificate(reservation.node.id, 'd', now);
    repository
        .complete_enrollment(
            reservation.enrollment_token.id,
            reservation.node.id,
            &request_digest,
            first.clone(),
            event(
                organization_id,
                reservation.node.id.as_uuid(),
                reservation.node.aggregate_version,
                "fleet.node.enrolled",
            ),
        )
        .await
        .expect("complete");

    let online = repository
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id: Uuid::now_v7(),
            agent_version: "0.1.1".into(),
            capabilities: capabilities("build-2"),
            observed_at: now + Duration::seconds(1),
        })
        .await
        .expect("heartbeat");
    assert_eq!(online.state, NodeState::Ready);
    assert!(online.accepts_new_work_at(now + Duration::seconds(2), Duration::seconds(10)));
    assert_eq!(
        online.availability_at(now + Duration::seconds(20), Duration::seconds(10)),
        NodeAvailability::Offline
    );

    let replacement = certificate(reservation.node.id, 'e', now + Duration::seconds(2));
    let rotation_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/nodes/{}/certificates",
            reservation.node.id
        ),
        "rotate-first",
        b"replacement-e",
    )
    .expect("rotation idempotency");
    let rotation = repository
        .reserve_certificate_rotation(
            organization_id,
            reservation.node.id,
            first.id,
            NodeCertificateRotationDraft {
                replacement_certificate_id: replacement.id,
                requested_at: now + Duration::seconds(2),
            },
            rotation_idempotency.clone(),
        )
        .await
        .expect("reserve rotation");
    assert!(rotation.replacement.is_none());
    let rotated = repository
        .complete_certificate_rotation(NodeCertificateRotationCompletion {
            organization_id,
            node_id: reservation.node.id,
            current_certificate_id: first.id,
            replacement: replacement.clone(),
            rotated_at: now + Duration::seconds(2),
            event: event(
                organization_id,
                reservation.node.id.as_uuid(),
                online.aggregate_version,
                "fleet.node.certificate-rotated",
            ),
            idempotency: rotation_idempotency.clone(),
        })
        .await
        .expect("complete rotation");
    assert_eq!(rotated.replacement, Some(replacement.clone()));
    let replay = repository
        .reserve_certificate_rotation(
            organization_id,
            reservation.node.id,
            first.id,
            NodeCertificateRotationDraft {
                replacement_certificate_id: replacement.id,
                requested_at: now + Duration::seconds(2),
            },
            rotation_idempotency,
        )
        .await
        .expect("replay rotation");
    assert!(replay.replayed);
    assert_eq!(replay.replacement, Some(replacement.clone()));
    assert!(repository
        .authenticate_certificate(&first.fingerprint, now + Duration::seconds(3))
        .await
        .is_err());
    assert_eq!(
        repository
            .authenticate_rotation_certificate(&first.fingerprint, now + Duration::seconds(3), now,)
            .await
            .expect("rotation replay authentication")
            .id,
        reservation.node.id
    );
    assert!(repository
        .authenticate_rotation_certificate(
            &first.fingerprint,
            now + Duration::seconds(20),
            now + Duration::seconds(10),
        )
        .await
        .is_err());
    assert_eq!(
        repository
            .authenticate_certificate(&replacement.fingerprint, now + Duration::seconds(3))
            .await
            .expect("replacement auth")
            .id,
        reservation.node.id
    );

    let draining = repository
        .set_state(NodeStateChange {
            organization_id,
            node_id: reservation.node.id,
            state: NodeState::Draining,
            expected_version: rotated.node.aggregate_version,
            changed_at: now + Duration::seconds(3),
            event: event(
                organization_id,
                reservation.node.id.as_uuid(),
                rotated.node.aggregate_version + 1,
                "fleet.node.state-changed",
            ),
            idempotency: IdempotencyRequest::new(
                format!("organizations/{organization_id}/nodes"),
                "drain-node",
                b"draining",
            )
            .expect("state idempotency"),
        })
        .await
        .expect("drain")
        .value;
    assert!(!draining.accepts_new_work_at(now + Duration::seconds(3), Duration::seconds(10)));
    let revoked = repository
        .set_state(NodeStateChange {
            organization_id,
            node_id: reservation.node.id,
            state: NodeState::Revoked,
            expected_version: draining.aggregate_version,
            changed_at: now + Duration::seconds(4),
            event: event(
                organization_id,
                reservation.node.id.as_uuid(),
                draining.aggregate_version + 1,
                "fleet.node.state-changed",
            ),
            idempotency: IdempotencyRequest::new(
                format!("organizations/{organization_id}/nodes"),
                "revoke-node",
                b"revoked",
            )
            .expect("state idempotency"),
        })
        .await
        .expect("revoke")
        .value;
    assert_eq!(revoked.state, NodeState::Revoked);
    assert!(repository
        .authenticate_certificate(&replacement.fingerprint, now + Duration::seconds(5))
        .await
        .is_err());
}

#[tokio::test]
async fn heartbeat_and_state_replays_are_exact_and_do_not_advance_versions() {
    let repository = InMemoryNodeRepository::new();
    let organization_id = OrganizationId::new();
    let secret = format!("a3sn_{}", "f".repeat(64));
    let credential = EnrollmentTokenCredential::from_secret(&secret).expect("credential");
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 123)
        .expect("sub-microsecond Fleet timestamp");
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "replay worker",
        credential.clone(),
        now,
        now + Duration::minutes(10),
    )
    .expect("token");
    repository
        .issue_enrollment_token(
            token.clone(),
            event(
                organization_id,
                token.id.as_uuid(),
                token.aggregate_version,
                "fleet.enrollment-token.issued",
            ),
            IdempotencyRequest::new("fleet/tokens", "token-replay", b"replay worker")
                .expect("idempotency"),
        )
        .await
        .expect("issue token");
    let request_digest = format!("sha256:{}", "6".repeat(64));
    let agent_instance_id = Uuid::now_v7();
    let reservation = repository
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new("worker-replay").expect("name"),
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: capabilities("build-replay"),
                request_digest: request_digest.clone(),
                requested_at: now,
            },
        )
        .await
        .expect("reserve");
    let leaf = certificate(reservation.node.id, 'f', now);
    repository
        .complete_enrollment(
            token.id,
            reservation.node.id,
            &request_digest,
            leaf.clone(),
            event(
                organization_id,
                reservation.node.id.as_uuid(),
                reservation.node.aggregate_version,
                "fleet.node.enrolled",
            ),
        )
        .await
        .expect("complete");
    assert_eq!(
        repository
            .find_active_certificate(organization_id, reservation.node.id)
            .await
            .expect("active certificate"),
        leaf
    );

    let observed_at = now + Duration::seconds(1);
    let heartbeat = NodeHeartbeatUpdate {
        node_id: reservation.node.id,
        agent_instance_id,
        agent_version: "0.1.0".into(),
        capabilities: capabilities("build-replay"),
        observed_at,
    };
    let accepted = repository
        .record_heartbeat(heartbeat.clone())
        .await
        .expect("heartbeat");
    assert_eq!(accepted.last_observed_at.nanosecond() % 1_000, 0);
    let mut replay_heartbeat = heartbeat.clone();
    replay_heartbeat.observed_at = replay_heartbeat
        .observed_at
        .with_nanosecond(replay_heartbeat.observed_at.nanosecond() / 1_000 * 1_000 + 987)
        .expect("sub-microsecond heartbeat replay timestamp");
    let replayed = repository
        .record_heartbeat(replay_heartbeat)
        .await
        .expect("heartbeat replay");
    assert_eq!(replayed, accepted);

    let mut conflicting_heartbeat = heartbeat;
    conflicting_heartbeat.agent_version = "0.1.1".into();
    assert!(matches!(
        repository.record_heartbeat(conflicting_heartbeat).await,
        Err(crate::modules::shared_kernel::domain::RepositoryError::Conflict(_))
    ));

    let state_idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/nodes"),
        "drain-replay",
        b"draining",
    )
    .expect("state idempotency");
    let changed = repository
        .set_state(NodeStateChange {
            organization_id,
            node_id: reservation.node.id,
            state: NodeState::Draining,
            expected_version: accepted.aggregate_version,
            changed_at: observed_at + Duration::seconds(1),
            event: event(
                organization_id,
                reservation.node.id.as_uuid(),
                accepted.aggregate_version + 1,
                "fleet.node.state-changed",
            ),
            idempotency: state_idempotency.clone(),
        })
        .await
        .expect("state change");
    assert!(!changed.replayed);
    let replay = repository
        .set_state(NodeStateChange {
            organization_id,
            node_id: reservation.node.id,
            state: NodeState::Draining,
            expected_version: accepted.aggregate_version,
            changed_at: observed_at + Duration::seconds(1),
            event: event(
                organization_id,
                reservation.node.id.as_uuid(),
                accepted.aggregate_version + 1,
                "fleet.node.state-changed",
            ),
            idempotency: state_idempotency,
        })
        .await
        .expect("state replay");
    assert!(replay.replayed);
    assert_eq!(replay.value, changed.value);

    let conflict = IdempotencyRequest::new(
        format!("organizations/{organization_id}/nodes"),
        "drain-replay",
        b"ready",
    )
    .expect("conflicting idempotency");
    assert_eq!(
        repository
            .set_state(NodeStateChange {
                organization_id,
                node_id: reservation.node.id,
                state: NodeState::Ready,
                expected_version: changed.value.aggregate_version,
                changed_at: observed_at + Duration::seconds(2),
                event: event(
                    organization_id,
                    reservation.node.id.as_uuid(),
                    changed.value.aggregate_version + 1,
                    "fleet.node.state-changed",
                ),
                idempotency: conflict,
            })
            .await,
        Err(crate::modules::shared_kernel::domain::RepositoryError::IdempotencyConflict)
    );
}
