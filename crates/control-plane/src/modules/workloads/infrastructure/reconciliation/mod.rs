use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, RuntimeObservationRecord,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeCommandId, NodeId, RepositoryError, ResourceClaimId,
};
use crate::modules::workloads::application::project_replica_runtime_spec;
use crate::modules::workloads::domain::entities::{
    DeploymentStatus, ResourceClaim, ResourceClaimState, WorkloadDesiredState,
    WorkloadReplicaLifecycle,
};
use crate::modules::workloads::domain::repositories::{
    ActiveRuntimeTarget, IResourceClaimRepository, IWorkloadRuntimeTargetRepository,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeResourceClaimBinding,
};
use a3s_runtime::contract::{
    RuntimeApplyRequest, RuntimeInspection, RuntimeUnitSpec, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const MAX_COMMAND_CHAIN: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkloadReconciliationReport {
    pub targets: usize,
    pub converged: usize,
    pub inspect_commands: usize,
    pub recovery_commands: usize,
    pub pending_commands: usize,
    pub completed_commands: usize,
    pub failures: Vec<WorkloadReconciliationFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadReconciliationFailure {
    pub workload_id: crate::modules::shared_kernel::domain::WorkloadId,
    pub message: String,
}

#[async_trait]
pub trait IWorkloadRuntimeControl: Send + Sync {
    async fn enqueue_command(
        &self,
        draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError>;

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError>;
}

#[async_trait]
impl<T> IWorkloadRuntimeControl for T
where
    T: INodeControlRepository + Send + Sync,
{
    async fn enqueue_command(
        &self,
        draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError> {
        INodeControlRepository::enqueue_command(self, draft).await
    }

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError> {
        INodeControlRepository::find_command(self, node_id, command_id).await
    }

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError> {
        INodeControlRepository::command_acknowledgement(self, node_id, command_id).await
    }

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
        INodeControlRepository::latest_runtime_observation(self, node_id, unit_id, generation).await
    }
}

pub struct WorkloadRuntimeReconciler {
    targets: Arc<dyn IWorkloadRuntimeTargetRepository>,
    control: Arc<dyn IWorkloadRuntimeControl>,
    resource_claims: Arc<dyn IResourceClaimRepository>,
    reconcile_interval: Duration,
    reconcile_interval_chrono: chrono::Duration,
    command_ttl: chrono::Duration,
    runtime_apply_timeout: chrono::Duration,
    batch_size: usize,
}

impl WorkloadRuntimeReconciler {
    pub fn new(
        targets: Arc<dyn IWorkloadRuntimeTargetRepository>,
        control: Arc<dyn IWorkloadRuntimeControl>,
        resource_claims: Arc<dyn IResourceClaimRepository>,
        reconcile_interval: Duration,
        command_ttl: Duration,
        runtime_apply_timeout: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if reconcile_interval.is_zero()
            || command_ttl.is_zero()
            || runtime_apply_timeout.is_zero()
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err("workload reconciliation policy is invalid".into());
        }
        let reconcile_interval_chrono = chrono::Duration::from_std(reconcile_interval)
            .map_err(|_| "workload reconciliation interval exceeds supported bounds")?;
        let command_ttl = chrono::Duration::from_std(command_ttl)
            .map_err(|_| "workload reconciliation command TTL exceeds supported bounds")?;
        let runtime_apply_timeout = chrono::Duration::from_std(runtime_apply_timeout)
            .map_err(|_| "workload reconciliation apply timeout exceeds supported bounds")?;
        Ok(Self {
            targets,
            control,
            resource_claims,
            reconcile_interval,
            reconcile_interval_chrono,
            command_ttl,
            runtime_apply_timeout,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<WorkloadReconciliationReport, RepositoryError> {
        let targets = self
            .targets
            .list_active_runtime_targets(self.batch_size)
            .await?;
        let mut report = WorkloadReconciliationReport {
            targets: targets.len(),
            ..WorkloadReconciliationReport::default()
        };
        for target in targets {
            if let Err(message) = self.reconcile_target(&target, now, &mut report).await {
                report.failures.push(WorkloadReconciliationFailure {
                    workload_id: target.workload.id,
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
                                    error = %failure.message,
                                    "workload Runtime reconciliation failed"
                                );
                            }
                            tracing::debug!(
                                targets = report.targets,
                                converged = report.converged,
                                inspect_commands = report.inspect_commands,
                                recovery_commands = report.recovery_commands,
                                pending_commands = report.pending_commands,
                                completed_commands = report.completed_commands,
                                failures = report.failures.len(),
                                "workload Runtime reconciliation cycle completed"
                            );
                        }
                        Err(error) => tracing::error!(error = %error, "workload Runtime reconciliation cycle failed"),
                    }
                }
            }
        }
    }

    async fn reconcile_target(
        &self,
        target: &ActiveRuntimeTarget,
        now: DateTime<Utc>,
        report: &mut WorkloadReconciliationReport,
    ) -> Result<(), String> {
        validate_target(target)?;
        let node_id = target
            .deployment
            .node_id
            .ok_or_else(|| "active deployment omitted its node".to_string())?;
        let spec = project_replica_runtime_spec(&target.revision, &target.replica)?;
        let resource_binding = self.resource_binding(target, &spec, node_id).await?;
        let context = ReconciliationContext {
            target,
            spec: &spec,
            node_id,
            resource_binding: resource_binding.as_ref(),
            now,
        };
        let latest = self
            .control
            .latest_runtime_observation(node_id, &spec.unit_id, spec.generation)
            .await
            .map_err(repository_error("load latest Runtime observation"))?;

        let Some(latest) = latest else {
            let evidence_id = target
                .deployment
                .command_id
                .map(NodeCommandId::as_uuid)
                .unwrap_or_else(|| target.deployment.id.as_uuid());
            return self.ensure_inspection(context, evidence_id, report).await;
        };
        latest
            .observation
            .validate_against(&spec)
            .map_err(|error| format!("latest Runtime observation is inconsistent: {error}"))?;
        validate_resource_observation(resource_binding.as_ref(), &latest.observation)?;
        if latest.observation.state == RuntimeUnitState::Unknown {
            return self
                .ensure_recovery(context, latest.report_id, report)
                .await;
        }
        if latest.observation.state.is_terminal() {
            return Err(format!(
                "desired running workload has terminal Runtime state {:?}; a new generation is required",
                latest.observation.state
            ));
        }
        let due_at = latest
            .received_at
            .checked_add_signed(self.reconcile_interval_chrono)
            .ok_or_else(|| "workload reconciliation due time overflowed".to_string())?;
        if now < due_at {
            report.converged += 1;
            return Ok(());
        }
        self.ensure_inspection(context, latest.report_id, report)
            .await
    }

    async fn ensure_inspection(
        &self,
        context: ReconciliationContext<'_>,
        mut evidence_id: Uuid,
        report: &mut WorkloadReconciliationReport,
    ) -> Result<(), String> {
        let ReconciliationContext {
            target,
            spec,
            node_id,
            resource_binding,
            now,
        } = context;
        for _ in 0..MAX_COMMAND_CHAIN {
            let command_id = reconciliation_command_id("inspect", target, evidence_id);
            let command = match self
                .control
                .find_command(node_id, command_id)
                .await
                .map_err(repository_error("load Runtime inspect command"))?
            {
                Some(command) => command,
                None => {
                    let command = self
                        .enqueue_or_reload(
                            inspection_draft(
                                target,
                                spec,
                                node_id,
                                command_id,
                                now,
                                self.command_ttl,
                            )?,
                            ExpectedCommand::Inspect { spec },
                        )
                        .await?;
                    report.inspect_commands += 1;
                    command
                }
            };
            validate_command(
                &command,
                target,
                node_id,
                command_id,
                ExpectedCommand::Inspect { spec },
            )?;
            let Some(acknowledgement) = self
                .control
                .command_acknowledgement(node_id, command_id)
                .await
                .map_err(repository_error("load Runtime inspect acknowledgement"))?
            else {
                report.pending_commands += 1;
                return Ok(());
            };
            report.completed_commands += 1;
            match acknowledgement.outcome {
                NodeCommandOutcome::Succeeded { result } => match *result {
                    NodeCommandResult::RuntimeInspected {
                        inspection: RuntimeInspection::Found { observation, .. },
                    } => {
                        observation.validate_against(spec).map_err(|error| {
                            format!("Runtime inspect result is inconsistent: {error}")
                        })?;
                        validate_resource_observation(resource_binding, &observation)?;
                        if observation.state == RuntimeUnitState::Unknown {
                            return self
                                .ensure_recovery(context, command_id.as_uuid(), report)
                                .await;
                        }
                        report.converged += usize::from(observation.converges(spec));
                        return Ok(());
                    }
                    NodeCommandResult::RuntimeInspected {
                        inspection: RuntimeInspection::NotFound { unit_id, .. },
                    } if unit_id == spec.unit_id => {
                        return self
                            .ensure_recovery(context, command_id.as_uuid(), report)
                            .await;
                    }
                    _ => return Err("Runtime inspect acknowledgement has the wrong result".into()),
                },
                NodeCommandOutcome::Rejected { failure }
                | NodeCommandOutcome::Failed { failure }
                    if failure.retryable || failure.code == "command_expired" =>
                {
                    evidence_id = command_id.as_uuid();
                }
                NodeCommandOutcome::Rejected { failure }
                | NodeCommandOutcome::Failed { failure } => {
                    return Err(format!(
                        "Runtime inspect failed with {}: {}",
                        failure.code, failure.message
                    ));
                }
            }
        }
        Err("Runtime inspect retry chain exceeded its safety bound".into())
    }

    async fn ensure_recovery(
        &self,
        context: ReconciliationContext<'_>,
        mut evidence_id: Uuid,
        report: &mut WorkloadReconciliationReport,
    ) -> Result<(), String> {
        let ReconciliationContext {
            target,
            spec,
            node_id,
            resource_binding,
            ..
        } = context;
        for _ in 0..MAX_COMMAND_CHAIN {
            let command_id = reconciliation_command_id("apply", target, evidence_id);
            let command = match self
                .control
                .find_command(node_id, command_id)
                .await
                .map_err(repository_error("load Runtime recovery command"))?
            {
                Some(command) => command,
                None => {
                    let command = self
                        .enqueue_or_reload(
                            recovery_draft(
                                context,
                                command_id,
                                self.command_ttl,
                                self.runtime_apply_timeout,
                            )?,
                            ExpectedCommand::Apply {
                                spec,
                                command_id,
                                resource_binding,
                            },
                        )
                        .await?;
                    report.recovery_commands += 1;
                    command
                }
            };
            validate_command(
                &command,
                target,
                node_id,
                command_id,
                ExpectedCommand::Apply {
                    spec,
                    command_id,
                    resource_binding,
                },
            )?;
            let Some(acknowledgement) = self
                .control
                .command_acknowledgement(node_id, command_id)
                .await
                .map_err(repository_error("load Runtime recovery acknowledgement"))?
            else {
                report.pending_commands += 1;
                return Ok(());
            };
            report.completed_commands += 1;
            match acknowledgement.outcome {
                NodeCommandOutcome::Succeeded { result } => match *result {
                    NodeCommandResult::RuntimeApplied { observation } => {
                        observation.validate_against(spec).map_err(|error| {
                            format!("Runtime recovery result is inconsistent: {error}")
                        })?;
                        validate_resource_observation(resource_binding, &observation)?;
                        if observation.state == RuntimeUnitState::Unknown {
                            evidence_id = command_id.as_uuid();
                            continue;
                        }
                        if observation.state.is_terminal() {
                            return Err(format!(
                                "Runtime recovery returned terminal state {:?}",
                                observation.state
                            ));
                        }
                        report.converged += usize::from(observation.converges(spec));
                        return Ok(());
                    }
                    _ => return Err("Runtime recovery acknowledgement has the wrong result".into()),
                },
                NodeCommandOutcome::Rejected { failure }
                | NodeCommandOutcome::Failed { failure }
                    if failure.retryable || failure.code == "command_expired" =>
                {
                    evidence_id = command_id.as_uuid();
                }
                NodeCommandOutcome::Rejected { failure }
                | NodeCommandOutcome::Failed { failure } => {
                    return Err(format!(
                        "Runtime recovery failed with {}: {}",
                        failure.code, failure.message
                    ));
                }
            }
        }
        Err("Runtime recovery retry chain exceeded its safety bound".into())
    }

    async fn resource_binding(
        &self,
        target: &ActiveRuntimeTarget,
        spec: &RuntimeUnitSpec,
        node_id: NodeId,
    ) -> Result<Option<NodeResourceClaimBinding>, String> {
        let claim = match self
            .resource_claims
            .find(
                target.workload.organization_id,
                ResourceClaimId::from_uuid(target.deployment.id.as_uuid()),
            )
            .await
        {
            Ok(claim) => claim,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "load active Runtime resource claim for reconciliation: {error}"
                ))
            }
        };
        validate_reconciliation_claim(&claim, target, spec, node_id)?;
        match claim.state {
            // Deployment workflow versions 1 and 2 reserved only in the
            // database and remain executable during a rolling upgrade.
            ResourceClaimState::ReservedInDb => Ok(None),
            ResourceClaimState::BoundToRuntimeUnit => {
                let command_id = claim.prepare_command_id.ok_or_else(|| {
                    "bound Runtime resource claim omitted its preparation command".to_string()
                })?;
                let command = self
                    .control
                    .find_command(node_id, command_id)
                    .await
                    .map_err(repository_error(
                        "load durable resource preparation command",
                    ))?
                    .ok_or_else(|| {
                        "bound Runtime resource preparation command is missing".to_string()
                    })?;
                if command.id != command_id
                    || command.node_id != node_id
                    || command.aggregate_id != claim.id.as_uuid()
                {
                    return Err(
                        "bound Runtime resource preparation command identity changed".into(),
                    );
                }
                let NodeCommandPayload::ResourceClaimPrepare { request } = command.payload else {
                    return Err(
                        "bound Runtime resource preparation command has the wrong payload".into(),
                    );
                };
                let expected = claim
                    .node_binding(request.binding.agent_instance_id)
                    .map_err(|error| {
                        format!("validate durable Runtime resource binding: {error}")
                    })?;
                let binding_digest = request
                    .binding
                    .digest()
                    .map_err(|error| format!("digest durable Runtime resource binding: {error}"))?;
                if request.binding != expected
                    || claim.prepared_binding_digest.as_ref() != Some(&binding_digest)
                {
                    return Err("durable Runtime resource binding changed".into());
                }
                Ok(Some(request.binding))
            }
            ResourceClaimState::PreparingOnAgent
            | ResourceClaimState::PreparedOnAgent
            | ResourceClaimState::Releasing
            | ResourceClaimState::Released
            | ResourceClaimState::Orphaned => Err(format!(
                "active Runtime has unusable resource claim state {}",
                claim.state.as_str()
            )),
        }
    }

    async fn enqueue_or_reload(
        &self,
        draft: NodeCommandDraft,
        expected: ExpectedCommand<'_>,
    ) -> Result<NodeCommand, String> {
        let node_id = draft.node_id;
        let command_id = draft.proposed_command_id;
        match self.control.enqueue_command(draft).await {
            Ok(write) => Ok(write.value),
            Err(RepositoryError::Conflict(_)) => self
                .control
                .find_command(node_id, command_id)
                .await
                .map_err(repository_error(
                    "reload concurrently inserted Runtime command",
                ))?
                .ok_or_else(|| {
                    format!("Runtime command {command_id} conflicted without a persisted command")
                })
                .and_then(|command| {
                    validate_expected_payload(&command, expected)?;
                    Ok(command)
                }),
            Err(error) => Err(format!("enqueue Runtime reconciliation command: {error}")),
        }
    }
}

#[derive(Clone, Copy)]
enum ExpectedCommand<'a> {
    Inspect {
        spec: &'a RuntimeUnitSpec,
    },
    Apply {
        spec: &'a RuntimeUnitSpec,
        command_id: NodeCommandId,
        resource_binding: Option<&'a NodeResourceClaimBinding>,
    },
}

#[derive(Clone, Copy)]
struct ReconciliationContext<'a> {
    target: &'a ActiveRuntimeTarget,
    spec: &'a RuntimeUnitSpec,
    node_id: NodeId,
    resource_binding: Option<&'a NodeResourceClaimBinding>,
    now: DateTime<Utc>,
}

fn validate_target(target: &ActiveRuntimeTarget) -> Result<(), String> {
    target.replica.validate()?;
    target.member.validate()?;
    target.replica_binding.validate_against(
        &target.deployment,
        &target.revision,
        &target.replica,
        &target.member,
    )?;
    if target.workload.desired_state != WorkloadDesiredState::Running
        || target.workload.active_revision_id != Some(target.revision.id)
        || target.revision.workload_id != target.workload.id
        || target.replica.lifecycle != WorkloadReplicaLifecycle::Desired
        || target.replica.organization_id != target.workload.organization_id
        || target.replica.project_id != target.workload.project_id
        || target.replica.environment_id != target.workload.environment_id
        || target.replica.workload_id != target.workload.id
        || target.member.organization_id != target.workload.organization_id
        || target.member.project_id != target.workload.project_id
        || target.member.environment_id != target.workload.environment_id
        || target.member.workload_id != target.workload.id
        || target.member.replica_id != target.replica.id
        || target.deployment.organization_id != target.workload.organization_id
        || target.deployment.workload_id != target.workload.id
        || target.deployment.revision_id != target.revision.id
        || !matches!(
            target.deployment.status,
            DeploymentStatus::Retiring | DeploymentStatus::Active
        )
        || target.deployment.node_id.is_none()
        || target.deployment.command_id.is_none()
        || target.replica_binding.deployment_id != target.deployment.id
        || target.replica_binding.organization_id != target.workload.organization_id
        || target.replica_binding.project_id != target.workload.project_id
        || target.replica_binding.environment_id != target.workload.environment_id
        || target.replica_binding.workload_id != target.workload.id
        || target.replica_binding.revision_id != target.revision.id
        || target.replica_binding.replica_id != target.replica.id
        || target.replica_binding.replica_generation != target.replica.generation
        || target.replica_binding.member_id != target.member.id
        || target.replica_binding.node_id != target.deployment.node_id
        || target.replica_binding.node_id != target.member.node_id
        || target.replica_binding.placement_generation != target.member.placement_generation
    {
        return Err("active Runtime target is inconsistent".into());
    }
    Ok(())
}

fn reconciliation_command_id(
    kind: &str,
    target: &ActiveRuntimeTarget,
    evidence_id: Uuid,
) -> NodeCommandId {
    let name = format!(
        "a3s.cloud.workload-reconcile:{kind}:{}:{}:{}:{}:{evidence_id}",
        target.workload.id, target.revision.id, target.replica.id, target.replica.generation
    );
    NodeCommandId::from_uuid(Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes()))
}

fn reconciliation_correlation_id(target: &ActiveRuntimeTarget) -> Uuid {
    let name = format!(
        "a3s.cloud.workload-reconcile:{}:{}:{}:{}",
        target.workload.id, target.revision.id, target.replica.id, target.replica.generation
    );
    Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes())
}

fn inspection_draft(
    target: &ActiveRuntimeTarget,
    spec: &RuntimeUnitSpec,
    node_id: NodeId,
    command_id: NodeCommandId,
    issued_at: DateTime<Utc>,
    command_ttl: chrono::Duration,
) -> Result<NodeCommandDraft, String> {
    Ok(NodeCommandDraft {
        proposed_command_id: command_id,
        node_id,
        aggregate_id: target.replica.id.as_uuid(),
        payload: NodeCommandPayload::RuntimeInspect {
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
        },
        issued_at,
        not_after: checked_add(issued_at, command_ttl, "Runtime inspect command")?,
        correlation_id: reconciliation_correlation_id(target),
    })
}

fn recovery_draft(
    context: ReconciliationContext<'_>,
    command_id: NodeCommandId,
    command_ttl: chrono::Duration,
    runtime_apply_timeout: chrono::Duration,
) -> Result<NodeCommandDraft, String> {
    let ReconciliationContext {
        target,
        spec,
        node_id,
        resource_binding,
        now: issued_at,
    } = context;
    let not_after = checked_add(issued_at, command_ttl, "Runtime recovery command")?;
    let runtime_deadline =
        checked_add(issued_at, runtime_apply_timeout, "Runtime recovery apply")?.min(not_after);
    Ok(NodeCommandDraft {
        proposed_command_id: command_id,
        node_id,
        aggregate_id: target.replica.id.as_uuid(),
        payload: NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("workload-reconcile:{command_id}:apply"),
                deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
                spec: spec.clone(),
            }),
            resource_claim: resource_binding.cloned().map(Box::new),
        },
        issued_at,
        not_after,
        correlation_id: reconciliation_correlation_id(target),
    })
}

fn validate_command(
    command: &NodeCommand,
    target: &ActiveRuntimeTarget,
    node_id: NodeId,
    command_id: NodeCommandId,
    expected: ExpectedCommand<'_>,
) -> Result<(), String> {
    if command.id != command_id
        || command.node_id != node_id
        || command.aggregate_id != target.replica.id.as_uuid()
        || command.correlation_id != reconciliation_correlation_id(target)
    {
        return Err("Runtime reconciliation command identity changed".into());
    }
    validate_expected_payload(command, expected)
}

fn validate_expected_payload(
    command: &NodeCommand,
    expected: ExpectedCommand<'_>,
) -> Result<(), String> {
    match (expected, &command.payload) {
        (
            ExpectedCommand::Inspect { spec },
            NodeCommandPayload::RuntimeInspect {
                unit_id,
                generation,
            },
        ) if unit_id == &spec.unit_id && *generation == spec.generation => Ok(()),
        (
            ExpectedCommand::Apply {
                spec,
                command_id,
                resource_binding,
            },
            NodeCommandPayload::RuntimeApply {
                request,
                resource_claim,
            },
        ) if request.request_id == format!("workload-reconcile:{command_id}:apply")
            && resource_claim.as_deref() == resource_binding
            && request.spec.digest().map_err(|error| {
                format!("could not digest Runtime recovery specification: {error}")
            })? == spec.digest().map_err(|error| {
                format!("could not digest expected Runtime specification: {error}")
            })? =>
        {
            Ok(())
        }
        _ => Err("Runtime reconciliation command payload changed".into()),
    }
}

fn validate_reconciliation_claim(
    claim: &ResourceClaim,
    target: &ActiveRuntimeTarget,
    spec: &RuntimeUnitSpec,
    node_id: NodeId,
) -> Result<(), String> {
    claim.validate()?;
    if claim.organization_id != target.workload.organization_id
        || claim.deployment_id != target.deployment.id
        || claim.workload_id != target.workload.id
        || claim.replica_id != target.replica.id
        || claim.replica_generation != target.replica.generation
        || claim.member_id != target.member.id
        || claim.placement_generation != target.member.placement_generation
        || claim.node_id != node_id
        || claim.runtime_unit_id != spec.unit_id
        || claim.runtime_generation != spec.generation
    {
        return Err("active Runtime resource claim identity changed".into());
    }
    Ok(())
}

fn validate_resource_observation(
    binding: Option<&NodeResourceClaimBinding>,
    observation: &a3s_runtime::contract::RuntimeObservation,
) -> Result<(), String> {
    if let Some(binding) = binding {
        binding
            .validate_runtime_observation(observation)
            .map_err(|error| {
                format!("Runtime reconciliation allocation binding is inconsistent: {error}")
            })?;
    }
    Ok(())
}

fn checked_add(
    at: DateTime<Utc>,
    duration: chrono::Duration,
    context: &str,
) -> Result<DateTime<Utc>, String> {
    at.checked_add_signed(duration)
        .ok_or_else(|| format!("{context} deadline overflowed"))
}

fn timestamp_millis(value: DateTime<Utc>) -> Result<u64, String> {
    u64::try_from(value.timestamp_millis())
        .map_err(|_| "Runtime reconciliation deadline predates Unix epoch".into())
}

fn repository_error(
    context: &'static str,
) -> impl FnOnce(RepositoryError) -> String + Send + Sync + 'static {
    move |error| format!("{context}: {error}")
}

#[cfg(test)]
mod tests;
