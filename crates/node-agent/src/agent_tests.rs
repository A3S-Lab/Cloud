use super::*;
use crate::GatewaySnapshotInstallOutcome;
use a3s_cloud_contracts::{
    GatewaySnapshot, NodeCertificate, NodeCommandEnvelope, NodeCommandLeaseResponse,
    NodeCommandMetadata, NodeCommandPayload, NodeGatewayAck, NodeGatewayAckReceipt,
    NodeLogChunkBatch, NodeLogChunkReceipt, NodeObservationReceipt,
};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeExecRequest, RuntimeExecResult, RuntimeFeature,
    RuntimeHealthObservation, RuntimeHealthState, RuntimeInspection, RuntimeLogChunk,
    RuntimeLogQuery, RuntimeObservation, RuntimeRemoval, RuntimeUnitClass, RuntimeUnitState,
};
use a3s_runtime::RuntimeResult;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use uuid::Uuid;

struct InspectRuntime {
    capabilities: RuntimeCapabilities,
    observation: RuntimeObservation,
    calls: AtomicUsize,
}

#[async_trait]
impl RuntimeClient for InspectRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Ok(self.capabilities.clone())
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(RuntimeError::Protocol("unexpected apply".into()))
    }

    async fn inspect(&self, _unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RuntimeInspection::Found {
            observation: Box::new(self.observation.clone()),
        })
    }

    async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        Err(RuntimeError::Protocol("unexpected stop".into()))
    }

    async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        Err(RuntimeError::Protocol("unexpected remove".into()))
    }

    async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        Err(RuntimeError::Protocol("unexpected logs".into()))
    }

    async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        Err(RuntimeError::Protocol("unexpected exec".into()))
    }
}

#[derive(Default)]
struct TransportState {
    leases: VecDeque<Vec<NodeCommandEnvelope>>,
    after_sequences: Vec<u64>,
    acknowledgement_conflicts: usize,
    events: Vec<String>,
    reports: BTreeSet<Uuid>,
    gateway_acknowledgements: BTreeSet<Uuid>,
}

#[derive(Default)]
struct FakeTransport {
    state: Mutex<TransportState>,
    identity: Mutex<Option<(Uuid, Uuid)>>,
}

#[async_trait]
impl NodeControlTransport for FakeTransport {
    async fn lease(
        &self,
        after_sequence: u64,
        _max_commands: u16,
        _wait_ms: u64,
    ) -> Result<NodeCommandLeaseResponse, NodeControlClientError> {
        let mut state = self.state.lock().await;
        state.after_sequences.push(after_sequence);
        let commands = state.leases.pop_front().unwrap_or_default();
        let lease_id = commands
            .first()
            .map(|command| command.lease_id)
            .unwrap_or_else(Uuid::now_v7);
        let identity = self
            .identity
            .lock()
            .await
            .as_ref()
            .copied()
            .expect("test transport identity");
        let node_id = commands
            .first()
            .map(|command| command.node_id)
            .unwrap_or(identity.0);
        Ok(NodeCommandLeaseResponse {
            schema: NodeCommandLeaseResponse::SCHEMA.into(),
            lease_id,
            node_id,
            agent_instance_id: identity.1,
            leased_until: Utc::now() + ChronoDuration::minutes(1),
            commands,
        })
    }

    async fn acknowledge(
        &self,
        acknowledgement: &NodeCommandAck,
    ) -> Result<NodeCommandAckReceipt, NodeControlClientError> {
        let mut state = self.state.lock().await;
        state
            .events
            .push(format!("ack:{}", acknowledgement.command_id));
        if state.acknowledgement_conflicts > 0 {
            state.acknowledgement_conflicts -= 1;
            return Err(NodeControlClientError::Rejected {
                status: 409,
                code: "conflict".into(),
                message: "lease must be rebound".into(),
                retryable: false,
            });
        }
        Ok(NodeCommandAckReceipt {
            schema: NodeCommandAckReceipt::SCHEMA.into(),
            command_id: acknowledgement.command_id,
            node_id: acknowledgement.node_id,
            replayed: false,
        })
    }

    async fn record_observations(
        &self,
        batch: &NodeObservationBatch,
    ) -> Result<NodeObservationReceipt, NodeControlClientError> {
        batch.validate().map_err(NodeControlClientError::Invalid)?;
        let mut state = self.state.lock().await;
        let mut accepted = 0_u16;
        let mut replayed = 0_u16;
        for report in &batch.observations {
            state.events.push(format!("report:{}", report.report_id));
            if state.reports.insert(report.report_id) {
                accepted += 1;
            } else {
                replayed += 1;
            }
        }
        if batch.observations.is_empty() {
            state.events.push("heartbeat".into());
        }
        Ok(NodeObservationReceipt {
            schema: NodeObservationReceipt::SCHEMA.into(),
            node_id: batch.node_id,
            heartbeat_observed_at: batch.heartbeat.observed_at,
            accepted_reports: accepted,
            replayed_reports: replayed,
        })
    }

    async fn record_log_chunks(
        &self,
        _batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "unexpected log upload".into(),
        ))
    }

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<NodeGatewayAckReceipt, NodeControlClientError> {
        let mut state = self.state.lock().await;
        state
            .events
            .push(format!("gateway:{}", acknowledgement.command_id));
        let replayed = !state
            .gateway_acknowledgements
            .insert(acknowledgement.acknowledgement_id);
        Ok(NodeGatewayAckReceipt {
            schema: NodeGatewayAckReceipt::SCHEMA.into(),
            acknowledgement_id: acknowledgement.acknowledgement_id,
            command_id: acknowledgement.command_id,
            node_id: acknowledgement.node_id,
            replayed,
        })
    }
}

struct AppliedGatewayInstaller;

#[async_trait]
impl GatewaySnapshotInstaller for AppliedGatewayInstaller {
    async fn install(
        &self,
        _snapshot: &GatewaySnapshot,
    ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
        Ok(GatewaySnapshotInstallOutcome::Applied)
    }
}

fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: "docker".into(),
        provider_build: "docker-test".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
        network_modes: vec![NetworkMode::None, NetworkMode::Service],
        mount_kinds: vec![MountKind::Volume, MountKind::Tmpfs],
        health_check_kinds: vec![HealthCheckKind::Http, HealthCheckKind::Tcp],
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
            RuntimeFeature::Logs,
        ],
    }
}

fn observation() -> RuntimeObservation {
    RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: "service-1".into(),
        generation: 1,
        spec_digest: format!("sha256:{}", "a".repeat(64)),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("container-1".into()),
        provider_build: Some("docker-test".into()),
        observed_at_ms: 1,
        started_at_ms: Some(1),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: RuntimeHealthState::Healthy,
            checked_at_ms: 1,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: None,
        provider_attestation: None,
        failure: None,
    }
}

async fn enrolled_identity(root: &Path, node_id: Uuid) -> EnrolledNodeIdentity {
    let store = FileNodeIdentityStore::new(root);
    let state = store
        .prepare("node-1".into(), "test".into(), capabilities())
        .await
        .expect("prepare identity");
    match state {
        NodeIdentityState::Pending(_) => {}
        NodeIdentityState::Enrolled(_) => panic!("new identity must be pending"),
    }
    let issued_at = Utc::now();
    store
        .complete(a3s_cloud_contracts::NodeEnrollmentResponse {
            schema: a3s_cloud_contracts::NodeEnrollmentResponse::SCHEMA.into(),
            node_id,
            certificate: NodeCertificate {
                certificate_id: Uuid::now_v7(),
                serial_number: "serial-1".into(),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                    .into(),
                issued_at,
                expires_at: issued_at + ChronoDuration::days(1),
            },
            heartbeat_interval_ms: 10,
            command_long_poll_ms: 1,
            certificate_rotation_window_ms: 60 * 60 * 1_000,
        })
        .await
        .expect("complete identity")
}

fn command(node_id: Uuid, command_id: Uuid, lease_id: Uuid) -> NodeCommandEnvelope {
    let issued_at = Utc::now() - ChronoDuration::milliseconds(1);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id,
            lease_id,
            node_id,
            sequence: 1,
            aggregate_id: Uuid::now_v7(),
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::RuntimeInspect {
            unit_id: "service-1".into(),
            generation: 1,
        },
    )
    .expect("command")
}

fn gateway_command(node_id: Uuid, command_id: Uuid, lease_id: Uuid) -> NodeCommandEnvelope {
    let issued_at = Utc::now() - ChronoDuration::milliseconds(1);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id,
            lease_id,
            node_id,
            sequence: 1,
            aggregate_id: Uuid::now_v7(),
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::GatewaySnapshotInstall {
            snapshot: Box::new(
                GatewaySnapshot::new(1, None, "management { enabled = true }\n")
                    .expect("Gateway snapshot"),
            ),
        },
    )
    .expect("Gateway command")
}

async fn session(
    root: &Path,
    transport: Arc<FakeTransport>,
    runtime: Arc<InspectRuntime>,
    node_id: Uuid,
) -> NodeAgentSession {
    let identity = enrolled_identity(root, node_id).await;
    *transport.identity.lock().await = Some((node_id, identity.agent_instance_id));
    NodeAgentSession::new(
        transport,
        runtime,
        Arc::new(AppliedGatewayInstaller),
        identity,
        capabilities(),
        "test".into(),
        root.to_owned(),
        Duration::from_millis(1),
        Duration::from_millis(4),
    )
    .expect("session")
}

async fn restarted_session(
    root: &Path,
    transport: Arc<FakeTransport>,
    runtime: Arc<InspectRuntime>,
) -> NodeAgentSession {
    let identity = match FileNodeIdentityStore::new(root)
        .prepare("node-1".into(), "test".into(), capabilities())
        .await
        .expect("reopen identity")
    {
        NodeIdentityState::Enrolled(identity) => identity,
        NodeIdentityState::Pending(_) => panic!("completed identity regressed after restart"),
    };
    *transport.identity.lock().await =
        Some((identity.response.node_id, identity.agent_instance_id));
    NodeAgentSession::new(
        transport,
        runtime,
        Arc::new(AppliedGatewayInstaller),
        identity,
        capabilities(),
        "test".into(),
        root.to_owned(),
        Duration::from_millis(1),
        Duration::from_millis(4),
    )
    .expect("restarted session")
}

#[tokio::test]
async fn command_observation_precedes_ack_and_only_ack_advances_the_cursor() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let command_id = Uuid::now_v7();
    let lease_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport::default());
    transport
        .state
        .lock()
        .await
        .leases
        .push_back(vec![command(node_id, command_id, lease_id)]);
    let runtime = Arc::new(InspectRuntime {
        capabilities: capabilities(),
        observation: observation(),
        calls: AtomicUsize::new(0),
    });
    let session = session(
        directory.path(),
        transport.clone(),
        runtime.clone(),
        node_id,
    )
    .await;

    session.synchronize_once().await.expect("synchronize");
    session.synchronize_once().await.expect("empty poll");

    let state = transport.state.lock().await;
    assert_eq!(state.after_sequences, vec![0, 1]);
    assert_eq!(
        &state.events[..2],
        &[format!("report:{command_id}"), format!("ack:{command_id}")]
    );
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gateway_acknowledgement_precedes_command_ack_and_is_replay_safe() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let command_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport::default());
    transport
        .state
        .lock()
        .await
        .leases
        .push_back(vec![gateway_command(node_id, command_id, Uuid::now_v7())]);
    let runtime = Arc::new(InspectRuntime {
        capabilities: capabilities(),
        observation: observation(),
        calls: AtomicUsize::new(0),
    });
    let session = session(directory.path(), transport.clone(), runtime, node_id).await;

    session.synchronize_once().await.expect("synchronize");

    let state = transport.state.lock().await;
    assert_eq!(
        &state.events[..2],
        &[format!("gateway:{command_id}"), format!("ack:{command_id}")]
    );
    assert_eq!(state.gateway_acknowledgements.len(), 1);
}

#[tokio::test]
async fn expired_lease_redelivery_reuses_the_durable_runtime_result() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let command_id = Uuid::now_v7();
    let first = command(node_id, command_id, Uuid::now_v7());
    let mut redelivered = first.clone();
    redelivered.lease_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport::default());
    {
        let mut state = transport.state.lock().await;
        state.acknowledgement_conflicts = 2;
        state.leases.push_back(vec![first]);
        state.leases.push_back(vec![redelivered]);
    }
    let runtime = Arc::new(InspectRuntime {
        capabilities: capabilities(),
        observation: observation(),
        calls: AtomicUsize::new(0),
    });
    let session = session(
        directory.path(),
        transport.clone(),
        runtime.clone(),
        node_id,
    )
    .await;

    session.synchronize_once().await.expect("first lease");
    drop(session);
    let restarted = restarted_session(directory.path(), transport.clone(), runtime.clone()).await;
    restarted
        .synchronize_once()
        .await
        .expect("redelivered lease after restart");
    restarted
        .synchronize_once()
        .await
        .expect("advanced poll after restart");

    let state = transport.state.lock().await;
    assert_eq!(state.after_sequences, vec![0, 0, 1]);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert!(
        state
            .events
            .iter()
            .filter(|event| *event == &format!("report:{command_id}"))
            .count()
            >= 3
    );
}

#[tokio::test]
async fn heartbeat_contains_no_synthetic_runtime_observation() {
    let directory = tempfile::tempdir().expect("state directory");
    let node_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport::default());
    let runtime = Arc::new(InspectRuntime {
        capabilities: capabilities(),
        observation: observation(),
        calls: AtomicUsize::new(0),
    });
    let session = session(directory.path(), transport.clone(), runtime, node_id).await;

    session.heartbeat_once().await.expect("heartbeat");

    assert_eq!(transport.state.lock().await.events, vec!["heartbeat"]);
}

#[tokio::test]
async fn process_lock_rejects_a_second_agent_for_the_same_state_directory() {
    let directory = tempfile::tempdir().expect("state directory");
    let first = acquire_process_lock(directory.path())
        .await
        .expect("first process lock");
    let second = acquire_process_lock(directory.path()).await;
    assert!(matches!(second, Err(NodeAgentError::State(_))));
    drop(first);
    acquire_process_lock(directory.path())
        .await
        .expect("released process lock");
}
