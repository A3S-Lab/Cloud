use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeEnrollmentRequest, NodeEnrollmentResponse,
    NodeGatewayAck, NodeHeartbeat, NodeObservationBatch, RuntimeObservationReport,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::{
    NodeCertificate, NodeCommandDraft,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeHeartbeatUpdate, NodeLogBatchReceiptDraft,
    NodeLogChunkQuery, NodeLogChunkReceiptDraft,
};
use a3s_cloud_control_plane::modules::fleet::domain::services::{
    CertificateAuthorityError, ICertificateAuthority, NodeCertificateRequest,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{NodeCapabilities, NodeState};
use a3s_cloud_control_plane::modules::fleet::infrastructure::LocalCertificateAuthority;
use a3s_cloud_control_plane::modules::fleet::{
    ChangeNodeState, ChangeNodeStateHandler, EnrollNode, EnrollNodeHandler, IssueEnrollmentToken,
    IssueEnrollmentTokenHandler, PostgresNodeRepository, RotateNodeCertificate,
    RotateNodeCertificateHandler,
};
use a3s_cloud_control_plane::modules::identity::domain::repositories::IOrganizationRepository;
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    NodeCertificateId, NodeCommandId, NodeId, OrganizationId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeLogStream, RuntimeObservation, RuntimeUnitClass, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::{Duration, Timelike, Utc};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

struct FailFirstRevokeAuthority {
    inner: LocalCertificateAuthority,
    fail_revoke: AtomicBool,
}

#[async_trait]
impl ICertificateAuthority for FailFirstRevokeAuthority {
    async fn issue(
        &self,
        request: NodeCertificateRequest,
    ) -> Result<NodeCertificate, CertificateAuthorityError> {
        self.inner.issue(request).await
    }

    async fn revoke(&self, certificate: &NodeCertificate) -> Result<(), CertificateAuthorityError> {
        if self.fail_revoke.swap(false, Ordering::SeqCst) {
            return Err(CertificateAuthorityError::Unavailable(
                "injected revocation interruption".into(),
            ));
        }
        self.inner.revoke(certificate).await
    }

    async fn health(&self) -> Result<bool, CertificateAuthorityError> {
        self.inner.health().await
    }
}

pub async fn exercise_fleet(
    executor: &PostgresExecutor,
    organization_uuid: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::from_uuid(organization_uuid);
    let identity = Arc::new(PostgresIdentityRepository::new(executor.clone()));
    let organizations: Arc<dyn IOrganizationRepository> = identity;
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let directory = tempfile::tempdir()?;
    let certificate_authority = Arc::new(FailFirstRevokeAuthority {
        inner: LocalCertificateAuthority::load_or_create(directory.path())?,
        fail_revoke: AtomicBool::new(true),
    });
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 789)
        .expect("sub-microsecond Fleet timestamp");
    let token_secret = format!("a3sn_{}", "d".repeat(64));
    let issue_handler = IssueEnrollmentTokenHandler::new(organizations, nodes.clone());
    let issue = IssueEnrollmentToken {
        organization_id,
        name: "postgres worker".into(),
        token_secret: token_secret.clone(),
        expires_at: now + Duration::minutes(10),
        idempotency_key: "postgres-fleet-token".into(),
        request_id: Uuid::now_v7(),
        requested_at: now,
    };
    let (left, right) = tokio::join!(
        issue_handler.execute(issue.clone(), context()),
        issue_handler.execute(issue, context())
    );
    let left = left??;
    let right = right??;
    assert_ne!(left.replayed, right.replayed);
    assert_eq!(left.enrollment_token.id, right.enrollment_token.id);

    let enrollment_request = NodeEnrollmentRequest {
        schema: NodeEnrollmentRequest::SCHEMA.into(),
        enrollment_token: token_secret,
        node_name: "postgres-worker-1".into(),
        agent_instance_id: Uuid::now_v7(),
        agent_version: "0.1.0".into(),
        csr_pem: csr(),
        runtime_capabilities: capabilities(),
    };
    let enroll_handler = EnrollNodeHandler::new(
        nodes.clone(),
        certificate_authority.clone(),
        Duration::hours(1),
        15 * 60 * 1_000,
        1_000,
        1_000,
    )
    .expect("enrollment handler policy");
    let enroll = EnrollNode {
        request: enrollment_request.clone(),
        request_id: Uuid::now_v7(),
        received_at: now + Duration::seconds(1),
    };
    let (left, right) = tokio::join!(
        enroll_handler.execute(enroll.clone(), context()),
        enroll_handler.execute(enroll, context())
    );
    let left = left??;
    let right = right??;
    assert_eq!(left.response, right.response);
    assert_eq!(left.response.schema, NodeEnrollmentResponse::SCHEMA);
    assert_eq!(left.response.certificate.issued_at.nanosecond() % 1_000, 0);
    assert_eq!(left.response.certificate.expires_at.nanosecond() % 1_000, 0);
    assert_ne!(left.replayed, right.replayed);

    let node_id = NodeId::from_uuid(left.response.node_id);
    assert!(matches!(
        nodes.find(OrganizationId::new(), node_id).await,
        Err(RepositoryError::NotFound)
    ));
    let runtime_capabilities = capabilities();
    let node_capabilities = NodeCapabilities::new(
        runtime_capabilities.provider_id.to_string(),
        runtime_capabilities.provider_build.clone(),
        serde_json::to_value(runtime_capabilities)?,
    )
    .expect("node capabilities");
    let heartbeat = NodeHeartbeatUpdate {
        node_id,
        agent_instance_id: enrollment_request.agent_instance_id,
        agent_version: enrollment_request.agent_version,
        capabilities: node_capabilities,
        observed_at: now + Duration::seconds(2),
    };
    let ready = nodes.record_heartbeat(heartbeat.clone()).await?;
    assert_eq!(ready.state, NodeState::Ready);
    assert_eq!(ready.last_observed_at.nanosecond() % 1_000, 0);
    assert_eq!(nodes.record_heartbeat(heartbeat.clone()).await?, ready);
    let mut conflicting = heartbeat;
    conflicting.agent_version = "0.1.1".into();
    assert!(matches!(
        nodes.record_heartbeat(conflicting).await,
        Err(RepositoryError::Conflict(_))
    ));

    exercise_command_control(
        executor,
        nodes.as_ref(),
        node_id,
        enrollment_request.agent_instance_id,
        now,
    )
    .await?;
    exercise_observation_control(
        executor,
        nodes.as_ref(),
        node_id,
        enrollment_request.agent_instance_id,
        now,
    )
    .await?;

    let rotate_handler = RotateNodeCertificateHandler::new(
        nodes.clone(),
        certificate_authority.clone(),
        Duration::hours(1),
    )
    .expect("rotation handler policy");
    let rotate = RotateNodeCertificate {
        organization_id,
        node_id,
        current_certificate_id: NodeCertificateId::from_uuid(
            left.response.certificate.certificate_id,
        ),
        csr_pem: csr(),
        idempotency_key: "postgres-fleet-rotation".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(3),
    };
    let interrupted = rotate_handler.execute(rotate.clone(), context()).await?;
    assert!(matches!(interrupted, Err(ApplicationError::Internal(_))));
    let recovered = rotate_handler.execute(rotate, context()).await??;
    assert!(recovered.replayed);
    let active = nodes
        .find_active_certificate(organization_id, node_id)
        .await?;
    assert_eq!(active.id, recovered.certificate.id);

    let current = nodes.find(organization_id, node_id).await?;
    let state_handler = ChangeNodeStateHandler::new(nodes.clone(), certificate_authority);
    let drain = ChangeNodeState {
        organization_id,
        node_id,
        state: NodeState::Draining,
        expected_version: current.aggregate_version,
        idempotency_key: "postgres-fleet-drain".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(4),
    };
    let drained = state_handler.execute(drain.clone(), context()).await??;
    assert_eq!(drained.node.state, NodeState::Draining);
    assert!(state_handler.execute(drain, context()).await??.replayed);
    let revoke = ChangeNodeState {
        organization_id,
        node_id,
        state: NodeState::Revoked,
        expected_version: drained.node.aggregate_version,
        idempotency_key: "postgres-fleet-revoke".into(),
        request_id: Uuid::now_v7(),
        requested_at: now + Duration::seconds(5),
    };
    let revoked = state_handler.execute(revoke.clone(), context()).await??;
    assert_eq!(revoked.node.state, NodeState::Revoked);
    assert!(state_handler.execute(revoke, context()).await??.replayed);
    let revoked_lease = nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id: enrollment_request.agent_instance_id,
                after_sequence: 0,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now + Duration::seconds(30),
            now + Duration::seconds(40),
        )
        .await;
    assert!(matches!(revoked_lease, Err(RepositoryError::NotFound)));

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_certificates where node_id = ",)
                    .bind(node_id.as_uuid()),
            )
            .await?,
        2
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_certificates where node_id = ",)
                    .bind(node_id.as_uuid())
                    .append(" and revoked_at is null"),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ",)
                    .bind(node_id.as_uuid()),
            )
            .await?,
        4
    );
    Ok(())
}

async fn exercise_command_control(
    executor: &PostgresExecutor,
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let aggregate_id = Uuid::now_v7();
    let first = inspect_draft(
        NodeCommandId::new(),
        node_id,
        aggregate_id,
        "postgres-service",
        now + Duration::seconds(6),
    );
    let (left, right) = tokio::join!(
        nodes.enqueue_command(first.clone()),
        nodes.enqueue_command(first)
    );
    let left = left?;
    let right = right?;
    assert_ne!(left.replayed, right.replayed);
    assert_eq!(left.value, right.value);
    assert_eq!(left.value.sequence, 1);

    let second = inspect_draft(
        NodeCommandId::new(),
        node_id,
        aggregate_id,
        "postgres-service",
        now + Duration::seconds(7),
    );
    let third = inspect_draft(
        NodeCommandId::new(),
        node_id,
        aggregate_id,
        "postgres-service",
        now + Duration::seconds(8),
    );
    let (second, third) = tokio::join!(nodes.enqueue_command(second), nodes.enqueue_command(third));
    let mut sequences = [second?.value.sequence, third?.value.sequence];
    sequences.sort_unstable();
    assert_eq!(sequences, [2, 3]);

    let request = NodeCommandLeaseRequest {
        schema: NodeCommandLeaseRequest::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        agent_instance_id,
        after_sequence: 0,
        max_commands: 1,
        wait_ms: 0,
    };
    let first_lease = nodes
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(9),
            now + Duration::seconds(19),
        )
        .await?;
    assert_eq!(first_lease.commands.len(), 1);
    assert_eq!(first_lease.commands[0].sequence, 1);
    assert!(nodes
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(10),
            now + Duration::seconds(20),
        )
        .await?
        .commands
        .is_empty());

    let first_ack = inspected_ack(&first_lease.commands[0], now + Duration::seconds(11));
    let accepted_first_ack = nodes
        .acknowledge_command(first_ack.clone(), now + Duration::seconds(11))
        .await?;
    assert!(!accepted_first_ack.replayed);
    assert_eq!(
        accepted_first_ack.value.completed_at.nanosecond() % 1_000,
        0
    );
    assert!(
        nodes
            .acknowledge_command(first_ack, now + Duration::seconds(12))
            .await?
            .replayed
    );
    assert_eq!(
        nodes
            .command_acknowledgement(
                node_id,
                NodeCommandId::from_uuid(first_lease.commands[0].command_id),
            )
            .await?,
        Some(accepted_first_ack.value)
    );

    let second_lease = nodes
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(12),
            now + Duration::seconds(17),
        )
        .await?;
    assert_eq!(second_lease.commands[0].sequence, 2);
    let replacement = nodes
        .lease_commands(
            &request,
            Uuid::now_v7(),
            now + Duration::seconds(18),
            now + Duration::seconds(28),
        )
        .await?;
    assert_eq!(
        replacement.commands[0].command_id,
        second_lease.commands[0].command_id
    );
    assert_ne!(replacement.lease_id, second_lease.lease_id);
    assert!(nodes
        .acknowledge_command(
            inspected_ack(&second_lease.commands[0], now + Duration::seconds(16)),
            now + Duration::seconds(18),
        )
        .await
        .is_err());
    nodes
        .acknowledge_command(
            inspected_ack(&replacement.commands[0], now + Duration::seconds(20)),
            now + Duration::seconds(20),
        )
        .await?;

    let wrong_agent = NodeCommandLeaseRequest {
        agent_instance_id: Uuid::now_v7(),
        ..request
    };
    assert!(matches!(
        nodes
            .lease_commands(
                &wrong_agent,
                Uuid::now_v7(),
                now + Duration::seconds(21),
                now + Duration::seconds(31),
            )
            .await,
        Err(RepositoryError::NotFound)
    ));

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where node_id = ")
                    .bind(node_id.as_uuid()),
            )
            .await?,
        3
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_commands where node_id = ",)
                    .bind(node_id.as_uuid())
                    .append(" and acknowledgement is not null"),
            )
            .await?,
        2
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<u64>("select last_sequence from nodes where id = ")
                    .bind(node_id.as_uuid())
            )
            .await?,
        3
    );
    Ok(())
}

fn inspect_draft(
    command_id: NodeCommandId,
    node_id: NodeId,
    aggregate_id: Uuid,
    unit_id: &str,
    issued_at: chrono::DateTime<Utc>,
) -> NodeCommandDraft {
    NodeCommandDraft {
        proposed_command_id: command_id,
        node_id,
        aggregate_id,
        payload: NodeCommandPayload::RuntimeInspect {
            unit_id: unit_id.into(),
            generation: 1,
        },
        issued_at,
        not_after: issued_at + Duration::minutes(2),
        correlation_id: Uuid::now_v7(),
    }
}

fn inspected_ack(
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    completed_at: chrono::DateTime<Utc>,
) -> NodeCommandAck {
    let NodeCommandPayload::RuntimeInspect {
        unit_id,
        generation,
    } = &command.payload
    else {
        panic!("test command must inspect Runtime state");
    };
    NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeInspected {
                inspection: a3s_runtime::contract::RuntimeInspection::NotFound {
                    schema: a3s_runtime::contract::RuntimeInspection::SCHEMA.into(),
                    unit_id: unit_id.clone(),
                    last_generation: Some(*generation),
                },
            }),
        },
    }
}

async fn exercise_observation_control(
    executor: &PostgresExecutor,
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed_at = now + Duration::seconds(22);
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
            runtime_capabilities: capabilities(),
        },
        observations: vec![RuntimeObservationReport {
            report_id,
            command_id: None,
            observed_at,
            observation: RuntimeObservation {
                schema: RuntimeObservation::SCHEMA.into(),
                unit_id: "postgres-service".into(),
                generation: 1,
                spec_digest: format!("sha256:{}", "5".repeat(64)),
                class: RuntimeUnitClass::Service,
                state: RuntimeUnitState::Accepted,
                provider_resource_id: None,
                provider_build: None,
                observed_at_ms: 1,
                started_at_ms: None,
                finished_at_ms: None,
                health: None,
                outputs: Vec::new(),
                usage: None,
                evidence: None,
                provider_attestation: None,
                failure: None,
            },
        }],
    };
    let accepted = nodes
        .record_observations(batch.clone(), observed_at)
        .await?;
    assert_eq!(
        (accepted.accepted_reports, accepted.replayed_reports),
        (1, 0)
    );
    let replayed = nodes
        .record_observations(batch.clone(), observed_at + Duration::milliseconds(1))
        .await?;
    assert_eq!(
        (replayed.accepted_reports, replayed.replayed_reports),
        (0, 1)
    );
    let latest = nodes
        .latest_runtime_observation(node_id, "postgres-service", 1)
        .await?
        .ok_or("latest Runtime observation was not found")?;
    assert_eq!(latest.report_id, report_id);
    assert_eq!(latest.observation, batch.observations[0].observation);
    assert!(nodes
        .latest_runtime_observation(node_id, "missing-service", 1)
        .await?
        .is_none());
    let mut conflict = batch;
    conflict.observations[0].observation.unit_id = "different-service".into();
    assert!(matches!(
        nodes
            .record_observations(conflict, observed_at + Duration::milliseconds(2))
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let snapshot =
        a3s_cloud_contracts::GatewaySnapshot::new(1, None, "management { enabled = true }\n")?;
    let gateway_command = nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id,
            aggregate_id: Uuid::now_v7(),
            payload: NodeCommandPayload::GatewaySnapshotInstall {
                snapshot: Box::new(snapshot.clone()),
            },
            issued_at: observed_at,
            not_after: observed_at + Duration::minutes(2),
            correlation_id: Uuid::now_v7(),
        })
        .await?
        .value;
    let gateway = NodeGatewayAck {
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
        !nodes
            .record_gateway_acknowledgement(gateway.clone(), observed_at + Duration::seconds(1),)
            .await?
            .replayed
    );
    assert!(
        nodes
            .record_gateway_acknowledgement(gateway.clone(), observed_at + Duration::seconds(2),)
            .await?
            .replayed
    );
    let mut gateway_conflict = gateway;
    gateway_conflict.state = GatewayAckState::Rejected;
    assert!(matches!(
        nodes
            .record_gateway_acknowledgement(gateway_conflict, observed_at + Duration::seconds(3),)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let log_batch = NodeLogBatchReceiptDraft {
        batch_id: Uuid::now_v7(),
        node_id,
        payload_digest: format!("sha256:{}", "3".repeat(64)),
        sent_at: observed_at + Duration::seconds(4),
        chunks: vec![NodeLogChunkReceiptDraft {
            unit_id: "postgres-service".into(),
            generation: 1,
            cursor: "opaque:1".into(),
            sequence: 1,
            observed_at_ms: 1,
            stream: "stdout".into(),
            checksum: format!("sha256:{}", "2".repeat(64)),
            object_key: format!("nodes/{node_id}/postgres-service/1.json"),
        }],
    };
    assert!(
        !nodes
            .record_log_chunks(log_batch.clone(), observed_at + Duration::seconds(4),)
            .await?
            .replayed
    );
    assert!(
        nodes
            .record_log_chunks(log_batch.clone(), observed_at + Duration::seconds(5),)
            .await?
            .replayed
    );
    let stored_logs = nodes
        .list_log_chunks(NodeLogChunkQuery {
            node_id,
            unit_id: "postgres-service".into(),
            generation: 1,
            after_sequence: 0,
            limit: 2,
            stream: Some(RuntimeLogStream::Stdout),
        })
        .await?;
    assert_eq!(stored_logs.len(), 1);
    assert_eq!(stored_logs[0].sequence, 1);
    assert_eq!(stored_logs[0].object_key, log_batch.chunks[0].object_key);
    assert!(nodes
        .list_log_chunks(NodeLogChunkQuery {
            node_id,
            unit_id: "postgres-service".into(),
            generation: 1,
            after_sequence: 1,
            limit: 2,
            stream: None,
        })
        .await?
        .is_empty());
    let mut log_conflict = log_batch;
    log_conflict.chunks[0].checksum = format!("sha256:{}", "1".repeat(64));
    assert!(matches!(
        nodes
            .record_log_chunks(log_conflict, observed_at + Duration::seconds(6))
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from runtime_observations where node_id = ",)
                    .bind(node_id.as_uuid())
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_log_chunks where node_id = ",)
                    .bind(node_id.as_uuid())
            )
            .await?,
        1
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from node_gateway_acknowledgements where node_id = ",
                )
                .bind(node_id.as_uuid())
            )
            .await?,
        1
    );
    Ok(())
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn csr() -> String {
    let key = KeyPair::generate().expect("node key");
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "postgres-node");
    params.distinguished_name = name;
    params
        .serialize_request(&key)
        .expect("CSR")
        .pem()
        .expect("CSR PEM")
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("docker").expect("valid Docker provider ID"),
        provider_build: "postgres-test".into(),
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
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
        ],
    }
}
