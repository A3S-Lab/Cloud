use crate::modules::edge::domain::repositories::{GatewayReplicaRecoveryTarget, IEdgeRepository};
use crate::modules::edge::domain::services::{
    GatewayObservationCommand, GatewayObservationCommandOutcome, IGatewayObservationQueue,
};
use crate::modules::edge::domain::{GatewayReplicaRecoveryState, GatewayRollout};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayRolloutId, NodeCommandId, NodeId, RepositoryError,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const EXPIRED_OBSERVATION_FAILURE: &str =
    "Gateway observation command expired before acknowledgement";
const MAX_OPTIMISTIC_STAGE_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayReplicaRecoveryReconciliationFailure {
    pub rollout_id: GatewayRolloutId,
    pub node_id: NodeId,
    pub operation: &'static str,
    pub error: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayReplicaRecoveryReconciliationReport {
    pub recovery_targets: usize,
    pub staged_attempts: usize,
    pub dispatched_commands: usize,
    pub replayed_commands: usize,
    pub pending_commands: usize,
    pub retryable_outcomes: usize,
    pub observed_replicas: usize,
    pub diverged_replicas: usize,
    pub replayed_outcomes: usize,
    pub superseded_outcomes: usize,
    pub failures: Vec<GatewayReplicaRecoveryReconciliationFailure>,
}

pub struct GatewayReplicaRecoveryReconciler {
    repository: Arc<dyn IEdgeRepository>,
    observations: Arc<dyn IGatewayObservationQueue>,
    interval: Duration,
    command_ttl: ChronoDuration,
    batch_size: usize,
}

impl GatewayReplicaRecoveryReconciler {
    pub fn new(
        repository: Arc<dyn IEdgeRepository>,
        observations: Arc<dyn IGatewayObservationQueue>,
        interval: Duration,
        command_ttl: ChronoDuration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero()
            || command_ttl <= ChronoDuration::zero()
            || command_ttl > ChronoDuration::hours(24)
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err(
                "Gateway replica recovery requires positive bounded timing and batch size".into(),
            );
        }
        Ok(Self {
            repository,
            observations,
            interval,
            command_ttl,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<GatewayReplicaRecoveryReconciliationReport, RepositoryError> {
        let now = canonical_timestamp(now);
        let targets = self
            .repository
            .pending_gateway_replica_recoveries(self.batch_size)
            .await?;
        let mut report = GatewayReplicaRecoveryReconciliationReport {
            recovery_targets: targets.len(),
            ..GatewayReplicaRecoveryReconciliationReport::default()
        };
        for mut target in targets {
            let current = match self
                .repository
                .find_gateway_rollout(target.organization_id, target.rollout.id)
                .await
            {
                Ok(rollout) => rollout,
                Err(_) => {
                    report.failures.push(failure(
                        target.rollout.id,
                        target.publication.node_id,
                        "restore",
                        "Gateway replica recovery aggregate restoration failed",
                    ));
                    continue;
                }
            };
            if !recovery_is_pending(&current, target.publication.node_id) {
                continue;
            }
            target.rollout = current;
            if target.validate().is_err() {
                report.failures.push(failure(
                    target.rollout.id,
                    target.publication.node_id,
                    "validate",
                    "Gateway replica recovery target validation failed",
                ));
                continue;
            }
            self.reconcile_target(target, now, &mut report).await;
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    gateway_rollout_id = %failure.rollout_id,
                                    gateway_node_id = %failure.node_id,
                                    operation = failure.operation,
                                    error = failure.error,
                                    "Gateway replica recovery reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Gateway replica recovery scan failed"
                        ),
                    }
                }
            }
        }
    }

    async fn reconcile_target(
        &self,
        target: GatewayReplicaRecoveryTarget,
        now: DateTime<Utc>,
        report: &mut GatewayReplicaRecoveryReconciliationReport,
    ) {
        let rollout_id = target.rollout.id;
        let node_id = target.publication.node_id;
        let organization_id = target.organization_id;
        let (rollout, command, staged_attempt) = match self.prepare_command(target, now).await {
            Ok(prepared) => prepared,
            Err(error) => {
                report
                    .failures
                    .push(failure(rollout_id, node_id, error.operation, error.error));
                return;
            }
        };
        report.staged_attempts += usize::from(staged_attempt);
        let existing_outcome = match self.observations.outcome(&command).await {
            Ok(outcome) => outcome,
            Err(_) => {
                report.failures.push(failure(
                    rollout.id,
                    node_id,
                    "poll",
                    "Gateway observation command outcome lookup failed",
                ));
                return;
            }
        };
        if let Some(outcome) = existing_outcome {
            self.project_outcome(organization_id, rollout, command, outcome, report)
                .await;
            return;
        }
        if command.not_after <= now {
            match self
                .repository
                .record_gateway_replica_recovery_failure(
                    organization_id,
                    rollout.id,
                    node_id,
                    rollout.aggregate_version,
                    command.command_id,
                    EXPIRED_OBSERVATION_FAILURE,
                    true,
                    now,
                )
                .await
            {
                Ok(_) => report.retryable_outcomes += 1,
                Err(_) => report.failures.push(failure(
                    rollout.id,
                    node_id,
                    "expire",
                    "Gateway observation expiry projection failed",
                )),
            }
            return;
        }
        match self.observations.enqueue(&command).await {
            Ok(dispatch) => {
                report.dispatched_commands += 1;
                report.replayed_commands += usize::from(dispatch.replayed);
            }
            Err(_) => {
                report.failures.push(failure(
                    rollout.id,
                    node_id,
                    "dispatch",
                    "Gateway observation command dispatch failed",
                ));
                return;
            }
        }
        let outcome = match self.observations.outcome(&command).await {
            Ok(outcome) => outcome,
            Err(_) => {
                report.failures.push(failure(
                    rollout.id,
                    node_id,
                    "poll",
                    "Gateway observation command outcome lookup failed",
                ));
                return;
            }
        };
        let Some(outcome) = outcome else {
            report.pending_commands += 1;
            return;
        };
        self.project_outcome(organization_id, rollout, command, outcome, report)
            .await;
    }

    async fn project_outcome(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        rollout: GatewayRollout,
        command: GatewayObservationCommand,
        outcome: GatewayObservationCommandOutcome,
        report: &mut GatewayReplicaRecoveryReconciliationReport,
    ) {
        let node_id = command.node_id;
        let transition = match &outcome {
            GatewayObservationCommandOutcome::Observed { observation, .. } => {
                self.repository
                    .record_gateway_replica_recovery_observation(
                        organization_id,
                        rollout.id,
                        node_id,
                        rollout.aggregate_version,
                        observation.as_ref().clone(),
                    )
                    .await
            }
            GatewayObservationCommandOutcome::Failed {
                failure,
                retryable,
                completed_at,
            } => {
                self.repository
                    .record_gateway_replica_recovery_failure(
                        organization_id,
                        rollout.id,
                        node_id,
                        rollout.aggregate_version,
                        command.command_id,
                        failure,
                        *retryable,
                        *completed_at,
                    )
                    .await
            }
        };
        match transition {
            Ok(rollout) => record_projected_state(&rollout, node_id, report),
            Err(RepositoryError::Conflict(_)) => {
                match self
                    .repository
                    .find_gateway_rollout(organization_id, rollout.id)
                    .await
                {
                    Ok(current) if outcome_is_projected(&current, &command, &outcome) => {
                        report.replayed_outcomes += 1;
                    }
                    Ok(current) if outcome_is_superseded(&current, &command) => {
                        report.superseded_outcomes += 1;
                    }
                    _ => report.failures.push(failure(
                        rollout.id,
                        node_id,
                        "project",
                        "Gateway observation outcome projection conflicted with different state",
                    )),
                }
            }
            Err(_) => report.failures.push(failure(
                rollout.id,
                node_id,
                "project",
                "Gateway observation outcome projection failed",
            )),
        }
    }

    async fn prepare_command(
        &self,
        mut target: GatewayReplicaRecoveryTarget,
        now: DateTime<Utc>,
    ) -> Result<(GatewayRollout, GatewayObservationCommand, bool), GatewayRecoveryPreparationFailure>
    {
        let node_id = target.publication.node_id;
        let mut staged_attempt = false;
        for _ in 0..MAX_OPTIMISTIC_STAGE_ATTEMPTS {
            let current_recovery = recovery(&target.rollout, node_id).ok_or(
                GatewayRecoveryPreparationFailure::new(
                    "validate",
                    "Gateway replica recovery state disappeared",
                ),
            )?;
            if current_recovery.state != GatewayReplicaRecoveryState::Required {
                break;
            }
            if now < current_recovery.updated_at {
                return Err(GatewayRecoveryPreparationFailure::new(
                    "validate",
                    "Gateway replica recovery time predates its durable state",
                ));
            }
            let attempt = current_recovery.attempt.checked_add(1).ok_or(
                GatewayRecoveryPreparationFailure::new(
                    "stage",
                    "Gateway replica recovery attempt space is exhausted",
                ),
            )?;
            let not_after = now.checked_add_signed(self.command_ttl).ok_or(
                GatewayRecoveryPreparationFailure::new(
                    "stage",
                    "Gateway observation command expiry exceeds supported time",
                ),
            )?;
            let command_id =
                deterministic_recovery_observation_command_id(target.rollout.id, node_id, attempt);
            match self
                .repository
                .stage_gateway_replica_recovery_observation(
                    target.organization_id,
                    target.rollout.id,
                    node_id,
                    target.rollout.aggregate_version,
                    command_id,
                    now,
                    not_after,
                )
                .await
            {
                Ok(rollout) => {
                    target.rollout = rollout;
                    staged_attempt = true;
                    break;
                }
                Err(RepositoryError::Conflict(_)) => {
                    target.rollout = self
                        .repository
                        .find_gateway_rollout(target.organization_id, target.rollout.id)
                        .await
                        .map_err(|_| {
                            GatewayRecoveryPreparationFailure::new(
                                "restore",
                                "Gateway observation concurrent stage restoration failed",
                            )
                        })?;
                }
                Err(_) => {
                    return Err(GatewayRecoveryPreparationFailure::new(
                        "stage",
                        "Gateway observation attempt staging failed",
                    ))
                }
            }
        }
        if recovery_state(&target.rollout, node_id) == Some(GatewayReplicaRecoveryState::Required) {
            return Err(GatewayRecoveryPreparationFailure::new(
                "stage",
                "Gateway observation attempt remained contended",
            ));
        }
        let recovery =
            recovery(&target.rollout, node_id).ok_or(GatewayRecoveryPreparationFailure::new(
                "restore",
                "Gateway observation attempt disappeared",
            ))?;
        if recovery.state != GatewayReplicaRecoveryState::Observing {
            return Err(GatewayRecoveryPreparationFailure::new(
                "restore",
                "Gateway observation attempt is not active",
            ));
        }
        let command_id = recovery
            .command_id
            .ok_or(GatewayRecoveryPreparationFailure::new(
                "restore",
                "Gateway observation command identity disappeared",
            ))?;
        let issued_at =
            recovery
                .command_issued_at
                .ok_or(GatewayRecoveryPreparationFailure::new(
                    "restore",
                    "Gateway observation command issue time disappeared",
                ))?;
        let not_after =
            recovery
                .command_not_after
                .ok_or(GatewayRecoveryPreparationFailure::new(
                    "restore",
                    "Gateway observation command expiry disappeared",
                ))?;
        if command_id
            != deterministic_recovery_observation_command_id(
                target.rollout.id,
                node_id,
                recovery.attempt,
            )
        {
            return Err(GatewayRecoveryPreparationFailure::new(
                "restore",
                "Gateway observation command identity is not deterministic",
            ));
        }
        let command = GatewayObservationCommand::new(
            target.rollout.id,
            target.rollout.correlation_id,
            node_id,
            target.publication.revision,
            target.publication.snapshot_digest.clone(),
            command_id,
            recovery.attempt,
            issued_at,
            not_after,
        )
        .map_err(|_| {
            GatewayRecoveryPreparationFailure::new(
                "restore",
                "Gateway observation command restoration failed",
            )
        })?;
        Ok((target.rollout, command, staged_attempt))
    }
}

pub(super) fn deterministic_recovery_observation_command_id(
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &rollout_id.as_uuid(),
        format!("a3s-cloud:gateway-replica-recovery-observation:v1:{node_id}:{attempt}").as_bytes(),
    ))
}

fn recovery_is_pending(rollout: &GatewayRollout, node_id: NodeId) -> bool {
    matches!(
        recovery_state(rollout, node_id),
        Some(GatewayReplicaRecoveryState::Required | GatewayReplicaRecoveryState::Observing)
    )
}

fn recovery(
    rollout: &GatewayRollout,
    node_id: NodeId,
) -> Option<&crate::modules::edge::domain::GatewayReplicaRecovery> {
    rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .and_then(|replica| replica.recovery.as_ref())
}

fn recovery_state(
    rollout: &GatewayRollout,
    node_id: NodeId,
) -> Option<GatewayReplicaRecoveryState> {
    recovery(rollout, node_id).map(|recovery| recovery.state)
}

fn record_projected_state(
    rollout: &GatewayRollout,
    node_id: NodeId,
    report: &mut GatewayReplicaRecoveryReconciliationReport,
) {
    match recovery_state(rollout, node_id) {
        Some(GatewayReplicaRecoveryState::Required) => {
            report.retryable_outcomes += 1;
        }
        Some(GatewayReplicaRecoveryState::Observed) => {
            report.observed_replicas += 1;
        }
        Some(GatewayReplicaRecoveryState::Diverged) => {
            report.diverged_replicas += 1;
        }
        Some(GatewayReplicaRecoveryState::Observing) | None => {
            report.failures.push(failure(
                rollout.id,
                node_id,
                "project",
                "Gateway observation outcome left an invalid recovery state",
            ));
        }
    }
}

fn outcome_is_projected(
    rollout: &GatewayRollout,
    command: &GatewayObservationCommand,
    outcome: &GatewayObservationCommandOutcome,
) -> bool {
    let Some(recovery) = recovery(rollout, command.node_id) else {
        return false;
    };
    if recovery.command_id != Some(command.command_id) || recovery.attempt != command.attempt {
        return false;
    }
    match outcome {
        GatewayObservationCommandOutcome::Observed { observation, .. } => {
            let observation = canonicalize_observation(observation.as_ref().clone());
            recovery.observation.as_ref() == Some(&observation)
                && recovery.updated_at == observation.observed_at
                && recovery.state != GatewayReplicaRecoveryState::Observing
        }
        GatewayObservationCommandOutcome::Failed {
            failure,
            retryable,
            completed_at,
        } => {
            recovery.observation.is_none()
                && recovery.failure.as_deref() == Some(failure)
                && recovery.updated_at == canonical_timestamp(*completed_at)
                && recovery.state
                    == if *retryable {
                        GatewayReplicaRecoveryState::Required
                    } else {
                        GatewayReplicaRecoveryState::Diverged
                    }
        }
    }
}

fn outcome_is_superseded(rollout: &GatewayRollout, command: &GatewayObservationCommand) -> bool {
    recovery(rollout, command.node_id).is_some_and(|recovery| recovery.attempt > command.attempt)
}

fn canonicalize_observation(
    mut observation: a3s_cloud_contracts::NodeGatewaySnapshotObservation,
) -> a3s_cloud_contracts::NodeGatewaySnapshotObservation {
    observation.observed_at = canonical_timestamp(observation.observed_at);
    if let Some(applied) = &mut observation.applied {
        applied.issued_at = canonical_timestamp(applied.issued_at);
        applied.expires_at = canonical_timestamp(applied.expires_at);
        applied.applied_at = canonical_timestamp(applied.applied_at);
    }
    observation
}

#[derive(Debug, Clone, Copy)]
struct GatewayRecoveryPreparationFailure {
    operation: &'static str,
    error: &'static str,
}

impl GatewayRecoveryPreparationFailure {
    const fn new(operation: &'static str, error: &'static str) -> Self {
        Self { operation, error }
    }
}

const fn failure(
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    operation: &'static str,
    error: &'static str,
) -> GatewayReplicaRecoveryReconciliationFailure {
    GatewayReplicaRecoveryReconciliationFailure {
        rollout_id,
        node_id,
        operation,
        error,
    }
}
