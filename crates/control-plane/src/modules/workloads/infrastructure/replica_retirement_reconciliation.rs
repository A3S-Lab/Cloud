use super::reconciliation::IWorkloadRuntimeControl;
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{
    NodeCommandId, RepositoryError, ResourceClaimId, WorkloadId,
};
use crate::modules::workloads::domain::entities::{
    ResourceClaim, ResourceClaimReleaseEvidence, ResourceClaimState, WorkloadReplicaLifecycle,
};
use crate::modules::workloads::domain::repositories::{
    IResourceClaimRepository, IWorkloadReplicaRetirementRepository, ReplicaRetirementCompletion,
    ReplicaRetirementDispatch, ReplicaRuntimeFence, RetiringReplicaTarget,
};
use a3s_cloud_contracts::{
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodeResourceClaimBinding,
    NodeResourceClaimRelease,
};
use a3s_runtime::contract::{RuntimeActionRequest, RuntimeRemoval};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const RETIREMENT_COMMAND_DOMAIN: &str = "a3s.cloud.workload-replica-retirement.v1";
const RETIREMENT_RETRY_DOMAIN: &[u8] = b"a3s.cloud.workload-replica-retirement.retry.v1";
const RETIREMENT_CORRELATION_DOMAIN: &str = "a3s.cloud.workload-replica-retirement-correlation.v1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicaRetirementReport {
    pub targets: usize,
    pub removal_commands: usize,
    pub release_commands: usize,
    pub runtime_fences: usize,
    pub claims_released: usize,
    pub retired: usize,
    pub replayed: usize,
    pub pending: usize,
    pub failures: Vec<ReplicaRetirementFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaRetirementFailure {
    pub workload_id: WorkloadId,
    pub replica_id: crate::modules::shared_kernel::domain::WorkloadReplicaId,
    pub message: String,
}

pub struct ReplicaRetirementReconciler {
    retirements: Arc<dyn IWorkloadReplicaRetirementRepository>,
    control: Arc<dyn IWorkloadRuntimeControl>,
    resource_claims: Arc<dyn IResourceClaimRepository>,
    reconcile_interval: Duration,
    command_ttl: chrono::Duration,
    runtime_remove_timeout: chrono::Duration,
    release_horizon: chrono::Duration,
    batch_size: usize,
}

impl ReplicaRetirementReconciler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        retirements: Arc<dyn IWorkloadReplicaRetirementRepository>,
        control: Arc<dyn IWorkloadRuntimeControl>,
        resource_claims: Arc<dyn IResourceClaimRepository>,
        reconcile_interval: Duration,
        command_ttl: Duration,
        runtime_remove_timeout: Duration,
        release_horizon: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if reconcile_interval.is_zero()
            || command_ttl.is_zero()
            || runtime_remove_timeout.is_zero()
            || release_horizon.is_zero()
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err("replica retirement reconciliation policy is invalid".into());
        }
        Ok(Self {
            retirements,
            control,
            resource_claims,
            reconcile_interval,
            command_ttl: chrono::Duration::from_std(command_ttl)
                .map_err(|_| "replica retirement command TTL exceeds supported bounds")?,
            runtime_remove_timeout: chrono::Duration::from_std(runtime_remove_timeout)
                .map_err(|_| "replica Runtime removal timeout exceeds supported bounds")?,
            release_horizon: chrono::Duration::from_std(release_horizon)
                .map_err(|_| "replica Claim release horizon exceeds supported bounds")?,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<ReplicaRetirementReport, RepositoryError> {
        let targets = self
            .retirements
            .pending_replica_retirements(self.batch_size)
            .await?;
        let mut report = ReplicaRetirementReport {
            targets: targets.len(),
            ..ReplicaRetirementReport::default()
        };
        for target in targets {
            let identity = (target.replica.workload_id, target.replica.id);
            if let Err(message) = self.reconcile_target(target, now, &mut report).await {
                report.failures.push(ReplicaRetirementFailure {
                    workload_id: identity.0,
                    replica_id: identity.1,
                    message,
                });
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.reconcile_interval);
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
                            for failure in &report.failures {
                                tracing::warn!(
                                    workload_id = %failure.workload_id,
                                    replica_id = %failure.replica_id,
                                    error = %failure.message,
                                    "Workload replica retirement reconciliation failed"
                                );
                            }
                            tracing::debug!(
                                targets = report.targets,
                                removal_commands = report.removal_commands,
                                release_commands = report.release_commands,
                                runtime_fences = report.runtime_fences,
                                claims_released = report.claims_released,
                                retired = report.retired,
                                replayed = report.replayed,
                                pending = report.pending,
                                failures = report.failures.len(),
                                "Workload replica retirement reconciliation cycle completed"
                            );
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Workload replica retirement candidate scan failed"
                        ),
                    }
                }
            }
        }
    }

    async fn reconcile_target(
        &self,
        mut target: RetiringReplicaTarget,
        now: DateTime<Utc>,
        report: &mut ReplicaRetirementReport,
    ) -> Result<(), String> {
        validate_target(&target)?;
        let runtime_fenced_at = if let Some(fenced_at) = target.replica.runtime_fenced_at {
            fenced_at
        } else if target.member.node_id.is_some() {
            match self.reconcile_runtime_removal(&target, now, report).await? {
                RemovalProgress::Pending => {
                    report.pending += 1;
                    return Ok(());
                }
                RemovalProgress::Fenced {
                    command_id,
                    fenced_at,
                } => {
                    target.replica = self
                        .retirements
                        .record_replica_runtime_fenced(ReplicaRuntimeFence {
                            organization_id: target.replica.organization_id,
                            workload_id: target.replica.workload_id,
                            replica_id: target.replica.id,
                            replica_generation: target.replica.generation,
                            expected_replica_version: target.replica.aggregate_version,
                            command_id,
                            fenced_at: fenced_at.max(target.replica.updated_at),
                        })
                        .await
                        .map_err(repository_error("persist replica Runtime fencing evidence"))?;
                    report.runtime_fences += 1;
                    target
                        .replica
                        .runtime_fenced_at
                        .ok_or_else(|| "fenced replica omitted its evidence time".to_string())?
                }
            }
        } else {
            now.max(target.replica.updated_at)
                .max(target.member.updated_at)
        };

        let claim_released_at = match self
            .reconcile_claim_release(&target, runtime_fenced_at, now, report)
            .await?
        {
            ClaimReleaseProgress::Pending => {
                report.pending += 1;
                return Ok(());
            }
            ClaimReleaseProgress::Released(released_at) => released_at,
        };
        let completed_at = now
            .max(runtime_fenced_at)
            .max(claim_released_at)
            .max(target.replica.updated_at)
            .max(target.member.updated_at);
        let completion = self
            .retirements
            .complete_replica_retirement(ReplicaRetirementCompletion {
                organization_id: target.replica.organization_id,
                workload_id: target.replica.workload_id,
                replica_id: target.replica.id,
                replica_generation: target.replica.generation,
                expected_replica_version: target.replica.aggregate_version,
                member_id: target.member.id,
                expected_member_version: target.member.aggregate_version,
                fenced_node_id: target.member.node_id,
                completed_at,
                correlation_id: retirement_correlation_id(&target),
            })
            .await
            .map_err(repository_error("complete Workload replica retirement"))?;
        if completion.replayed {
            report.replayed += 1;
        } else {
            report.retired += 1;
        }
        Ok(())
    }

    async fn reconcile_runtime_removal(
        &self,
        target: &RetiringReplicaTarget,
        now: DateTime<Utc>,
        report: &mut ReplicaRetirementReport,
    ) -> Result<RemovalProgress, String> {
        let _deployment = target
            .deployment
            .as_ref()
            .ok_or_else(|| "placed retiring replica omitted its deployment".to_string())?;
        let node_id = target
            .member
            .node_id
            .ok_or_else(|| "dispatched retiring replica omitted its member node".to_string())?;
        let command_id = match target.replica.retirement_command_id {
            Some(command_id) => command_id,
            None => first_retirement_command_id(target),
        };
        let command = match self
            .control
            .find_command(node_id, command_id)
            .await
            .map_err(repository_error("load replica Runtime removal command"))?
        {
            Some(command) => command,
            None => {
                let command = self.enqueue_removal(target, command_id, now).await?;
                report.removal_commands += 1;
                if target.replica.retirement_command_id != Some(command.id) {
                    self.retirements
                        .dispatch_replica_retirement(ReplicaRetirementDispatch {
                            organization_id: target.replica.organization_id,
                            workload_id: target.replica.workload_id,
                            replica_id: target.replica.id,
                            replica_generation: target.replica.generation,
                            expected_replica_version: target.replica.aggregate_version,
                            command_id: command.id,
                            dispatched_at: command.issued_at.max(target.replica.updated_at),
                        })
                        .await
                        .map_err(repository_error("persist replica Runtime removal dispatch"))?;
                }
                return Ok(RemovalProgress::Pending);
            }
        };
        validate_removal_command(&command, target)?;
        if target.replica.retirement_command_id != Some(command.id) {
            self.retirements
                .dispatch_replica_retirement(ReplicaRetirementDispatch {
                    organization_id: target.replica.organization_id,
                    workload_id: target.replica.workload_id,
                    replica_id: target.replica.id,
                    replica_generation: target.replica.generation,
                    expected_replica_version: target.replica.aggregate_version,
                    command_id: command.id,
                    dispatched_at: command.issued_at.max(target.replica.updated_at),
                })
                .await
                .map_err(repository_error("recover replica Runtime removal dispatch"))?;
            return Ok(RemovalProgress::Pending);
        }
        if let Some(acknowledgement) = self
            .control
            .command_acknowledgement(node_id, command.id)
            .await
            .map_err(repository_error(
                "load replica Runtime removal acknowledgement",
            ))?
        {
            return match acknowledgement.outcome {
                NodeCommandOutcome::Succeeded { result } => {
                    let NodeCommandResult::RuntimeRemoved { removal } = result.as_ref() else {
                        return Err(
                            "replica Runtime removal acknowledgement has the wrong result".into(),
                        );
                    };
                    validate_removal_result(removal, &command, target)?;
                    Ok(RemovalProgress::Fenced {
                        command_id: command.id,
                        fenced_at: acknowledgement.completed_at,
                    })
                }
                NodeCommandOutcome::Rejected { failure }
                | NodeCommandOutcome::Failed { failure }
                    if failure.retryable || failure.code == "command_expired" =>
                {
                    self.enqueue_retry(target, command.id, now, report).await?;
                    Ok(RemovalProgress::Pending)
                }
                NodeCommandOutcome::Rejected { failure }
                | NodeCommandOutcome::Failed { failure } => Err(format!(
                    "replica Runtime removal failed with {}: {}",
                    failure.code, failure.message
                )),
            };
        }
        if now >= command.not_after {
            self.enqueue_retry(target, command.id, now, report).await?;
        }
        Ok(RemovalProgress::Pending)
    }

    async fn enqueue_retry(
        &self,
        target: &RetiringReplicaTarget,
        previous_command_id: NodeCommandId,
        now: DateTime<Utc>,
        report: &mut ReplicaRetirementReport,
    ) -> Result<(), String> {
        let command_id = NodeCommandId::from_uuid(Uuid::new_v5(
            &previous_command_id.as_uuid(),
            RETIREMENT_RETRY_DOMAIN,
        ));
        let command = self.enqueue_removal(target, command_id, now).await?;
        report.removal_commands += 1;
        self.retirements
            .dispatch_replica_retirement(ReplicaRetirementDispatch {
                organization_id: target.replica.organization_id,
                workload_id: target.replica.workload_id,
                replica_id: target.replica.id,
                replica_generation: target.replica.generation,
                expected_replica_version: target.replica.aggregate_version,
                command_id: command.id,
                dispatched_at: command.issued_at.max(target.replica.updated_at),
            })
            .await
            .map_err(repository_error("persist replica Runtime removal retry"))?;
        Ok(())
    }

    async fn enqueue_removal(
        &self,
        target: &RetiringReplicaTarget,
        command_id: NodeCommandId,
        now: DateTime<Utc>,
    ) -> Result<NodeCommand, String> {
        let node_id = target
            .member
            .node_id
            .ok_or_else(|| "retiring Runtime removal omitted its node".to_string())?;
        let issued_at = now
            .max(target.replica.updated_at)
            .max(target.member.updated_at);
        let not_after = checked_add(issued_at, self.command_ttl, "Runtime removal command")?;
        let runtime_deadline = checked_add(
            issued_at,
            self.runtime_remove_timeout,
            "replica Runtime removal",
        )?
        .min(not_after);
        let request = RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("replica-retirement:{command_id}:remove"),
            unit_id: target
                .replica_binding
                .as_ref()
                .ok_or_else(|| "retiring Runtime removal omitted its binding".to_string())?
                .runtime_unit_id
                .clone(),
            generation: target.replica.generation,
            deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
        };
        let command = self
            .control
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: command_id,
                node_id,
                aggregate_id: target.replica.id.as_uuid(),
                payload: NodeCommandPayload::RuntimeRemove { request },
                issued_at,
                not_after,
                correlation_id: retirement_correlation_id(target),
            })
            .await
            .map_err(repository_error("enqueue replica Runtime removal"))?
            .value;
        validate_removal_command(&command, target)?;
        Ok(command)
    }

    async fn reconcile_claim_release(
        &self,
        target: &RetiringReplicaTarget,
        runtime_fenced_at: DateTime<Utc>,
        now: DateTime<Utc>,
        report: &mut ReplicaRetirementReport,
    ) -> Result<ClaimReleaseProgress, String> {
        let Some(deployment) = &target.deployment else {
            return Ok(ClaimReleaseProgress::Released(runtime_fenced_at));
        };
        let mut claim = match self
            .resource_claims
            .find(
                target.replica.organization_id,
                ResourceClaimId::from_uuid(deployment.id.as_uuid()),
            )
            .await
        {
            Ok(claim) => claim,
            Err(RepositoryError::NotFound) => {
                return Ok(ClaimReleaseProgress::Released(runtime_fenced_at))
            }
            Err(error) => return Err(format!("load retiring replica Claim: {error}")),
        };
        validate_claim(&claim, target)?;
        for _ in 0..4 {
            match claim.state {
                ResourceClaimState::Released => {
                    return Ok(ClaimReleaseProgress::Released(
                        claim.released_at.ok_or_else(|| {
                            "released Claim omitted its evidence time".to_string()
                        })?,
                    ))
                }
                ResourceClaimState::ReservedInDb => {
                    claim = self
                        .resource_claims
                        .cancel_database_reservation(
                            claim.organization_id,
                            claim.id,
                            claim.aggregate_version,
                            now.max(runtime_fenced_at).max(claim.updated_at),
                        )
                        .await
                        .map_err(repository_error("cancel retiring replica database Claim"))?;
                    report.claims_released += 1;
                }
                ResourceClaimState::PreparingOnAgent => {
                    claim = self
                        .resource_claims
                        .orphan(
                            claim.organization_id,
                            claim.id,
                            claim.aggregate_version,
                            "replica retirement superseded in-flight Claim preparation".into(),
                            now.max(claim.updated_at),
                        )
                        .await
                        .map_err(repository_error("fence in-flight retiring replica Claim"))?;
                }
                ResourceClaimState::PreparedOnAgent
                | ResourceClaimState::BoundToRuntimeUnit
                | ResourceClaimState::Orphaned => {
                    let next_generation =
                        claim.claim_generation.checked_add(1).ok_or_else(|| {
                            "retiring replica Claim generation overflowed".to_string()
                        })?;
                    let command_id = release_command_id(&claim, next_generation);
                    claim = self
                        .resource_claims
                        .begin_release(
                            claim.organization_id,
                            claim.id,
                            claim.aggregate_version,
                            command_id,
                            now.max(runtime_fenced_at).max(claim.updated_at),
                        )
                        .await
                        .map_err(repository_error("persist retiring replica Claim release"))?;
                }
                ResourceClaimState::Releasing => break,
            }
        }
        if claim.state != ResourceClaimState::Releasing {
            return Err("retiring replica Claim release did not reach a dispatchable state".into());
        }
        let binding = self.load_prepared_binding(&claim).await?;
        let command_id = claim
            .release_command_id
            .ok_or_else(|| "releasing replica Claim omitted its command".to_string())?;
        let issued_at = claim
            .release_requested_at
            .ok_or_else(|| "releasing replica Claim omitted its request time".to_string())?;
        let not_after = checked_add(issued_at, self.command_ttl, "Claim release command")?.min(
            checked_add(now, self.release_horizon, "Claim release horizon")?,
        );
        if now >= not_after {
            self.resource_claims
                .orphan(
                    claim.organization_id,
                    claim.id,
                    claim.aggregate_version,
                    "retiring replica Claim release command expired without evidence".into(),
                    now.max(claim.updated_at),
                )
                .await
                .map_err(repository_error("orphan expired retiring replica Claim"))?;
            return Ok(ClaimReleaseProgress::Pending);
        }
        let request = NodeResourceClaimRelease {
            schema: NodeResourceClaimRelease::SCHEMA.into(),
            claim_generation: claim.claim_generation,
            claim_digest: claim.claim_digest.clone(),
            binding,
        };
        let command = match self
            .control
            .find_command(claim.node_id, command_id)
            .await
            .map_err(repository_error(
                "load retiring replica Claim release command",
            ))? {
            Some(command) => command,
            None => {
                report.release_commands += 1;
                self.control
                    .enqueue_command(NodeCommandDraft {
                        proposed_command_id: command_id,
                        node_id: claim.node_id,
                        aggregate_id: claim.id.as_uuid(),
                        payload: NodeCommandPayload::ResourceClaimRelease {
                            request: Box::new(request.clone()),
                        },
                        issued_at,
                        not_after,
                        correlation_id: claim.deployment_id.as_uuid(),
                    })
                    .await
                    .map_err(repository_error("enqueue retiring replica Claim release"))?
                    .value
            }
        };
        validate_release_command(&command, &claim, &request)?;
        let Some(acknowledgement) = self
            .control
            .command_acknowledgement(claim.node_id, command.id)
            .await
            .map_err(repository_error(
                "load retiring replica Claim release acknowledgement",
            ))?
        else {
            return Ok(ClaimReleaseProgress::Pending);
        };
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                let NodeCommandResult::ResourceClaimReleased { released } = result.as_ref() else {
                    return Err("retiring replica Claim release has the wrong result".into());
                };
                released.validate_for(&request).map_err(|error| {
                    format!("retiring replica Claim evidence is invalid: {error}")
                })?;
                if released.released_at < runtime_fenced_at {
                    return Err(
                        "retiring replica Claim release predates Runtime fencing evidence".into(),
                    );
                }
                let evidence = ResourceClaimReleaseEvidence::AgentReleased {
                    command_id: command.id,
                    slots: released.slots.clone(),
                    evidence_digest: released
                        .evidence_digest()
                        .map_err(|error| format!("digest retiring Claim evidence: {error}"))?,
                    observed_at: released.released_at,
                };
                claim = self
                    .resource_claims
                    .record_released(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        evidence,
                        acknowledgement.completed_at.max(claim.updated_at),
                    )
                    .await
                    .map_err(repository_error("persist retiring replica Claim evidence"))?;
                report.claims_released += 1;
                Ok(ClaimReleaseProgress::Released(
                    claim
                        .released_at
                        .ok_or_else(|| "released Claim omitted its evidence time".to_string())?,
                ))
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                self.resource_claims
                    .orphan(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        bounded_reason(format!(
                            "retiring replica Claim release failed with {}: {}",
                            failure.code, failure.message
                        )),
                        acknowledgement.completed_at.max(claim.updated_at),
                    )
                    .await
                    .map_err(repository_error("orphan failed retiring replica Claim"))?;
                Ok(ClaimReleaseProgress::Pending)
            }
        }
    }

    async fn load_prepared_binding(
        &self,
        claim: &ResourceClaim,
    ) -> Result<NodeResourceClaimBinding, String> {
        let command_id = claim.prepare_command_id.ok_or_else(|| {
            "issued retiring replica Claim omitted its preparation command".to_string()
        })?;
        let command = self
            .control
            .find_command(claim.node_id, command_id)
            .await
            .map_err(repository_error("load retiring replica Claim preparation"))?
            .ok_or_else(|| "retiring replica Claim preparation command is missing".to_string())?;
        if command.id != command_id
            || command.node_id != claim.node_id
            || command.aggregate_id != claim.id.as_uuid()
        {
            return Err("retiring replica Claim preparation identity changed".into());
        }
        let NodeCommandPayload::ResourceClaimPrepare { request } = &command.payload else {
            return Err("retiring replica Claim preparation has the wrong payload".into());
        };
        let expected = claim
            .node_binding(request.binding.agent_instance_id)
            .map_err(|error| format!("validate retiring replica Claim binding: {error}"))?;
        if request.binding != expected {
            return Err("retiring replica durable Claim binding changed".into());
        }
        Ok(request.binding.clone())
    }
}

enum RemovalProgress {
    Pending,
    Fenced {
        command_id: NodeCommandId,
        fenced_at: DateTime<Utc>,
    },
}

enum ClaimReleaseProgress {
    Pending,
    Released(DateTime<Utc>),
}

fn validate_target(target: &RetiringReplicaTarget) -> Result<(), String> {
    target.replica.validate()?;
    target.member.validate()?;
    if target.replica.lifecycle != WorkloadReplicaLifecycle::Retiring
        || target.revision.workload_id != target.replica.workload_id
        || target.revision.id != target.replica.revision_id
        || target.revision.generation != target.replica.revision_generation
        || target.member.organization_id != target.replica.organization_id
        || target.member.workload_id != target.replica.workload_id
        || target.member.replica_id != target.replica.id
        || target.deployment.is_some() != target.replica_binding.is_some()
        || target.member.node_id.is_some() && target.replica_binding.is_none()
        || target.replica.runtime_fenced_at.is_some()
            && target.replica.retirement_command_id.is_none()
    {
        return Err("retiring Workload replica target is inconsistent".into());
    }
    if let (Some(deployment), Some(binding)) = (&target.deployment, &target.replica_binding) {
        if deployment.id != binding.deployment_id
            || deployment.organization_id != target.replica.organization_id
            || deployment.workload_id != target.replica.workload_id
            || deployment.revision_id != target.replica.revision_id
            || binding.organization_id != target.replica.organization_id
            || binding.workload_id != target.replica.workload_id
            || binding.revision_id != target.replica.revision_id
            || binding.replica_id != target.replica.id
            || binding.replica_generation != target.replica.generation
            || binding.member_id != target.member.id
            || binding.runtime_generation != target.replica.generation
            || binding.node_id != deployment.node_id
            || binding.node_id.is_some() && binding.node_id != target.member.node_id
            || deployment.command_id.is_some() && target.member.node_id.is_none()
        {
            return Err("retiring Workload replica binding is inconsistent".into());
        }
    }
    Ok(())
}

fn validate_claim(claim: &ResourceClaim, target: &RetiringReplicaTarget) -> Result<(), String> {
    claim.validate()?;
    let deployment = target
        .deployment
        .as_ref()
        .ok_or_else(|| "retiring replica Claim has no deployment".to_string())?;
    let binding = target
        .replica_binding
        .as_ref()
        .ok_or_else(|| "retiring replica Claim has no binding".to_string())?;
    if claim.organization_id != target.replica.organization_id
        || claim.workload_id != target.replica.workload_id
        || claim.deployment_id != deployment.id
        || claim.replica_id != target.replica.id
        || claim.replica_generation != target.replica.generation
        || claim.member_id != target.member.id
        || claim.runtime_unit_id != binding.runtime_unit_id
        || claim.runtime_generation != binding.runtime_generation
        || target
            .member
            .node_id
            .is_some_and(|node_id| claim.node_id != node_id)
    {
        return Err("retiring replica Claim changed its exact binding".into());
    }
    Ok(())
}

fn validate_removal_command(
    command: &NodeCommand,
    target: &RetiringReplicaTarget,
) -> Result<(), String> {
    let binding = target
        .replica_binding
        .as_ref()
        .ok_or_else(|| "retiring Runtime command omitted its binding".to_string())?;
    if command.node_id
        != target
            .member
            .node_id
            .ok_or_else(|| "retiring Runtime command omitted its member node".to_string())?
        || command.aggregate_id != target.replica.id.as_uuid()
        || command.correlation_id != retirement_correlation_id(target)
    {
        return Err("retiring Runtime command identity changed".into());
    }
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err("retiring Runtime command has the wrong payload".into());
    };
    if request.request_id != format!("replica-retirement:{}:remove", command.id)
        || request.unit_id != binding.runtime_unit_id
        || request.generation != binding.runtime_generation
    {
        return Err("retiring Runtime removal changed its exact Runtime identity".into());
    }
    Ok(())
}

fn validate_removal_result(
    removal: &RuntimeRemoval,
    command: &NodeCommand,
    target: &RetiringReplicaTarget,
) -> Result<(), String> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        unreachable!("validated Runtime removal command");
    };
    if removal.request_id != request.request_id
        || removal.unit_id != request.unit_id
        || removal.generation != request.generation
        || removal.unit_id
            != target
                .replica_binding
                .as_ref()
                .ok_or_else(|| "retiring Runtime result omitted its binding".to_string())?
                .runtime_unit_id
    {
        return Err("retiring Runtime removal evidence changed identity".into());
    }
    Ok(())
}

fn validate_release_command(
    command: &NodeCommand,
    claim: &ResourceClaim,
    request: &NodeResourceClaimRelease,
) -> Result<(), String> {
    if command.id
        != claim
            .release_command_id
            .ok_or_else(|| "releasing replica Claim omitted its command identity".to_string())?
        || command.node_id != claim.node_id
        || command.aggregate_id != claim.id.as_uuid()
        || command.correlation_id != claim.deployment_id.as_uuid()
    {
        return Err("retiring replica Claim release command identity changed".into());
    }
    let NodeCommandPayload::ResourceClaimRelease { request: persisted } = &command.payload else {
        return Err("retiring replica Claim release command has the wrong payload".into());
    };
    if persisted.as_ref() != request {
        return Err("retiring replica Claim release command changed its exact Claim".into());
    }
    Ok(())
}

fn first_retirement_command_id(target: &RetiringReplicaTarget) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &target.replica.id.as_uuid(),
        format!("{RETIREMENT_COMMAND_DOMAIN}:{}", target.replica.generation).as_bytes(),
    ))
}

fn retirement_correlation_id(target: &RetiringReplicaTarget) -> Uuid {
    Uuid::new_v5(
        &target.replica.id.as_uuid(),
        format!(
            "{RETIREMENT_CORRELATION_DOMAIN}:{}",
            target.replica.generation
        )
        .as_bytes(),
    )
}

fn release_command_id(claim: &ResourceClaim, generation: u64) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &claim.id.as_uuid(),
        format!("resource-claim-release:{generation}").as_bytes(),
    ))
}

fn checked_add(
    at: DateTime<Utc>,
    duration: chrono::Duration,
    label: &str,
) -> Result<DateTime<Utc>, String> {
    at.checked_add_signed(duration)
        .ok_or_else(|| format!("{label} deadline overflowed"))
}

fn timestamp_millis(at: DateTime<Utc>) -> Result<u64, String> {
    u64::try_from(at.timestamp_millis())
        .map_err(|_| "replica Runtime deadline predates the Unix epoch".into())
}

fn bounded_reason(reason: String) -> String {
    const MAX_REASON_BYTES: usize = 16 * 1024;
    if reason.len() <= MAX_REASON_BYTES {
        return reason.replace(['\0', '\r', '\n'], " ");
    }
    let mut end = MAX_REASON_BYTES;
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    reason[..end].replace(['\0', '\r', '\n'], " ")
}

fn repository_error(context: &'static str) -> impl FnOnce(RepositoryError) -> String {
    move |error| format!("{context}: {error}")
}

#[cfg(test)]
mod tests;
