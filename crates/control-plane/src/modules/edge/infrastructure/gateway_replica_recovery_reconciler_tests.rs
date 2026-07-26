use super::gateway_replica_recovery_reconciler::{
    deterministic_recovery_observation_command_id, GatewayReplicaRecoveryReconciler,
};
use crate::modules::edge::domain::events::{GatewayRolloutStaged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
};
use crate::modules::edge::domain::services::{
    GatewayObservationCommand, GatewayObservationCommandOutcome, GatewayObservationDispatch,
    IGatewayObservationQueue,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayReplicaRecoveryState, GatewayRollout, GatewayScope,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{
    GatewayManagementProtocol, GatewaySnapshot, GatewaySnapshotObservationState,
    NodeGatewaySnapshotObservation,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Barrier, Mutex};
use uuid::Uuid;

struct OutcomeSynchronization {
    barrier: Arc<Barrier>,
    remaining: usize,
}

#[derive(Default)]
struct RecordingObservationState {
    enqueue_failures: usize,
    commands: BTreeMap<NodeCommandId, GatewayObservationCommand>,
    outcomes: BTreeMap<NodeCommandId, GatewayObservationCommandOutcome>,
    outcome_synchronization: Option<OutcomeSynchronization>,
}

#[derive(Default)]
struct RecordingObservationQueue {
    state: Mutex<RecordingObservationState>,
}

impl RecordingObservationQueue {
    async fn fail_next_enqueue(&self) {
        self.state.lock().await.enqueue_failures += 1;
    }

    async fn record_outcome(
        &self,
        command_id: NodeCommandId,
        outcome: GatewayObservationCommandOutcome,
    ) {
        self.state.lock().await.outcomes.insert(command_id, outcome);
    }

    async fn command(&self, command_id: NodeCommandId) -> Option<GatewayObservationCommand> {
        self.state.lock().await.commands.get(&command_id).cloned()
    }

    async fn command_count(&self) -> usize {
        self.state.lock().await.commands.len()
    }

    async fn synchronize_next_outcomes(&self, participants: usize) {
        assert!(participants > 1);
        self.state.lock().await.outcome_synchronization = Some(OutcomeSynchronization {
            barrier: Arc::new(Barrier::new(participants)),
            remaining: participants,
        });
    }
}

#[async_trait]
impl IGatewayObservationQueue for RecordingObservationQueue {
    async fn enqueue(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<GatewayObservationDispatch, RepositoryError> {
        command.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.lock().await;
        if state.enqueue_failures > 0 {
            state.enqueue_failures -= 1;
            return Err(RepositoryError::Storage(
                "injected Gateway observation queue failure".into(),
            ));
        }
        if let Some(existing) = state.commands.get(&command.command_id) {
            if existing != command {
                return Err(RepositoryError::Conflict(
                    "Gateway observation command identity was reused".into(),
                ));
            }
            return Ok(GatewayObservationDispatch { replayed: true });
        }
        state.commands.insert(command.command_id, command.clone());
        Ok(GatewayObservationDispatch { replayed: false })
    }

    async fn outcome(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<Option<GatewayObservationCommandOutcome>, RepositoryError> {
        let (outcome, barrier) = {
            let mut state = self.state.lock().await;
            let outcome = state.outcomes.get(&command.command_id).cloned();
            let barrier = state
                .outcome_synchronization
                .take()
                .map(|mut synchronization| {
                    let barrier = Arc::clone(&synchronization.barrier);
                    synchronization.remaining -= 1;
                    if synchronization.remaining > 0 {
                        state.outcome_synchronization = Some(synchronization);
                    }
                    barrier
                });
            (outcome, barrier)
        };
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
        Ok(outcome)
    }
}

#[tokio::test]
async fn recovery_reconciler_restages_before_dispatch_and_recovers_every_crash_gap() {
    let fixture = unavailable_fixture().await;
    let queue = Arc::new(RecordingObservationQueue::default());
    queue.fail_next_enqueue().await;
    let repository_port: Arc<dyn IEdgeRepository> = fixture.repository.clone();
    let queue_port: Arc<dyn IGatewayObservationQueue> = queue.clone();
    let first = GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&repository_port),
        Arc::clone(&queue_port),
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        10,
    )
    .expect("recovery reconciler");
    let first_at = fixture.unavailable_at + Duration::seconds(1);
    let first_report = first.run_once(first_at).await.expect("first recovery pass");
    assert_eq!(first_report.recovery_targets, 1);
    assert_eq!(first_report.staged_attempts, 1);
    assert_eq!(first_report.dispatched_commands, 0);
    assert_eq!(first_report.failures.len(), 1);

    let first_command =
        deterministic_recovery_observation_command_id(fixture.rollout_id, fixture.node_id, 1);
    let observing = fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, fixture.rollout_id)
        .await
        .expect("observing rollout");
    assert_eq!(
        recovery_state(&observing, fixture.node_id),
        GatewayReplicaRecoveryState::Observing
    );
    assert_eq!(recovery_attempt(&observing, fixture.node_id), 1);
    assert_eq!(
        recovery_command_id(&observing, fixture.node_id),
        first_command
    );
    assert!(queue.command(first_command).await.is_none());

    let restarted = GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&repository_port),
        Arc::clone(&queue_port),
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        10,
    )
    .expect("restarted recovery reconciler");
    let dispatch_report = restarted
        .run_once(first_at + Duration::seconds(1))
        .await
        .expect("recovery redispatch");
    assert_eq!(dispatch_report.staged_attempts, 0);
    assert_eq!(dispatch_report.dispatched_commands, 1);
    assert_eq!(dispatch_report.replayed_commands, 0);
    assert_eq!(dispatch_report.pending_commands, 1);
    let dispatched = queue
        .command(first_command)
        .await
        .expect("durably staged command was dispatched");
    assert_eq!(dispatched.attempt, 1);
    assert_eq!(dispatched.rollout_id, fixture.rollout_id);
    assert_eq!(dispatched.node_id, fixture.node_id);
    assert_eq!(dispatched.candidate_revision, fixture.publication.revision);
    assert_eq!(
        dispatched.candidate_snapshot_digest,
        fixture.publication.snapshot_digest
    );

    let replay_report = restarted
        .run_once(first_at + Duration::seconds(2))
        .await
        .expect("idempotent command replay");
    assert_eq!(replay_report.dispatched_commands, 1);
    assert_eq!(replay_report.replayed_commands, 1);
    assert_eq!(replay_report.pending_commands, 1);

    queue
        .record_outcome(
            first_command,
            GatewayObservationCommandOutcome::Observed {
                observation: Box::new(observation(
                    first_command,
                    &fixture.publication,
                    GatewaySnapshotObservationState::Applying,
                    first_at + Duration::seconds(3),
                )),
                completed_at: first_at + Duration::seconds(3),
            },
        )
        .await;
    let applying_report = restarted
        .run_once(first_at + Duration::seconds(4))
        .await
        .expect("project Applying observation");
    assert_eq!(applying_report.retryable_outcomes, 1);
    assert_eq!(applying_report.dispatched_commands, 0);
    let required = fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, fixture.rollout_id)
        .await
        .expect("required retry");
    assert_eq!(
        recovery_state(&required, fixture.node_id),
        GatewayReplicaRecoveryState::Required
    );
    assert_eq!(recovery_attempt(&required, fixture.node_id), 1);

    let second_at = first_at + Duration::seconds(5);
    let second_report = restarted
        .run_once(second_at)
        .await
        .expect("stage second observation");
    assert_eq!(second_report.staged_attempts, 1);
    assert_eq!(second_report.dispatched_commands, 1);
    assert_eq!(second_report.pending_commands, 1);
    let second_command =
        deterministic_recovery_observation_command_id(fixture.rollout_id, fixture.node_id, 2);
    assert_ne!(second_command, first_command);
    assert_eq!(
        queue
            .command(second_command)
            .await
            .expect("second observation command")
            .attempt,
        2
    );

    queue
        .record_outcome(
            second_command,
            GatewayObservationCommandOutcome::Observed {
                observation: Box::new(observation(
                    second_command,
                    &fixture.publication,
                    GatewaySnapshotObservationState::Uninitialized,
                    second_at + Duration::seconds(1),
                )),
                completed_at: second_at + Duration::seconds(1),
            },
        )
        .await;
    let completed_report = restarted
        .run_once(second_at + Duration::seconds(2))
        .await
        .expect("complete physical observation");
    assert_eq!(completed_report.observed_replicas, 1);
    assert_eq!(completed_report.dispatched_commands, 0);
    assert!(completed_report.failures.is_empty());
    let observed = fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, fixture.rollout_id)
        .await
        .expect("observed recovery");
    assert_eq!(
        recovery_state(&observed, fixture.node_id),
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(recovery_attempt(&observed, fixture.node_id), 2);
    assert!(fixture
        .repository
        .pending_gateway_replica_recoveries(10)
        .await
        .expect("terminal recovery scan")
        .is_empty());
}

#[tokio::test]
async fn recovery_reconciler_expires_missing_ack_and_terminalizes_protocol_failure() {
    let fixture = unavailable_fixture().await;
    let queue = Arc::new(RecordingObservationQueue::default());
    let repository_port: Arc<dyn IEdgeRepository> = fixture.repository.clone();
    let queue_port: Arc<dyn IGatewayObservationQueue> = queue.clone();
    let reconciler = GatewayReplicaRecoveryReconciler::new(
        repository_port,
        queue_port,
        std::time::Duration::from_secs(1),
        Duration::seconds(10),
        10,
    )
    .expect("recovery reconciler");
    let first_at = fixture.unavailable_at + Duration::seconds(1);
    let staged = reconciler
        .run_once(first_at)
        .await
        .expect("stage expiring observation");
    assert_eq!(staged.staged_attempts, 1);
    assert_eq!(staged.pending_commands, 1);
    let first_command =
        deterministic_recovery_observation_command_id(fixture.rollout_id, fixture.node_id, 1);

    let expired = reconciler
        .run_once(first_at + Duration::seconds(10))
        .await
        .expect("expire missing acknowledgement");
    assert_eq!(expired.retryable_outcomes, 1);
    assert_eq!(expired.dispatched_commands, 0);
    let required = fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, fixture.rollout_id)
        .await
        .expect("required after expiry");
    assert_eq!(
        recovery_state(&required, fixture.node_id),
        GatewayReplicaRecoveryState::Required
    );
    assert_eq!(
        recovery_command_id(&required, fixture.node_id),
        first_command
    );

    let second_at = first_at + Duration::seconds(11);
    reconciler
        .run_once(second_at)
        .await
        .expect("stage replacement attempt");
    let second_command =
        deterministic_recovery_observation_command_id(fixture.rollout_id, fixture.node_id, 2);
    queue
        .record_outcome(
            second_command,
            GatewayObservationCommandOutcome::Failed {
                failure: "Gateway observation command rejected with code unsupported_protocol"
                    .into(),
                retryable: false,
                completed_at: second_at + Duration::seconds(1),
            },
        )
        .await;
    let terminal = reconciler
        .run_once(second_at + Duration::seconds(2))
        .await
        .expect("project terminal protocol failure");
    assert_eq!(terminal.diverged_replicas, 1);
    assert_eq!(terminal.dispatched_commands, 0);
    let diverged = fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, fixture.rollout_id)
        .await
        .expect("diverged recovery");
    assert_eq!(
        recovery_state(&diverged, fixture.node_id),
        GatewayReplicaRecoveryState::Diverged
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_recovery_workers_project_one_exact_outcome_without_false_failure() {
    let fixture = unavailable_fixture().await;
    let queue = Arc::new(RecordingObservationQueue::default());
    let repository_port: Arc<dyn IEdgeRepository> = fixture.repository.clone();
    let queue_port: Arc<dyn IGatewayObservationQueue> = queue.clone();
    let staged_by = GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&repository_port),
        Arc::clone(&queue_port),
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        10,
    )
    .expect("recovery reconciler");
    let staged_at = fixture.unavailable_at + Duration::seconds(1);
    let staged = staged_by
        .run_once(staged_at)
        .await
        .expect("stage observation");
    assert_eq!(staged.staged_attempts, 1);
    assert_eq!(queue.command_count().await, 1);
    let command_id =
        deterministic_recovery_observation_command_id(fixture.rollout_id, fixture.node_id, 1);
    queue
        .record_outcome(
            command_id,
            GatewayObservationCommandOutcome::Observed {
                observation: Box::new(observation(
                    command_id,
                    &fixture.publication,
                    GatewaySnapshotObservationState::Uninitialized,
                    staged_at + Duration::seconds(1),
                )),
                completed_at: staged_at + Duration::seconds(1),
            },
        )
        .await;
    queue.synchronize_next_outcomes(2).await;

    let first = GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&repository_port),
        Arc::clone(&queue_port),
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        10,
    )
    .expect("first concurrent recovery reconciler");
    let second = GatewayReplicaRecoveryReconciler::new(
        repository_port,
        queue_port,
        std::time::Duration::from_secs(1),
        Duration::minutes(1),
        10,
    )
    .expect("second concurrent recovery reconciler");
    let reconciled_at = staged_at + Duration::seconds(2);
    let (first_report, second_report) = tokio::join!(
        first.run_once(reconciled_at),
        second.run_once(reconciled_at)
    );
    let first_report = first_report.expect("first concurrent projection");
    let second_report = second_report.expect("second concurrent projection");
    assert!(first_report.failures.is_empty());
    assert!(second_report.failures.is_empty());
    assert_eq!(
        first_report.observed_replicas + second_report.observed_replicas,
        1
    );
    assert_eq!(
        first_report.replayed_outcomes + second_report.replayed_outcomes,
        1
    );
    assert_eq!(
        first_report.superseded_outcomes + second_report.superseded_outcomes,
        0
    );
    assert_eq!(queue.command_count().await, 1);
    let recovered = fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, fixture.rollout_id)
        .await
        .expect("concurrently recovered rollout");
    assert_eq!(
        recovery_state(&recovered, fixture.node_id),
        GatewayReplicaRecoveryState::Observed
    );
    assert_eq!(recovery_attempt(&recovered, fixture.node_id), 1);
}

#[test]
fn recovery_reconciler_configuration_and_command_identity_are_closed() {
    let repository: Arc<dyn IEdgeRepository> = Arc::new(InMemoryEdgeRepository::new());
    let queue: Arc<dyn IGatewayObservationQueue> = Arc::new(RecordingObservationQueue::default());
    assert!(GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&repository),
        Arc::clone(&queue),
        std::time::Duration::ZERO,
        Duration::seconds(1),
        1,
    )
    .is_err());
    assert!(GatewayReplicaRecoveryReconciler::new(
        Arc::clone(&repository),
        Arc::clone(&queue),
        std::time::Duration::from_secs(1),
        Duration::zero(),
        1,
    )
    .is_err());
    assert!(GatewayReplicaRecoveryReconciler::new(
        repository,
        queue,
        std::time::Duration::from_secs(1),
        Duration::seconds(1),
        0,
    )
    .is_err());
    let rollout_id = GatewayRolloutId::new();
    let node_id = NodeId::new();
    assert_eq!(
        deterministic_recovery_observation_command_id(rollout_id, node_id, 1),
        deterministic_recovery_observation_command_id(rollout_id, node_id, 1)
    );
    assert_ne!(
        deterministic_recovery_observation_command_id(rollout_id, node_id, 1),
        deterministic_recovery_observation_command_id(rollout_id, node_id, 2)
    );
}

struct UnavailableFixture {
    repository: Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    publication: GatewayPublication,
    unavailable_at: DateTime<Utc>,
}

async fn unavailable_fixture() -> UnavailableFixture {
    let repository = Arc::new(InMemoryEdgeRepository::new());
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        now,
    )
    .expect("Gateway scope");
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "gateway-recovery-reconciler-scopes",
                scope.id.to_string(),
                node_id.to_string().as_bytes(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("persist Gateway scope");
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::hours(1),
        "# Gateway recovery reconciler snapshot",
    )
    .expect("Gateway snapshot");
    let publication = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        Uuid::now_v7(),
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("Gateway publication");
    let rollout = GatewayRollout::stage(
        GatewayRolloutId::new(),
        &scope,
        1,
        std::slice::from_ref(&publication),
        now,
    )
    .expect("Gateway rollout");
    repository
        .stage_gateway_rollout(StageGatewayRollout {
            scope: scope.clone(),
            rollout: rollout.clone(),
            route_replicas: Vec::new(),
            publications: vec![publication.clone()],
            certificates: Vec::new(),
            expected_scope_versions: BTreeMap::from([(node_id, 0)]),
            idempotency: IdempotencyRequest::new(
                format!("gateway-scopes/{}/rollouts", scope.id),
                "gateway-recovery-reconciler",
                rollout.id.to_string().as_bytes(),
            )
            .expect("rollout idempotency"),
            event: GatewayRolloutStaged::envelope(&scope, &rollout).expect("rollout event"),
            route_event: None,
        })
        .await
        .expect("stage rollout");
    let unavailable_at = publication.command_not_after + Duration::seconds(1);
    repository
        .mark_gateway_rollout_replica_unavailable(
            organization_id,
            rollout.id,
            node_id,
            rollout.aggregate_version,
            "Gateway command expired before exact acknowledgement",
            unavailable_at,
        )
        .await
        .expect("mark unavailable");
    UnavailableFixture {
        repository,
        organization_id,
        rollout_id: rollout.id,
        node_id,
        publication,
        unavailable_at,
    }
}

fn observation(
    command_id: NodeCommandId,
    publication: &GatewayPublication,
    state: GatewaySnapshotObservationState,
    observed_at: DateTime<Utc>,
) -> NodeGatewaySnapshotObservation {
    NodeGatewaySnapshotObservation {
        schema: NodeGatewaySnapshotObservation::SCHEMA.into(),
        observation_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        state,
        ready: false,
        applied: None,
        observed_at,
        management_protocol: GatewayManagementProtocol::advertised_v1(),
    }
}

fn recovery_state(rollout: &GatewayRollout, node_id: NodeId) -> GatewayReplicaRecoveryState {
    recovery(rollout, node_id).state
}

fn recovery_attempt(rollout: &GatewayRollout, node_id: NodeId) -> u32 {
    recovery(rollout, node_id).attempt
}

fn recovery_command_id(rollout: &GatewayRollout, node_id: NodeId) -> NodeCommandId {
    recovery(rollout, node_id)
        .command_id
        .expect("recovery command ID")
}

fn recovery(
    rollout: &GatewayRollout,
    node_id: NodeId,
) -> &crate::modules::edge::domain::GatewayReplicaRecovery {
    rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .and_then(|replica| replica.recovery.as_ref())
        .expect("Gateway replica recovery")
}
