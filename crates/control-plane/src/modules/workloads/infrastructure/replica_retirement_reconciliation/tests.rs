use super::*;
use crate::modules::fleet::domain::repositories::RuntimeObservationRecord;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeId,
    OperationId, OrganizationId, ProjectId, ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, HttpHealthCheck, OciArtifact, ResourceClaimReservation, ResourceKind,
    ResourceSlotRequest, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate, Workload,
    WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::workloads::infrastructure::{
    InMemoryResourceClaimRepository, InMemoryWorkloadRepository,
};
use crate::modules::workloads::{
    CreateDeploymentBundle, DeploymentRequested, IWorkloadReplicaDeploymentRepository,
    IWorkloadReplicaEvacuationRepository, IWorkloadRepository, ReconfigureReplicaSetWrite,
    ReplicaEvacuationCandidate, ReplicaEvacuationRequest, WorkloadControlSpec,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandFailure, NodeResourceClaimPrepare, NodeResourceClaimReleased,
    NodeResourceInventory, NodeResourceSlot, ResourceAllocation, ResourceUnit,
};
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use std::collections::{BTreeMap, HashMap};
use tokio::sync::RwLock;

#[derive(Default)]
struct FakeControl {
    state: RwLock<FakeControlState>,
}

#[derive(Default)]
struct FakeControlState {
    sequence: u64,
    commands: BTreeMap<NodeCommandId, NodeCommand>,
    acknowledgements: HashMap<NodeCommandId, NodeCommandAck>,
}

impl FakeControl {
    async fn commands(&self) -> Vec<NodeCommand> {
        let mut commands = self
            .state
            .read()
            .await
            .commands
            .values()
            .cloned()
            .collect::<Vec<_>>();
        commands.sort_by_key(|command| command.sequence);
        commands
    }

    async fn acknowledge_at(
        &self,
        command: &NodeCommand,
        completed_at: DateTime<Utc>,
        outcome: NodeCommandOutcome,
    ) {
        self.state.write().await.acknowledgements.insert(
            command.id,
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.id.as_uuid(),
                lease_id: Uuid::now_v7(),
                node_id: command.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest().expect("command payload digest"),
                completed_at,
                outcome,
            },
        );
    }
}

#[async_trait]
impl IWorkloadRuntimeControl for FakeControl {
    async fn enqueue_command(
        &self,
        draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(command) = state.commands.get(&draft.proposed_command_id) {
            return Ok(IdempotentWrite {
                value: command.clone(),
                replayed: true,
            });
        }
        state.sequence += 1;
        let command =
            NodeCommand::issue(draft, state.sequence).map_err(RepositoryError::Conflict)?;
        state.commands.insert(command.id, command.clone());
        Ok(IdempotentWrite {
            value: command,
            replayed: false,
        })
    }

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .commands
            .get(&command_id)
            .filter(|command| command.node_id == node_id)
            .cloned())
    }

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .acknowledgements
            .get(&command_id)
            .filter(|acknowledgement| acknowledgement.node_id == node_id.as_uuid())
            .cloned())
    }

    async fn latest_runtime_observation(
        &self,
        _node_id: NodeId,
        _unit_id: &str,
        _generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
        Ok(None)
    }
}

#[tokio::test]
async fn unplaced_retiring_replica_completes_once_without_commands(
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let target = seed_retiring_target(
        repository.as_ref(),
        now - ChronoDuration::minutes(1),
        FixturePlacement::Unplaced,
    )
    .await?;
    let control = Arc::new(FakeControl::default());
    let reconciler = reconciler(
        repository.clone(),
        control.clone(),
        Arc::new(InMemoryResourceClaimRepository::new()),
    )?;

    let report = reconciler.run_once(now).await?;
    assert_eq!(report.targets, 1);
    assert_eq!(report.retired, 1);
    assert_eq!(report.pending, 0);
    assert!(report.failures.is_empty());
    assert!(control.commands().await.is_empty());
    let retired = repository
        .find_workload_replica(
            target.replica.organization_id,
            target.replica.workload_id,
            target.replica.id,
        )
        .await?;
    assert_eq!(retired.lifecycle, WorkloadReplicaLifecycle::Retired);
    assert_eq!(retired.retirement_command_id, None);
    assert_eq!(retired.runtime_fenced_at, None);
    assert!(repository.pending_replica_retirements(10).await?.is_empty());
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.replica.retired")
            .count(),
        1
    );
    assert_eq!(reconciler.run_once(now).await?.targets, 0);
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.replica.retired")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn scheduled_replica_emits_a_removal_fence_before_retirement(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - ChronoDuration::minutes(1);
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let target =
        seed_retiring_target(repository.as_ref(), base, FixturePlacement::Scheduled).await?;
    let control = Arc::new(FakeControl::default());
    let reconciler = reconciler(
        repository.clone(),
        control.clone(),
        Arc::new(InMemoryResourceClaimRepository::new()),
    )?;

    let report = reconciler
        .run_once(base + ChronoDuration::seconds(10))
        .await?;
    assert_eq!(report.removal_commands, 1);
    assert_eq!(report.retired, 0);
    assert_eq!(report.pending, 1);
    assert!(report.failures.is_empty());
    assert_eq!(
        control
            .commands()
            .await
            .iter()
            .filter(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
            .count(),
        1
    );
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        target.member.node_id
    );
    assert_eq!(
        repository
            .find_workload_replica(
                target.replica.organization_id,
                target.replica.workload_id,
                target.replica.id,
            )
            .await?
            .lifecycle,
        WorkloadReplicaLifecycle::Retiring
    );
    Ok(())
}

#[tokio::test]
async fn runtime_fence_precedes_claim_release_and_member_clear(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - ChronoDuration::minutes(1);
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let target =
        seed_retiring_target(repository.as_ref(), base, FixturePlacement::Dispatched).await?;
    assert!(matches!(
        repository
            .complete_replica_retirement(ReplicaRetirementCompletion {
                organization_id: target.replica.organization_id,
                workload_id: target.replica.workload_id,
                replica_id: target.replica.id,
                replica_generation: target.replica.generation,
                expected_replica_version: target.replica.aggregate_version,
                member_id: target.member.id,
                expected_member_version: target.member.aggregate_version,
                fenced_node_id: target.member.node_id,
                completed_at: base + ChronoDuration::seconds(6),
                correlation_id: Uuid::now_v7(),
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        target.member.node_id
    );
    let claims = Arc::new(InMemoryResourceClaimRepository::new());
    let control = Arc::new(FakeControl::default());
    let claim = bound_claim(
        &target,
        claims.as_ref(),
        control.as_ref(),
        target.replica.updated_at,
    )
    .await?;
    let reconciler = reconciler(repository.clone(), control.clone(), claims.clone())?;

    let first = reconciler
        .run_once(base + ChronoDuration::seconds(10))
        .await?;
    assert_eq!(first.removal_commands, 1);
    assert_eq!(first.pending, 1);
    assert_eq!(first.release_commands, 0);
    assert_eq!(
        claims.find(claim.organization_id, claim.id).await?.state,
        ResourceClaimState::BoundToRuntimeUnit
    );
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        target.member.node_id
    );

    let removal = control
        .commands()
        .await
        .into_iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
        .ok_or("Runtime removal command")?;
    let removal_completed_at = base + ChronoDuration::seconds(11);
    control
        .acknowledge_at(
            &removal,
            removal_completed_at,
            successful_runtime_removal(&removal, removal_completed_at)?,
        )
        .await;

    let fenced = reconciler
        .run_once(base + ChronoDuration::seconds(12))
        .await?;
    assert_eq!(fenced.runtime_fences, 1);
    assert_eq!(fenced.release_commands, 1);
    assert_eq!(fenced.pending, 1);
    let replica = repository
        .find_workload_replica(
            target.replica.organization_id,
            target.replica.workload_id,
            target.replica.id,
        )
        .await?;
    assert_eq!(replica.lifecycle, WorkloadReplicaLifecycle::Retiring);
    assert_eq!(replica.retirement_command_id, Some(removal.id));
    assert_eq!(
        replica.runtime_fenced_at,
        Some(canonical_timestamp(removal_completed_at))
    );
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        target.member.node_id
    );
    assert_eq!(
        claims.find(claim.organization_id, claim.id).await?.state,
        ResourceClaimState::Releasing
    );

    let release = control
        .commands()
        .await
        .into_iter()
        .find(|command| {
            matches!(
                command.payload,
                NodeCommandPayload::ResourceClaimRelease { .. }
            )
        })
        .ok_or("Claim release command")?;
    let release_completed_at = base + ChronoDuration::seconds(13);
    control
        .acknowledge_at(
            &release,
            release_completed_at,
            successful_claim_release(&release, release_completed_at)?,
        )
        .await;

    let completed = reconciler
        .run_once(base + ChronoDuration::seconds(14))
        .await?;
    assert_eq!(completed.claims_released, 1);
    assert_eq!(completed.retired, 1);
    assert_eq!(completed.pending, 0);
    assert_eq!(
        claims.find(claim.organization_id, claim.id).await?.state,
        ResourceClaimState::Released
    );
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        None
    );
    assert_eq!(
        repository
            .find_workload_replica(
                target.replica.organization_id,
                target.replica.workload_id,
                target.replica.id,
            )
            .await?
            .lifecycle,
        WorkloadReplicaLifecycle::Retired
    );
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.replica.retired")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn retryable_runtime_removal_rotates_the_command_without_releasing_claim(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - ChronoDuration::minutes(1);
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let target =
        seed_retiring_target(repository.as_ref(), base, FixturePlacement::Dispatched).await?;
    let claims = Arc::new(InMemoryResourceClaimRepository::new());
    let control = Arc::new(FakeControl::default());
    let claim = bound_claim(
        &target,
        claims.as_ref(),
        control.as_ref(),
        target.replica.updated_at,
    )
    .await?;
    let reconciler = reconciler(repository.clone(), control.clone(), claims.clone())?;

    reconciler
        .run_once(base + ChronoDuration::seconds(10))
        .await?;
    let first = control
        .commands()
        .await
        .into_iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
        .ok_or("first Runtime removal")?;
    control
        .acknowledge_at(
            &first,
            base + ChronoDuration::seconds(11),
            NodeCommandOutcome::Failed {
                failure: NodeCommandFailure {
                    code: "provider_unavailable".into(),
                    message: "temporary Runtime outage".into(),
                    retryable: true,
                },
            },
        )
        .await;

    let retried = reconciler
        .run_once(base + ChronoDuration::seconds(12))
        .await?;
    assert_eq!(retried.removal_commands, 1);
    assert_eq!(retried.pending, 1);
    assert_eq!(retried.release_commands, 0);
    let commands = control.commands().await;
    let removals = commands
        .iter()
        .filter(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
        .collect::<Vec<_>>();
    assert_eq!(removals.len(), 2);
    assert_ne!(removals[0].id, removals[1].id);
    assert_eq!(
        claims.find(claim.organization_id, claim.id).await?.state,
        ResourceClaimState::BoundToRuntimeUnit
    );
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        target.member.node_id
    );
    assert_eq!(
        repository
            .find_workload_replica(
                target.replica.organization_id,
                target.replica.workload_id,
                target.replica.id,
            )
            .await?
            .retirement_command_id,
        Some(removals[1].id)
    );
    Ok(())
}

#[tokio::test]
async fn terminal_runtime_removal_failure_keeps_claim_and_placement_fenced(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - ChronoDuration::minutes(1);
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let target =
        seed_retiring_target(repository.as_ref(), base, FixturePlacement::Dispatched).await?;
    let claims = Arc::new(InMemoryResourceClaimRepository::new());
    let control = Arc::new(FakeControl::default());
    let claim = bound_claim(
        &target,
        claims.as_ref(),
        control.as_ref(),
        target.replica.updated_at,
    )
    .await?;
    let reconciler = reconciler(repository.clone(), control.clone(), claims.clone())?;

    reconciler
        .run_once(base + ChronoDuration::seconds(10))
        .await?;
    let removal = control
        .commands()
        .await
        .into_iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
        .ok_or("Runtime removal")?;
    control
        .acknowledge_at(
            &removal,
            base + ChronoDuration::seconds(11),
            NodeCommandOutcome::Rejected {
                failure: NodeCommandFailure {
                    code: "permission_denied".into(),
                    message: "Runtime removal is not authorized".into(),
                    retryable: false,
                },
            },
        )
        .await;

    let failed = reconciler
        .run_once(base + ChronoDuration::seconds(12))
        .await?;
    assert_eq!(failed.failures.len(), 1);
    assert_eq!(failed.release_commands, 0);
    assert_eq!(failed.retired, 0);
    assert_eq!(
        claims.find(claim.organization_id, claim.id).await?.state,
        ResourceClaimState::BoundToRuntimeUnit
    );
    assert_eq!(
        current_member(repository.as_ref(), &target).await?.node_id,
        target.member.node_id
    );
    assert_eq!(
        repository
            .find_workload_replica(
                target.replica.organization_id,
                target.replica.workload_id,
                target.replica.id,
            )
            .await?
            .lifecycle,
        WorkloadReplicaLifecycle::Retiring
    );
    Ok(())
}

#[tokio::test]
async fn evacuation_fences_and_releases_the_old_generation_before_rematerializing(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - ChronoDuration::minutes(1);
    let repository = Arc::new(InMemoryWorkloadRepository::new());
    let target = seed_evacuating_target(repository.as_ref(), base).await?;
    let stable_replica_id = target.replica.id;
    let previous_generation = target.replica.generation;
    let previous_deployment_id = target.deployment.as_ref().ok_or("evacuated deployment")?.id;
    let claims = Arc::new(InMemoryResourceClaimRepository::new());
    let control = Arc::new(FakeControl::default());
    let claim = bound_claim(
        &target,
        claims.as_ref(),
        control.as_ref(),
        target.replica.updated_at,
    )
    .await?;
    let reconciler = reconciler(repository.clone(), control.clone(), claims.clone())?;

    reconciler
        .run_once(base + ChronoDuration::seconds(10))
        .await?;
    let removal = control
        .commands()
        .await
        .into_iter()
        .find(|command| matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }))
        .ok_or("Runtime removal")?;
    let removal_completed_at = base + ChronoDuration::seconds(11);
    control
        .acknowledge_at(
            &removal,
            removal_completed_at,
            successful_runtime_removal(&removal, removal_completed_at)?,
        )
        .await;
    let releasing = reconciler
        .run_once(base + ChronoDuration::seconds(12))
        .await?;
    assert_eq!(releasing.runtime_fences, 1);
    assert_eq!(releasing.release_commands, 1);
    assert_eq!(releasing.evacuated, 0);
    let release = control
        .commands()
        .await
        .into_iter()
        .find(|command| {
            matches!(
                command.payload,
                NodeCommandPayload::ResourceClaimRelease { .. }
            )
        })
        .ok_or("Claim release")?;
    let release_completed_at = base + ChronoDuration::seconds(13);
    control
        .acknowledge_at(
            &release,
            release_completed_at,
            successful_claim_release(&release, release_completed_at)?,
        )
        .await;

    let completed = reconciler
        .run_once(base + ChronoDuration::seconds(14))
        .await?;
    assert_eq!(completed.claims_released, 1);
    assert_eq!(completed.evacuated, 1);
    assert_eq!(completed.retired, 0);
    assert_eq!(
        claims.find(claim.organization_id, claim.id).await?.state,
        ResourceClaimState::Released
    );
    let replica = repository
        .find_workload_replica(
            target.replica.organization_id,
            target.replica.workload_id,
            target.replica.id,
        )
        .await?;
    assert_eq!(replica.id, stable_replica_id);
    assert_eq!(replica.generation, previous_generation + 1);
    assert_eq!(replica.lifecycle, WorkloadReplicaLifecycle::Desired);
    assert_eq!(replica.evacuation_node_id, None);
    let member = current_member(repository.as_ref(), &target).await?;
    assert_eq!(member.node_id, None);
    assert_eq!(member.placement_generation, 1);

    let candidates = repository.pending_replica_deployments(10).await?;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].replica_id, stable_replica_id);
    assert_eq!(candidates[0].replica_generation, previous_generation + 1);
    let materialized = repository
        .materialize_replica_deployment(candidates[0], base + ChronoDuration::seconds(15))
        .await?
        .ok_or("replacement replica deployment")?;
    assert_ne!(materialized.deployment.id, previous_deployment_id);
    let binding = repository
        .find_deployment_replica_binding(target.replica.organization_id, materialized.deployment.id)
        .await?;
    assert_eq!(binding.replica_id, stable_replica_id);
    assert_eq!(binding.replica_generation, previous_generation + 1);
    assert_eq!(binding.placement_generation, 1);
    let events = repository.outbox_events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "workload.replica.evacuation.requested")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_key == "workload.replica.evacuated")
            .count(),
        1
    );
    Ok(())
}

fn reconciler(
    retirements: Arc<dyn IWorkloadReplicaRetirementRepository>,
    control: Arc<dyn IWorkloadRuntimeControl>,
    claims: Arc<dyn IResourceClaimRepository>,
) -> Result<ReplicaRetirementReconciler, String> {
    ReplicaRetirementReconciler::new(
        retirements,
        control,
        claims,
        Duration::from_secs(1),
        Duration::from_secs(60),
        Duration::from_secs(30),
        Duration::from_secs(30),
        100,
    )
}

async fn seed_retiring_target(
    repository: &InMemoryWorkloadRepository,
    at: DateTime<Utc>,
    placement: FixturePlacement,
) -> Result<RetiringReplicaTarget, Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("replica retirement fixture")?,
        at,
    );
    let mut bundle = deployment_bundle(workload.clone(), at)?;
    bundle.control = WorkloadControlSpec::unmanaged_replica_set(1, 2)?;
    repository.create_deployment(bundle).await?;
    if placement != FixturePlacement::Unplaced {
        let candidate = repository
            .pending_replica_deployments(10)
            .await?
            .into_iter()
            .find(|candidate| candidate.replica_ordinal == 1)
            .ok_or("retiring replica deployment candidate")?;
        let materialization = repository
            .materialize_replica_deployment(candidate, at + ChronoDuration::seconds(1))
            .await?
            .ok_or("retiring replica deployment materialization")?;
        let resolving = repository
            .mark_resolving(
                materialization.deployment.id,
                materialization.deployment.aggregate_version,
                at + ChronoDuration::seconds(2),
            )
            .await?;
        let scheduled = repository
            .assign_node(
                resolving.id,
                resolving.aggregate_version,
                NodeId::new(),
                at + ChronoDuration::seconds(3),
            )
            .await?;
        if placement == FixturePlacement::Dispatched {
            repository
                .mark_dispatched(
                    scheduled.id,
                    scheduled.aggregate_version,
                    NodeCommandId::new(),
                    at + ChronoDuration::seconds(4),
                )
                .await?;
        }
    }
    let control = repository
        .find_workload_control(organization_id, workload.id)
        .await?;
    repository
        .reconfigure_replica_set(replica_set_write(
            &control,
            1,
            match placement {
                FixturePlacement::Unplaced => "retire-unplaced-replica",
                FixturePlacement::Scheduled => "retire-scheduled-replica",
                FixturePlacement::Dispatched => "retire-dispatched-replica",
            },
            at + ChronoDuration::seconds(5),
        )?)
        .await?;
    repository
        .pending_replica_retirements(10)
        .await?
        .into_iter()
        .find(|target| target.replica.workload_id == workload.id && target.replica.ordinal == 1)
        .ok_or_else(|| "retiring replica target is missing".into())
}

async fn seed_evacuating_target(
    repository: &InMemoryWorkloadRepository,
    at: DateTime<Utc>,
) -> Result<RetiringReplicaTarget, Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::new();
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("replica evacuation fixture")?,
        at,
    );
    let mut bundle = deployment_bundle(workload.clone(), at)?;
    bundle.control = WorkloadControlSpec::unmanaged_replica_set(1, 1)?;
    let deployment = bundle.deployment.clone();
    repository.create_deployment(bundle).await?;
    let resolving = repository
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            at + ChronoDuration::seconds(1),
        )
        .await?;
    let source_node_id = NodeId::new();
    let scheduled = repository
        .assign_node(
            resolving.id,
            resolving.aggregate_version,
            source_node_id,
            at + ChronoDuration::seconds(2),
        )
        .await?;
    repository
        .mark_dispatched(
            scheduled.id,
            scheduled.aggregate_version,
            NodeCommandId::new(),
            at + ChronoDuration::seconds(3),
        )
        .await?;
    let replica = repository
        .list_workload_replicas(organization_id, workload.id)
        .await?
        .into_iter()
        .next()
        .ok_or("evacuated replica")?;
    let member = repository
        .find_workload_replica_member(
            organization_id,
            replica.id,
            crate::modules::shared_kernel::domain::WorkloadReplicaMemberId::from_uuid(
                replica.id.as_uuid(),
            ),
        )
        .await?;
    let candidate = ReplicaEvacuationCandidate {
        organization_id,
        workload_id: workload.id,
        replica_id: replica.id,
        replica_generation: replica.generation,
        expected_replica_version: replica.aggregate_version,
        member_id: member.id,
        expected_member_version: member.aggregate_version,
        source_node_id,
        placement_generation: member.placement_generation,
    };
    repository
        .request_replica_evacuation(ReplicaEvacuationRequest {
            candidate,
            requested_at: at + ChronoDuration::seconds(4),
            correlation_id: Uuid::now_v7(),
        })
        .await?;
    repository
        .pending_replica_retirements(10)
        .await?
        .into_iter()
        .find(|target| target.replica.id == replica.id)
        .ok_or_else(|| "evacuating replica target is missing".into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FixturePlacement {
    Unplaced,
    Scheduled,
    Dispatched,
}

async fn current_member(
    repository: &InMemoryWorkloadRepository,
    target: &RetiringReplicaTarget,
) -> Result<WorkloadReplicaMember, RepositoryError> {
    repository
        .find_workload_replica_member(
            target.replica.organization_id,
            target.replica.id,
            target.member.id,
        )
        .await
}

async fn bound_claim(
    target: &RetiringReplicaTarget,
    claims: &InMemoryResourceClaimRepository,
    control: &FakeControl,
    at: DateTime<Utc>,
) -> Result<ResourceClaim, Box<dyn std::error::Error>> {
    let deployment = target.deployment.as_ref().ok_or("placed deployment")?;
    let binding = target.replica_binding.as_ref().ok_or("placed binding")?;
    let node_id = target.member.node_id.ok_or("placed node")?;
    let agent_instance_id = Uuid::now_v7();
    let allocation = ResourceAllocation::Scalar {
        amount: 1,
        unit: ResourceUnit::Count,
    };
    let inventory = NodeResourceInventory::new(
        node_id.as_uuid(),
        agent_instance_id,
        1,
        at,
        vec![NodeResourceSlot::new(
            ResourceKind::Accelerator,
            "gpu/retiring-replica",
            allocation.clone(),
        )?],
    )?;
    let reserved = claims
        .reserve(ResourceClaimReservation {
            id: ResourceClaimId::from_uuid(deployment.id.as_uuid()),
            binding: binding.clone(),
            node_id,
            inventory,
            topology_digest: digest('b'),
            slots: vec![ResourceSlotRequest::new(
                ResourceKind::Accelerator,
                "gpu/retiring-replica",
                allocation,
            )?],
            reserved_at: at + ChronoDuration::seconds(2),
        })
        .await?
        .value;
    let node_binding = reserved.node_binding(agent_instance_id)?;
    let prepare = control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id,
            aggregate_id: reserved.id.as_uuid(),
            payload: NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(NodeResourceClaimPrepare {
                    schema: NodeResourceClaimPrepare::SCHEMA.into(),
                    claim_generation: reserved.claim_generation,
                    claim_digest: reserved.claim_digest.clone(),
                    binding: node_binding.clone(),
                }),
            },
            issued_at: at + ChronoDuration::seconds(3),
            not_after: at + ChronoDuration::minutes(5),
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await?
        .value;
    let preparing = claims
        .begin_preparation(
            reserved.organization_id,
            reserved.id,
            reserved.aggregate_version,
            prepare.id,
            prepare.issued_at,
        )
        .await?;
    let binding_digest = node_binding.digest()?;
    let prepared = claims
        .record_prepared(
            preparing.organization_id,
            preparing.id,
            preparing.aggregate_version,
            prepare.id,
            binding_digest.clone(),
            at + ChronoDuration::seconds(4),
        )
        .await?;
    Ok(claims
        .bind(
            prepared.organization_id,
            prepared.id,
            prepared.aggregate_version,
            crate::modules::workloads::domain::entities::ResourceClaimBindingEvidence {
                runtime_unit_id: prepared.runtime_unit_id.clone(),
                runtime_generation: prepared.runtime_generation,
                binding_digest,
                slots: prepared.slot_evidence(),
                observed_at: at + ChronoDuration::seconds(5),
            },
            at + ChronoDuration::seconds(5),
        )
        .await?)
}

fn successful_runtime_removal(
    command: &NodeCommand,
    completed_at: DateTime<Utc>,
) -> Result<NodeCommandOutcome, Box<dyn std::error::Error>> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err("command is not Runtime removal".into());
    };
    Ok(NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::RuntimeRemoved {
            removal: RuntimeRemoval {
                schema: RuntimeRemoval::SCHEMA.into(),
                request_id: request.request_id.clone(),
                unit_id: request.unit_id.clone(),
                generation: request.generation,
                removed_at_ms: u64::try_from(completed_at.timestamp_millis())?,
                already_absent: false,
            },
        }),
    })
}

fn successful_claim_release(
    command: &NodeCommand,
    completed_at: DateTime<Utc>,
) -> Result<NodeCommandOutcome, Box<dyn std::error::Error>> {
    let NodeCommandPayload::ResourceClaimRelease { request } = &command.payload else {
        return Err("command is not Claim release".into());
    };
    Ok(NodeCommandOutcome::Succeeded {
        result: Box::new(NodeCommandResult::ResourceClaimReleased {
            released: NodeResourceClaimReleased::new(request, completed_at)?,
        }),
    })
}

fn deployment_bundle(
    workload: Workload,
    requested_at: DateTime<Utc>,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        service_template(),
        requested_at,
    )?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        requested_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        workload.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new("cloud.deployment", "4")?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "organizationId": workload.organization_id,
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    Ok(CreateDeploymentBundle {
        workload,
        control: WorkloadControlSpec::unmanaged_replica_set(1, 2)?,
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new(
            "test.replica-retirement",
            Uuid::now_v7().to_string(),
            b"replica-retirement-fixture",
        )?,
        event,
    })
}

fn replica_set_write(
    control: &crate::modules::workloads::domain::entities::WorkloadControl,
    desired_replicas: u32,
    idempotency_key: &str,
    requested_at: DateTime<Utc>,
) -> Result<ReconfigureReplicaSetWrite, Box<dyn std::error::Error>> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "organizationId": control.organization_id,
        "workloadId": control.workload_id,
        "expectedPolicyGeneration": control.spec.placement_policy.generation(),
        "desiredReplicas": desired_replicas,
    }))?;
    Ok(ReconfigureReplicaSetWrite {
        organization_id: control.organization_id,
        workload_id: control.workload_id,
        expected_control_version: control.aggregate_version,
        expected_policy_generation: control.spec.placement_policy.generation(),
        desired_replicas,
        managed_owner: control.spec.managed_owner.clone(),
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{}/workloads/{}/replica-set",
                control.organization_id, control.workload_id
            ),
            idempotency_key,
            &canonical,
        )?,
        correlation_id: Uuid::now_v7(),
        requested_at,
    })
}

fn service_template() -> ServiceTemplate {
    let digest = digest('a');
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/retirement@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: Some(HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 1_000,
        }),
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
