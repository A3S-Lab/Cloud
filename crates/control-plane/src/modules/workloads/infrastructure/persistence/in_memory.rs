use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadPlacementGroupId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentPlacementGroupBinding, DeploymentReplicaBinding, DeploymentStatus,
    OciArtifact, Workload, WorkloadControl, WorkloadPlacementGroup, WorkloadReplica,
    WorkloadReplicaLifecycle, WorkloadReplicaMember, WorkloadRevision, WorkloadWriterFenceReceipt,
};
use crate::modules::workloads::domain::events::{
    WorkloadDeploymentHealthChanged, WorkloadReplicaEvacuated, WorkloadReplicaEvacuationRequested,
    WorkloadReplicaRetired, WorkloadReplicaSetReconfigured,
};
use crate::modules::workloads::domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle,
    IWorkloadPlacementGroupRepository, IWorkloadPlacementGroupSchedulingRepository,
    IWorkloadReplicaDeploymentRepository, IWorkloadReplicaEvacuationRepository,
    IWorkloadReplicaRetirementRepository, IWorkloadRepository, IWorkloadRuntimeTargetRepository,
    IWorkloadWriterFenceRepository, PlacementGroupCancellationWrite, PlacementGroupMaterialization,
    PlacementGroupPlacement, PlacementGroupSchedulingWrite, ReconfigureReplicaSetWrite,
    ReplicaDeploymentCandidate, ReplicaDeploymentMaterialization, ReplicaEvacuationCandidate,
    ReplicaEvacuationRequest, ReplicaRetirementCompletion, ReplicaRetirementDispatch,
    ReplicaRuntimeFence, ReplicaSetWriteResult, RequestDeploymentCancellationBundle,
    RequestWorkloadStopBundle, RetiringReplicaTarget, WorkloadStopBundle,
    WorkloadWriterFenceCommit,
};
use crate::modules::workloads::domain::services::{
    plan_replica_set_reconfiguration, ReplicaSetReconfigurationError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

use crate::modules::workloads::infrastructure::replica_deployment_materialization::{
    build_group_deployment_write, build_replica_deployment_write, created_materialization,
    materialization_from_existing, validate_existing_group_materialization_context,
    validate_existing_materialization, PlacementGroupDeploymentContext,
};

#[derive(Default)]
pub struct InMemoryWorkloadRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    workloads: BTreeMap<WorkloadId, Workload>,
    names: BTreeMap<
        (
            OrganizationId,
            crate::modules::shared_kernel::domain::EnvironmentId,
            String,
        ),
        WorkloadId,
    >,
    revisions: BTreeMap<WorkloadRevisionId, WorkloadRevision>,
    deployments: BTreeMap<DeploymentId, Deployment>,
    controls: BTreeMap<WorkloadId, WorkloadControl>,
    replicas: BTreeMap<WorkloadReplicaId, WorkloadReplica>,
    replica_members: BTreeMap<WorkloadReplicaMemberId, WorkloadReplicaMember>,
    placement_groups: BTreeMap<WorkloadPlacementGroupId, WorkloadPlacementGroup>,
    deployment_replica_bindings: BTreeMap<DeploymentId, DeploymentReplicaBinding>,
    deployment_replica_member_bindings:
        BTreeMap<(DeploymentId, WorkloadReplicaMemberId), DeploymentReplicaBinding>,
    deployment_placement_group_bindings: BTreeMap<DeploymentId, DeploymentPlacementGroupBinding>,
    idempotency: BTreeMap<(String, String), (String, DeploymentBundle)>,
    cancellation_idempotency: BTreeMap<(String, String), (String, Deployment)>,
    stop_idempotency: BTreeMap<(String, String), (String, WorkloadStopBundle)>,
    replica_set_idempotency: BTreeMap<(String, String), (String, ReplicaSetWriteResult)>,
    writer_fences: BTreeMap<(WorkloadId, u64), WorkloadWriterFenceReceipt>,
    writer_fence_operations: BTreeMap<OperationId, OperationRequest>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

#[async_trait]
impl IWorkloadPlacementGroupRepository for InMemoryWorkloadRepository {
    async fn materialize_placement_group(
        &self,
        write: crate::modules::workloads::domain::entities::WorkloadPlacementGroupWrite,
    ) -> Result<PlacementGroupMaterialization, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(existing) = state.placement_groups.get(&write.group.id) {
            if !existing.same_plan(&write.group) {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let replica_members = current_placement_group_members(&state, existing)?;
            return Ok(PlacementGroupMaterialization {
                group: existing.clone(),
                replica_members,
                replayed: true,
            });
        }
        if state.placement_groups.values().any(|candidate| {
            candidate.organization_id == write.group.organization_id
                && candidate.replica_id == write.group.replica_id
                && candidate.replica_generation == write.group.replica_generation
        }) {
            return Err(RepositoryError::IdempotencyConflict);
        }

        let workload = state
            .workloads
            .get(&write.group.workload_id)
            .ok_or(RepositoryError::NotFound)?;
        let control = state
            .controls
            .get(&write.group.workload_id)
            .ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?;
        let revision = state
            .revisions
            .get(&write.group.revision_id)
            .ok_or_else(|| {
                RepositoryError::Storage("placement-group revision is missing".into())
            })?;
        let replica = state
            .replicas
            .get(&write.group.replica_id)
            .ok_or_else(|| RepositoryError::Storage("placement-group replica is missing".into()))?;
        control
            .validate_against(workload)
            .map_err(RepositoryError::Storage)?;
        let policy = &control.spec.placement_policy;
        write
            .group
            .validate_context(workload, policy, revision, replica)
            .map_err(|_| {
                RepositoryError::Conflict(
                    "Workload placement-group plan is stale or inconsistent".into(),
                )
            })?;

        let mut materialized_members = Vec::with_capacity(write.replica_members.len());
        let mut missing_members = Vec::new();
        for member in &write.replica_members {
            if let Some(existing) = state.replica_members.get(&member.id) {
                write
                    .group
                    .validate_available_replica_member(existing)
                    .map_err(RepositoryError::Conflict)?;
                materialized_members.push(existing.clone());
            } else {
                missing_members.push(member.clone());
                materialized_members.push(member.clone());
            }
        }
        for member in missing_members {
            state.replica_members.insert(member.id, member);
        }
        state
            .placement_groups
            .insert(write.group.id, write.group.clone());
        Ok(PlacementGroupMaterialization {
            group: write.group,
            replica_members: materialized_members,
            replayed: false,
        })
    }

    async fn find_placement_group(
        &self,
        organization_id: OrganizationId,
        group_id: WorkloadPlacementGroupId,
    ) -> Result<WorkloadPlacementGroup, RepositoryError> {
        self.state
            .read()
            .await
            .placement_groups
            .get(&group_id)
            .filter(|group| group.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn find_placement_group_for_replica_generation(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
        replica_generation: u64,
    ) -> Result<WorkloadPlacementGroup, RepositoryError> {
        self.state
            .read()
            .await
            .placement_groups
            .values()
            .find(|group| {
                group.organization_id == organization_id
                    && group.replica_id == replica_id
                    && group.replica_generation == replica_generation
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }
}

fn current_placement_group_members(
    state: &State,
    group: &WorkloadPlacementGroup,
) -> Result<Vec<WorkloadReplicaMember>, RepositoryError> {
    group
        .members
        .iter()
        .map(|planned| {
            let member = state
                .replica_members
                .get(&planned.member_id)
                .filter(|member| member.ordinal == planned.ordinal)
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Workload placement-group plan references a missing member".into(),
                    )
                })?;
            group
                .validate_replica_member_identity(member)
                .map_err(RepositoryError::Storage)?;
            Ok(member.clone())
        })
        .collect()
}

impl InMemoryWorkloadRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    #[cfg(test)]
    pub(crate) async fn writer_fence_operation(
        &self,
        operation_id: OperationId,
    ) -> Option<OperationRequest> {
        self.state
            .read()
            .await
            .writer_fence_operations
            .get(&operation_id)
            .cloned()
    }

    #[cfg(test)]
    pub(crate) async fn seed_placement_group_foundation(
        &self,
        workload: Workload,
        spec: crate::modules::workloads::domain::entities::WorkloadControlSpec,
        revision: WorkloadRevision,
    ) -> Result<WorkloadReplica, RepositoryError> {
        if spec.placement_policy.topology()
            != crate::modules::workloads::domain::entities::PlacementTopology::MultiNode
            || spec.placement_policy.desired_replicas() != 1
            || revision.workload_id != workload.id
        {
            return Err(RepositoryError::Conflict(
                "placement-group fixture foundation is invalid".into(),
            ));
        }
        let control =
            WorkloadControl::create(&workload, spec).map_err(RepositoryError::Conflict)?;
        let replica =
            WorkloadReplica::canonical(&workload, &revision).map_err(RepositoryError::Conflict)?;
        let member = WorkloadReplicaMember::canonical(&workload, &replica)
            .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if state.workloads.contains_key(&workload.id)
            || state.revisions.contains_key(&revision.id)
            || state.replicas.contains_key(&replica.id)
            || state.replica_members.contains_key(&member.id)
        {
            return Err(RepositoryError::Conflict(
                "placement-group fixture identity is already in use".into(),
            ));
        }
        state.names.insert(
            (
                workload.organization_id,
                workload.environment_id,
                workload.name.key().to_owned(),
            ),
            workload.id,
        );
        state.controls.insert(workload.id, control);
        state.revisions.insert(revision.id, revision);
        state.replica_members.insert(member.id, member);
        state.replicas.insert(replica.id, replica.clone());
        state.workloads.insert(workload.id, workload);
        Ok(replica)
    }
}

#[async_trait]
impl IWorkloadRepository for InMemoryWorkloadRepository {
    async fn create_deployment(
        &self,
        request: CreateDeploymentBundle,
    ) -> Result<DeploymentBundle, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (
            request.idempotency.scope.clone(),
            request.idempotency.key.clone(),
        );
        if let Some((digest, response)) = state.idempotency.get(&key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let mut response = response.clone();
            response.replayed = true;
            return Ok(response);
        }
        validate_bundle(&request)?;
        let desired_replicas = request.control.placement_policy.desired_replicas();
        if desired_replicas == 0 {
            return Err(RepositoryError::Conflict(
                "a deployment cannot be created for a scale-to-zero Workload".into(),
            ));
        }
        let is_new_workload = !state.workloads.contains_key(&request.workload.id);
        let (workload, control, replicas, members, control_changed) = if let Some(existing) =
            state.workloads.get(&request.workload.id)
        {
            if existing != &request.workload {
                return Err(RepositoryError::Conflict(
                    "workload changed before a new revision was requested".into(),
                ));
            }
            if state.deployments.values().any(|deployment| {
                deployment.workload_id == existing.id && !deployment.status.is_terminal()
            }) {
                return Err(RepositoryError::Conflict(
                    "workload already has a nonterminal deployment".into(),
                ));
            }
            let mut control = state.controls.get(&existing.id).cloned().ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?;
            let control_changed = control
                .authorize_deployment(&request.control, request.revision.created_at)
                .map_err(RepositoryError::Conflict)?;
            let replica_id = WorkloadReplica::deterministic_id(existing.id, 0)
                .map_err(RepositoryError::Storage)?;
            let mut replica = state.replicas.get(&replica_id).cloned().ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its canonical replica".into())
            })?;
            replica
                .advance(&request.revision, request.revision.created_at)
                .map_err(RepositoryError::Conflict)?;
            let member_id = WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid());
            let member = state
                .replica_members
                .get(&member_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Workload is missing its canonical replica member".into(),
                    )
                })?;
            (
                existing.clone(),
                control,
                vec![replica],
                vec![member],
                control_changed,
            )
        } else {
            let name_key = (
                request.workload.organization_id,
                request.workload.environment_id,
                request.workload.name.key().to_owned(),
            );
            if state.names.contains_key(&name_key) {
                return Err(RepositoryError::Conflict(
                    "workload name is already in use".into(),
                ));
            }
            let control = WorkloadControl::create(&request.workload, request.control.clone())
                .map_err(RepositoryError::Conflict)?;
            let mut replicas = Vec::with_capacity(desired_replicas as usize);
            let mut members = Vec::with_capacity(desired_replicas as usize);
            for ordinal in 0..desired_replicas {
                let replica =
                    WorkloadReplica::for_ordinal(&request.workload, &request.revision, ordinal)
                        .map_err(RepositoryError::Conflict)?;
                let member = WorkloadReplicaMember::for_replica(&request.workload, &replica)
                    .map_err(RepositoryError::Conflict)?;
                members.push(member);
                replicas.push(replica);
            }
            (request.workload.clone(), control, replicas, members, false)
        };
        let next_generation = state
            .revisions
            .values()
            .filter(|revision| revision.workload_id == workload.id)
            .map(|revision| revision.generation)
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| RepositoryError::Storage("workload generation overflowed".into()))?;
        if request.revision.generation != next_generation {
            return Err(RepositoryError::Conflict(format!(
                "workload revision generation must be {next_generation}"
            )));
        }
        if state.revisions.contains_key(&request.revision.id)
            || state.deployments.contains_key(&request.deployment.id)
            || state
                .deployment_replica_bindings
                .contains_key(&request.deployment.id)
        {
            return Err(RepositoryError::Conflict(
                "workload revision or deployment identity is already in use".into(),
            ));
        }
        let replica = replicas
            .first()
            .ok_or_else(|| RepositoryError::Storage("Workload replica set is empty".into()))?;
        let member = members.first().ok_or_else(|| {
            RepositoryError::Storage("Workload replica member set is empty".into())
        })?;
        let binding = DeploymentReplicaBinding::create(
            &request.deployment,
            &request.revision,
            replica,
            member,
        )
        .map_err(RepositoryError::Conflict)?;
        if is_new_workload {
            state.names.insert(
                (
                    request.workload.organization_id,
                    request.workload.environment_id,
                    request.workload.name.key().to_owned(),
                ),
                request.workload.id,
            );
            state
                .workloads
                .insert(request.workload.id, request.workload.clone());
            state.controls.insert(request.workload.id, control);
            for member in members {
                state.replica_members.insert(member.id, member);
            }
        } else if control_changed {
            state.controls.insert(request.workload.id, control);
        }
        for replica in replicas {
            state.replicas.insert(replica.id, replica);
        }
        state
            .revisions
            .insert(request.revision.id, request.revision.clone());
        state
            .deployments
            .insert(request.deployment.id, request.deployment.clone());
        state
            .deployment_replica_bindings
            .insert(request.deployment.id, binding.clone());
        state
            .deployment_replica_member_bindings
            .insert((request.deployment.id, binding.member_id), binding);
        state.outbox.push(request.event);
        let response = DeploymentBundle {
            workload,
            revision: request.revision,
            deployment: request.deployment,
            operation: request.operation,
            replayed: false,
        };
        state
            .idempotency
            .insert(key, (request.idempotency.request_digest, response.clone()));
        Ok(response)
    }

    async fn replay_deployment(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DeploymentBundle>, RepositoryError> {
        let state = self.state.read().await;
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        let Some((digest, bundle)) = state.idempotency.get(&key) else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(bundle.clone()))
    }

    async fn request_deployment_cancellation(
        &self,
        request: RequestDeploymentCancellationBundle,
    ) -> Result<crate::modules::shared_kernel::domain::IdempotentWrite<Deployment>, RepositoryError>
    {
        let mut state = self.state.write().await;
        let key = (
            request.idempotency.scope.clone(),
            request.idempotency.key.clone(),
        );
        if let Some((digest, deployment)) = state.cancellation_idempotency.get(&key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(crate::modules::shared_kernel::domain::IdempotentWrite {
                value: deployment.clone(),
                replayed: true,
            });
        }
        let current = state
            .deployments
            .get(&request.deployment.id)
            .ok_or(RepositoryError::NotFound)?;
        state
            .controls
            .get(&current.workload_id)
            .ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?
            .require_direct_mutation()
            .map_err(RepositoryError::Conflict)?;
        validate_cancellation_bundle(&request, current)?;
        state
            .deployments
            .insert(request.deployment.id, request.deployment.clone());
        state.outbox.push(request.event);
        state.cancellation_idempotency.insert(
            key,
            (
                request.idempotency.request_digest,
                request.deployment.clone(),
            ),
        );
        Ok(crate::modules::shared_kernel::domain::IdempotentWrite {
            value: request.deployment,
            replayed: false,
        })
    }

    async fn replay_deployment_cancellation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<Deployment>, RepositoryError> {
        let state = self.state.read().await;
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        let Some((digest, deployment)) = state.cancellation_idempotency.get(&key) else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(deployment.clone()))
    }

    async fn request_workload_stop(
        &self,
        request: RequestWorkloadStopBundle,
    ) -> Result<WorkloadStopBundle, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (
            request.idempotency.scope.clone(),
            request.idempotency.key.clone(),
        );
        if let Some((digest, response)) = state.stop_idempotency.get(&key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let mut response = response.clone();
            response.replayed = true;
            return Ok(response);
        }
        let current = state
            .workloads
            .get(&request.workload.id)
            .filter(|workload| workload.organization_id == request.workload.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        state
            .controls
            .get(&current.id)
            .ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?
            .require_direct_mutation()
            .map_err(RepositoryError::Conflict)?;
        if current.aggregate_version != request.expected_version {
            return Err(RepositoryError::Conflict(format!(
                "workload changed from expected version {} to {}",
                request.expected_version, current.aggregate_version
            )));
        }
        let mut expected = current;
        expected
            .request_stop(request.workload.updated_at)
            .map_err(RepositoryError::Conflict)?;
        if expected != request.workload
            || request.operation.organization_id != request.workload.organization_id
            || request.operation.subject.kind() != "workload"
            || request.operation.subject.id() != request.workload.id.as_uuid()
            || request.operation.requested_at < request.workload.updated_at
            || request.event.organization_id != request.workload.organization_id.as_uuid()
            || request.event.aggregate_id != request.workload.id.as_uuid()
            || request.event.aggregate_version != request.workload.aggregate_version
        {
            return Err(RepositoryError::Conflict(
                "workload stop bundle is inconsistent with stored state".into(),
            ));
        }
        state
            .workloads
            .insert(request.workload.id, request.workload.clone());
        state.outbox.push(request.event);
        let response = WorkloadStopBundle {
            workload: request.workload,
            operation: request.operation,
            replayed: false,
        };
        state
            .stop_idempotency
            .insert(key, (request.idempotency.request_digest, response.clone()));
        Ok(response)
    }

    async fn complete_workload_stop(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        expected_version: u64,
        stopped_at: DateTime<Utc>,
    ) -> Result<Workload, RepositoryError> {
        let mut state = self.state.write().await;
        let current = state
            .workloads
            .get(&workload_id)
            .filter(|workload| workload.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if current.aggregate_version != expected_version {
            if current.desired_state
                == crate::modules::workloads::domain::entities::WorkloadDesiredState::Stopped
                && current.active_revision_id.is_none()
            {
                return Ok(current);
            }
            return Err(RepositoryError::Conflict(format!(
                "workload changed from expected version {expected_version} to {}",
                current.aggregate_version
            )));
        }
        let mut workload = current;
        workload
            .complete_stop(stopped_at)
            .map_err(RepositoryError::Conflict)?;
        state.workloads.insert(workload_id, workload.clone());
        Ok(workload)
    }

    async fn reconfigure_replica_set(
        &self,
        write: ReconfigureReplicaSetWrite,
    ) -> Result<ReplicaSetWriteResult, RepositoryError> {
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.scope.clone(),
            write.idempotency.key.clone(),
        );
        if let Some((digest, response)) = state.replica_set_idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let mut response = response.clone();
            response.replayed = true;
            return Ok(response);
        }

        let workload = state_workload(&state, write.organization_id, write.workload_id)?;
        let current_control = state
            .controls
            .get(&write.workload_id)
            .filter(|control| control.organization_id == write.organization_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?;
        let mut current_replicas = state
            .replicas
            .values()
            .filter(|replica| {
                replica.organization_id == write.organization_id
                    && replica.workload_id == write.workload_id
            })
            .cloned()
            .collect::<Vec<_>>();
        current_replicas.sort_by_key(|replica| (replica.ordinal, replica.id));
        let canonical = current_replicas
            .iter()
            .find(|replica| replica.ordinal == 0)
            .ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its canonical replica".into())
            })?;
        let revision = state
            .revisions
            .get(&canonical.revision_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Workload canonical replica references a missing revision".into(),
                )
            })?;
        let reconfiguration = plan_replica_set_reconfiguration(
            &workload,
            current_control.clone(),
            &revision,
            current_replicas,
            write.expected_control_version,
            write.expected_policy_generation,
            write.desired_replicas,
            write.managed_owner.as_ref(),
            write.requested_at,
        )
        .map_err(replica_set_repository_error)?;
        for member in &reconfiguration.members_to_create {
            if state.replica_members.contains_key(&member.id) {
                return Err(RepositoryError::Storage(
                    "new Workload replica member identity already exists".into(),
                ));
            }
        }
        let event = WorkloadReplicaSetReconfigured::envelope(
            &current_control,
            &reconfiguration.control,
            write.correlation_id,
        )
        .map_err(RepositoryError::Storage)?;

        for member in reconfiguration.members_to_create {
            state.replica_members.insert(member.id, member);
        }
        for replica in &reconfiguration.replicas {
            state.replicas.insert(replica.id, replica.clone());
        }
        state.controls.insert(
            reconfiguration.control.workload_id,
            reconfiguration.control.clone(),
        );
        state.outbox.push(event);
        let response = ReplicaSetWriteResult {
            control: reconfiguration.control,
            replicas: reconfiguration.replicas,
            replayed: false,
        };
        state.replica_set_idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, response.clone()),
        );
        Ok(response)
    }

    async fn find_workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Workload, RepositoryError> {
        let state = self.state.read().await;
        state_workload(&state, organization_id, workload_id)
    }

    async fn find_workload_control(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<WorkloadControl, RepositoryError> {
        self.state
            .read()
            .await
            .controls
            .get(&workload_id)
            .filter(|control| control.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn find_workload_replica(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        replica_id: WorkloadReplicaId,
    ) -> Result<WorkloadReplica, RepositoryError> {
        self.state
            .read()
            .await
            .replicas
            .get(&replica_id)
            .filter(|replica| {
                replica.organization_id == organization_id && replica.workload_id == workload_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_workload_replicas(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<WorkloadReplica>, RepositoryError> {
        let state = self.state.read().await;
        let mut replicas = state
            .replicas
            .values()
            .filter(|replica| {
                replica.organization_id == organization_id && replica.workload_id == workload_id
            })
            .cloned()
            .collect::<Vec<_>>();
        replicas.sort_by_key(|replica| (replica.ordinal, replica.id));
        Ok(replicas)
    }

    async fn find_workload_replica_member(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
        member_id: WorkloadReplicaMemberId,
    ) -> Result<WorkloadReplicaMember, RepositoryError> {
        self.state
            .read()
            .await
            .replica_members
            .get(&member_id)
            .filter(|member| {
                member.organization_id == organization_id && member.replica_id == replica_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_workload_replica_members(
        &self,
        organization_id: OrganizationId,
        replica_id: WorkloadReplicaId,
    ) -> Result<Vec<WorkloadReplicaMember>, RepositoryError> {
        let state = self.state.read().await;
        let mut members = state
            .replica_members
            .values()
            .filter(|member| {
                member.organization_id == organization_id && member.replica_id == replica_id
            })
            .cloned()
            .collect::<Vec<_>>();
        members.sort_by_key(|member| (member.ordinal, member.id));
        Ok(members)
    }

    async fn find_deployment_replica_binding(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<DeploymentReplicaBinding, RepositoryError> {
        self.state
            .read()
            .await
            .deployment_replica_bindings
            .get(&deployment_id)
            .filter(|binding| binding.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_deployment_replica_member_bindings(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Vec<DeploymentReplicaBinding>, RepositoryError> {
        let state = self.state.read().await;
        let mut bindings = state
            .deployment_replica_member_bindings
            .values()
            .filter(|binding| {
                binding.organization_id == organization_id && binding.deployment_id == deployment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        bindings.sort_by_key(|binding| {
            state
                .replica_members
                .get(&binding.member_id)
                .map_or((u32::MAX, binding.member_id), |member| {
                    (member.ordinal, member.id)
                })
        });
        Ok(bindings)
    }

    async fn find_deployment_placement_group_binding(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<DeploymentPlacementGroupBinding, RepositoryError> {
        self.state
            .read()
            .await
            .deployment_placement_group_bindings
            .get(&deployment_id)
            .filter(|binding| binding.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_workloads(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Workload>, RepositoryError> {
        let mut workloads = self
            .state
            .read()
            .await
            .workloads
            .values()
            .filter(|workload| {
                workload.organization_id == organization_id
                    && workload.project_id == project_id
                    && workload.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        workloads.sort_by(|left, right| {
            left.name
                .key()
                .cmp(right.name.key())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(workloads)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        revision_id: WorkloadRevisionId,
    ) -> Result<WorkloadRevision, RepositoryError> {
        let state = self.state.read().await;
        let revision = state
            .revisions
            .get(&revision_id)
            .ok_or(RepositoryError::NotFound)?;
        state_workload(&state, organization_id, revision.workload_id)?;
        Ok(revision.clone())
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<WorkloadRevision>, RepositoryError> {
        let state = self.state.read().await;
        state_workload(&state, organization_id, workload_id)?;
        let mut revisions = state
            .revisions
            .values()
            .filter(|revision| revision.workload_id == workload_id)
            .cloned()
            .collect::<Vec<_>>();
        revisions.sort_by_key(|revision| std::cmp::Reverse((revision.generation, revision.id)));
        Ok(revisions)
    }

    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        revision_id: WorkloadRevisionId,
        artifact: OciArtifact,
        resolved_at: DateTime<Utc>,
    ) -> Result<WorkloadRevision, RepositoryError> {
        let mut state = self.state.write().await;
        let workload_id = state
            .revisions
            .get(&revision_id)
            .map(|revision| revision.workload_id)
            .ok_or(RepositoryError::NotFound)?;
        state_workload(&state, organization_id, workload_id)?;
        let revision = state
            .revisions
            .get_mut(&revision_id)
            .ok_or(RepositoryError::NotFound)?;
        revision.resolve(artifact, resolved_at).map_err(|error| {
            RepositoryError::Conflict(format!(
                "workload revision resolution was rejected: {error}"
            ))
        })?;
        Ok(revision.clone())
    }

    async fn find_deployment(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Deployment, RepositoryError> {
        self.state
            .read()
            .await
            .deployments
            .get(&deployment_id)
            .filter(|deployment| deployment.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_deployments(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Vec<Deployment>, RepositoryError> {
        let state = self.state.read().await;
        state_workload(&state, organization_id, workload_id)?;
        let mut deployments = state
            .deployments
            .values()
            .filter(|deployment| {
                deployment.organization_id == organization_id
                    && deployment.workload_id == workload_id
            })
            .cloned()
            .collect::<Vec<_>>();
        deployments
            .sort_by_key(|deployment| std::cmp::Reverse((deployment.requested_at, deployment.id)));
        Ok(deployments)
    }

    async fn mark_resolving(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate_desired(&self.state, deployment_id, expected_version, |deployment| {
            deployment.resolve(at)
        })
        .await
    }

    async fn assign_node(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        node_id: NodeId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        let mut state = self.state.write().await;
        let mut deployment = state
            .deployments
            .get(&deployment_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if deployment.aggregate_version != expected_version {
            return Err(version_conflict(
                expected_version,
                deployment.aggregate_version,
            ));
        }
        require_current_desired_replica(&state, &deployment)?;
        deployment.schedule(node_id, at).map_err(transition_error)?;
        let mut binding = state
            .deployment_replica_bindings
            .get(&deployment_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "deployment is missing its canonical replica binding".into(),
                )
            })?;
        let mut member = state
            .replica_members
            .get(&binding.member_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "deployment replica binding references a missing member".into(),
                )
            })?;
        member.place(node_id, at).map_err(transition_error)?;
        binding
            .assign(&deployment, &member)
            .map_err(transition_error)?;
        state.replica_members.insert(member.id, member);
        state
            .deployment_replica_bindings
            .insert(deployment_id, binding.clone());
        state
            .deployment_replica_member_bindings
            .insert((deployment_id, binding.member_id), binding);
        state.deployments.insert(deployment_id, deployment.clone());
        Ok(deployment)
    }

    async fn mark_dispatched(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate_desired(&self.state, deployment_id, expected_version, |deployment| {
            deployment.dispatch(command_id, at)
        })
        .await
    }

    async fn mark_verifying(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate_desired(&self.state, deployment_id, expected_version, |deployment| {
            deployment.verify(at)
        })
        .await
    }

    async fn activate(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        retirement_required: bool,
        at: DateTime<Utc>,
    ) -> Result<(Workload, Deployment), RepositoryError> {
        let mut state = self.state.write().await;
        let (workload_id, revision_id) = {
            let deployment = state
                .deployments
                .get(&deployment_id)
                .ok_or(RepositoryError::NotFound)?;
            if deployment.aggregate_version != expected_version {
                if matches!(
                    deployment.status,
                    crate::modules::workloads::domain::entities::DeploymentStatus::Retiring
                        | crate::modules::workloads::domain::entities::DeploymentStatus::Active
                ) {
                    let mut replay = deployment.clone();
                    replay
                        .activate(retirement_required, at)
                        .map_err(transition_error)?;
                    let workload = state
                        .workloads
                        .get(&deployment.workload_id)
                        .ok_or(RepositoryError::NotFound)?;
                    if workload.active_revision_id == Some(deployment.revision_id) {
                        return Ok((workload.clone(), deployment.clone()));
                    }
                }
                return Err(version_conflict(
                    expected_version,
                    deployment.aggregate_version,
                ));
            }
            (deployment.workload_id, deployment.revision_id)
        };
        let mut deployment = state
            .deployments
            .get(&deployment_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let previous_deployment = deployment.clone();
        require_current_desired_replica(&state, &deployment)?;
        let mut workload = state
            .workloads
            .get(&workload_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let previous_workload = workload.clone();
        let at = if workload.active_revision_id == Some(revision_id) {
            at.max(workload.updated_at)
        } else {
            at
        };
        deployment
            .activate(retirement_required, at)
            .map_err(transition_error)?;
        workload
            .activate(revision_id, at)
            .map_err(transition_error)?;
        let revision = state.revisions.get(&revision_id).ok_or_else(|| {
            RepositoryError::Storage("activated Workload revision disappeared".into())
        })?;
        let health_event = WorkloadDeploymentHealthChanged::healthy_envelope(
            &previous_deployment,
            &deployment,
            &previous_workload,
            &workload,
            revision,
        )
        .map_err(RepositoryError::Storage)?;
        state.deployments.insert(deployment_id, deployment.clone());
        state.workloads.insert(workload_id, workload.clone());
        if let Some(event) = health_event {
            state.outbox.push(event);
        }
        Ok((workload, deployment))
    }

    async fn dispatch_retirement(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.dispatch_retirement(command_id, at)
        })
        .await
    }

    async fn complete_retirement(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.complete_retirement(at)
        })
        .await
    }

    async fn fail(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        reason: String,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        let mut state = self.state.write().await;
        let mut deployment = state
            .deployments
            .get(&deployment_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if matches!(
            deployment.status,
            DeploymentStatus::Failed | DeploymentStatus::Orphaned
        ) && deployment.failure.as_ref() == Some(&reason)
        {
            return Ok(deployment);
        }
        if deployment.aggregate_version != expected_version {
            return Err(version_conflict(
                expected_version,
                deployment.aggregate_version,
            ));
        }
        let previous = deployment.clone();
        deployment.fail(reason, at).map_err(transition_error)?;
        let workload = state
            .workloads
            .get(&deployment.workload_id)
            .ok_or_else(|| {
                RepositoryError::Storage("Workload health-fact owner disappeared".into())
            })?;
        let revision = state
            .revisions
            .get(&deployment.revision_id)
            .ok_or_else(|| {
                RepositoryError::Storage("Workload health-fact revision disappeared".into())
            })?;
        let health_event = WorkloadDeploymentHealthChanged::failure_envelope(
            &previous,
            &deployment,
            workload,
            revision,
        )
        .map_err(RepositoryError::Storage)?;
        state.deployments.insert(deployment_id, deployment.clone());
        if let Some(event) = health_event {
            state.outbox.push(event);
        }
        Ok(deployment)
    }

    async fn mark_cancellation_requested(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.request_cancellation(at)
        })
        .await
    }

    async fn begin_cleanup(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.begin_cleanup(command_id, at)
        })
        .await
    }

    async fn retry_cleanup(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.retry_cleanup(command_id, at)
        })
        .await
    }

    async fn cancel(
        &self,
        deployment_id: DeploymentId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<Deployment, RepositoryError> {
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.cancel(at)
        })
        .await
    }
}

#[async_trait]
impl IWorkloadPlacementGroupSchedulingRepository for InMemoryWorkloadRepository {
    async fn schedule_placement_group(
        &self,
        write: PlacementGroupSchedulingWrite,
    ) -> Result<IdempotentWrite<PlacementGroupPlacement>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let mut deployment = state
            .deployments
            .get(&write.deployment_id)
            .filter(|deployment| deployment.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let group_binding = state
            .deployment_placement_group_bindings
            .get(&deployment.id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "placement-group Deployment is missing its group binding".into(),
                )
            })?;
        if group_binding.group_id != write.group_id
            || group_binding.group_plan_digest != write.group_plan_digest
            || usize::try_from(group_binding.member_count).ok() != Some(write.placements.len())
        {
            return Err(RepositoryError::Conflict(
                "placement-group scheduling write changed the immutable plan".into(),
            ));
        }
        let group = state
            .placement_groups
            .get(&write.group_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("placement-group plan is missing".into()))?;
        if group.id != write.group_id
            || group.plan_digest != write.group_plan_digest
            || group.members.len() != write.placements.len()
        {
            return Err(RepositoryError::Conflict(
                "placement-group scheduling write changed the immutable plan".into(),
            ));
        }
        let mut bindings = group
            .members
            .iter()
            .map(|plan| {
                state
                    .deployment_replica_member_bindings
                    .get(&(deployment.id, plan.member_id))
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "placement-group Deployment member binding is missing".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let stored_members = group
            .members
            .iter()
            .map(|plan| {
                state
                    .replica_members
                    .get(&plan.member_id)
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage("placement-group member is missing".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        if deployment.status == DeploymentStatus::Scheduled {
            if deployment.node_id != write.placements.first().map(|placement| placement.node_id)
                || bindings.len() != write.placements.len()
                || bindings
                    .iter()
                    .zip(&write.placements)
                    .zip(&stored_members)
                    .any(|((binding, placement), member)| {
                        binding.member_id != placement.member_id
                            || binding.node_id != Some(placement.node_id)
                            || member.id != placement.member_id
                            || member.ordinal != placement.ordinal
                            || member.node_id != Some(placement.node_id)
                            || member.placement_generation != binding.placement_generation
                    })
            {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: PlacementGroupPlacement {
                    deployment,
                    member_bindings: bindings,
                },
                replayed: true,
            });
        }
        if deployment.status != DeploymentStatus::Resolving {
            return Err(RepositoryError::Conflict(format!(
                "placement-group Deployment cannot schedule from {}",
                deployment.status.as_str()
            )));
        }
        if deployment.aggregate_version != write.expected_deployment_version {
            return Err(version_conflict(
                write.expected_deployment_version,
                deployment.aggregate_version,
            ));
        }
        require_current_desired_replica(&state, &deployment)?;
        let workload = state
            .workloads
            .get(&deployment.workload_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("placement-group Workload is missing".into())
            })?;
        let control = state
            .controls
            .get(&deployment.workload_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("placement-group Workload control is missing".into())
            })?;
        let revision = state
            .revisions
            .get(&deployment.revision_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("placement-group Workload revision is missing".into())
            })?;
        let replica = state
            .replicas
            .get(&group.replica_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("placement-group replica is missing".into()))?;
        let mut members = stored_members;
        validate_existing_group_materialization_context(
            &deployment,
            PlacementGroupDeploymentContext {
                workload: &workload,
                policy: &control.spec.placement_policy,
                revision: &revision,
                replica: &replica,
                group: &group,
                members: &members,
            },
            &bindings,
            &group_binding,
        )
        .map_err(RepositoryError::Conflict)?;

        let previous_deployment_version = deployment.aggregate_version;
        let leader_node_id = write
            .placements
            .first()
            .map(|placement| placement.node_id)
            .ok_or_else(|| RepositoryError::Conflict("placement-group leader is missing".into()))?;
        deployment
            .schedule(leader_node_id, write.scheduled_at)
            .map_err(RepositoryError::Conflict)?;
        debug_assert!(deployment.aggregate_version > previous_deployment_version);
        for (((plan, placement), member), binding) in group
            .members
            .iter()
            .zip(&write.placements)
            .zip(&mut members)
            .zip(&mut bindings)
        {
            if plan.ordinal != placement.ordinal || plan.member_id != placement.member_id {
                return Err(RepositoryError::Conflict(
                    "placement-group scheduling member changed the immutable plan".into(),
                ));
            }
            member
                .place(placement.node_id, write.scheduled_at)
                .map_err(RepositoryError::Conflict)?;
            binding
                .assign_placement_group_member(&deployment, member, plan)
                .map_err(RepositoryError::Conflict)?;
        }

        for (member, binding) in members.into_iter().zip(&bindings) {
            state.replica_members.insert(member.id, member);
            state
                .deployment_replica_member_bindings
                .insert((deployment.id, binding.member_id), binding.clone());
        }
        let leader_binding = bindings.first().cloned().ok_or_else(|| {
            RepositoryError::Storage("placement-group leader binding is missing".into())
        })?;
        state
            .deployment_replica_bindings
            .insert(deployment.id, leader_binding);
        state.deployments.insert(deployment.id, deployment.clone());
        Ok(IdempotentWrite {
            value: PlacementGroupPlacement {
                deployment,
                member_bindings: bindings,
            },
            replayed: false,
        })
    }

    async fn cancel_placement_group(
        &self,
        write: PlacementGroupCancellationWrite,
    ) -> Result<IdempotentWrite<PlacementGroupPlacement>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let mut deployment = state
            .deployments
            .get(&write.deployment_id)
            .filter(|deployment| deployment.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let group_binding = state
            .deployment_placement_group_bindings
            .get(&deployment.id)
            .filter(|binding| {
                binding.group_id == write.group_id
                    && binding.group_plan_digest == write.group_plan_digest
            })
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "placement-group cancellation changed the immutable plan".into(),
                )
            })?;
        let group = state
            .placement_groups
            .get(&group_binding.group_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("placement-group plan is missing".into()))?;
        if group.id != write.group_id
            || group.plan_digest != write.group_plan_digest
            || usize::try_from(group_binding.member_count).ok() != Some(group.members.len())
        {
            return Err(RepositoryError::Conflict(
                "placement-group cancellation changed the immutable plan".into(),
            ));
        }
        let bindings = group
            .members
            .iter()
            .map(|plan| {
                state
                    .deployment_replica_member_bindings
                    .get(&(deployment.id, plan.member_id))
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "placement-group Deployment member binding is missing".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let stored_members = group
            .members
            .iter()
            .map(|plan| {
                state
                    .replica_members
                    .get(&plan.member_id)
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage("placement-group member is missing".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if deployment.status == DeploymentStatus::Cancelled {
            if bindings
                .iter()
                .zip(&stored_members)
                .any(|(binding, member)| {
                    binding.member_id != member.id
                        || member.node_id.is_some()
                        || binding.placement_generation != member.placement_generation
                })
            {
                return Err(RepositoryError::Storage(
                    "cancelled placement-group member state is inconsistent".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: PlacementGroupPlacement {
                    deployment,
                    member_bindings: bindings,
                },
                replayed: true,
            });
        }
        if deployment.status != DeploymentStatus::Cancelling
            || deployment.command_id.is_some()
            || deployment.cleanup_command_id.is_some()
            || deployment.aggregate_version != write.expected_deployment_version
        {
            return Err(RepositoryError::Conflict(
                "placement-group cancellation is not safe before Agent preparation".into(),
            ));
        }
        let mut members = Vec::with_capacity(group.members.len());
        for ((plan, binding), stored_member) in
            group.members.iter().zip(&bindings).zip(stored_members)
        {
            let mut member = stored_member;
            if plan.member_id != binding.member_id || plan.member_id != member.id {
                return Err(RepositoryError::Storage(
                    "placement-group cancellation member is inconsistent".into(),
                ));
            }
            match binding.node_id {
                Some(node_id) => member
                    .release_after_fencing(node_id, write.cancelled_at)
                    .map_err(RepositoryError::Conflict)?,
                None if member.node_id.is_none() => {}
                None => {
                    return Err(RepositoryError::Conflict(
                        "unassigned placement-group binding has a placed member".into(),
                    ))
                }
            }
            members.push(member);
        }
        deployment
            .cancel(write.cancelled_at)
            .map_err(RepositoryError::Conflict)?;
        for member in members {
            state.replica_members.insert(member.id, member);
        }
        state.deployments.insert(deployment.id, deployment.clone());
        Ok(IdempotentWrite {
            value: PlacementGroupPlacement {
                deployment,
                member_bindings: bindings,
            },
            replayed: false,
        })
    }
}

#[async_trait]
impl IWorkloadReplicaDeploymentRepository for InMemoryWorkloadRepository {
    async fn pending_replica_deployments(
        &self,
        limit: usize,
    ) -> Result<Vec<ReplicaDeploymentCandidate>, RepositoryError> {
        if limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "replica deployment candidate limit must be between 1 and 10000".into(),
            ));
        }
        let state = self.state.read().await;
        let mut candidates = state
            .replicas
            .values()
            .filter_map(|replica| {
                let workload = state.workloads.get(&replica.workload_id)?;
                let control = state.controls.get(&replica.workload_id)?;
                let already_bound = state.deployment_replica_bindings.values().any(|binding| {
                    binding.replica_id == replica.id
                        && binding.replica_generation == replica.generation
                });
                let execution_shape_ready = match control.spec.placement_policy.topology() {
                    crate::modules::workloads::domain::entities::PlacementTopology::SingleNode => {
                        control.spec.placement_policy.members_per_replica() == 1
                    }
                    crate::modules::workloads::domain::entities::PlacementTopology::MultiNode => {
                        state.placement_groups.values().any(|group| {
                            group.organization_id == replica.organization_id
                                && group.workload_id == replica.workload_id
                                && group.replica_id == replica.id
                                && group.revision_id == replica.revision_id
                                && group.revision_generation == replica.revision_generation
                                && group.replica_generation == replica.generation
                                && group.policy_generation
                                    == control.spec.placement_policy.generation()
                                && group.placement_policy_digest
                                    == control.spec.placement_policy.digest()
                                && group.members.len()
                                    == control.spec.placement_policy.members_per_replica() as usize
                        })
                    }
                };
                if replica.lifecycle != WorkloadReplicaLifecycle::Desired
                    || workload.desired_state
                        != crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
                    || workload
                        .active_revision_id
                        .is_some_and(|active| active != replica.revision_id)
                    || replica.ordinal >= control.spec.placement_policy.desired_replicas()
                    || !execution_shape_ready
                    || already_bound
                {
                    return None;
                }
                Some((
                    replica.updated_at,
                    ReplicaDeploymentCandidate {
                        organization_id: replica.organization_id,
                        workload_id: replica.workload_id,
                        replica_id: replica.id,
                        replica_ordinal: replica.ordinal,
                        revision_id: replica.revision_id,
                        revision_generation: replica.revision_generation,
                        replica_generation: replica.generation,
                    },
                ))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(updated_at, candidate)| {
            (
                *updated_at,
                candidate.workload_id,
                candidate.replica_ordinal,
                candidate.replica_id,
            )
        });
        Ok(candidates
            .into_iter()
            .take(limit)
            .map(|(_, candidate)| candidate)
            .collect())
    }

    async fn materialize_replica_deployment(
        &self,
        candidate: ReplicaDeploymentCandidate,
        requested_at: DateTime<Utc>,
    ) -> Result<Option<ReplicaDeploymentMaterialization>, RepositoryError> {
        let mut state = self.state.write().await;
        let workload = state
            .workloads
            .get(&candidate.workload_id)
            .filter(|workload| workload.organization_id == candidate.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let control = state
            .controls
            .get(&candidate.workload_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?;
        control
            .validate_against(&workload)
            .map_err(RepositoryError::Storage)?;
        let replica = state
            .replicas
            .get(&candidate.replica_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("replica deployment candidate is missing".into())
            })?;
        if replica.workload_id != candidate.workload_id
            || replica.ordinal != candidate.replica_ordinal
            || replica.revision_id != candidate.revision_id
            || replica.revision_generation != candidate.revision_generation
            || replica.generation != candidate.replica_generation
            || replica.lifecycle != WorkloadReplicaLifecycle::Desired
            || replica.ordinal >= control.spec.placement_policy.desired_replicas()
            || workload.desired_state
                != crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
            || workload
                .active_revision_id
                .is_some_and(|active| active != replica.revision_id)
        {
            return Ok(None);
        }
        let revision = state
            .revisions
            .get(&candidate.revision_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("replica deployment revision is missing".into())
            })?;
        let topology = control.spec.placement_policy.topology();
        let placement_group = match topology {
            crate::modules::workloads::domain::entities::PlacementTopology::SingleNode => None,
            crate::modules::workloads::domain::entities::PlacementTopology::MultiNode => {
                let Some(group) = state.placement_groups.values().find(|group| {
                    group.organization_id == candidate.organization_id
                        && group.replica_id == replica.id
                        && group.replica_generation == replica.generation
                }) else {
                    return Ok(None);
                };
                if group
                    .validate_context(
                        &workload,
                        &control.spec.placement_policy,
                        &revision,
                        &replica,
                    )
                    .is_err()
                {
                    return Ok(None);
                }
                Some(group.clone())
            }
        };
        if let Some(binding) = state.deployment_replica_bindings.values().find(|binding| {
            binding.replica_id == replica.id && binding.replica_generation == replica.generation
        }) {
            let deployment = state
                .deployments
                .get(&binding.deployment_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "replica generation binding references a missing deployment".into(),
                    )
                })?;
            let mut member_bindings = state
                .deployment_replica_member_bindings
                .values()
                .filter(|member_binding| member_binding.deployment_id == deployment.id)
                .cloned()
                .collect::<Vec<_>>();
            member_bindings.sort_by_key(|member_binding| {
                state
                    .replica_members
                    .get(&member_binding.member_id)
                    .map_or((u32::MAX, member_binding.member_id), |member| {
                        (member.ordinal, member.id)
                    })
            });
            let placement_group_binding = state
                .deployment_placement_group_bindings
                .get(&deployment.id)
                .cloned();
            validate_existing_materialization(
                &deployment,
                binding,
                &member_bindings,
                placement_group_binding.as_ref(),
                topology,
            )
            .map_err(RepositoryError::Storage)?;
            if let (Some(group), Some(group_binding)) =
                (placement_group.as_ref(), placement_group_binding.as_ref())
            {
                let members = current_placement_group_members(&state, group)?;
                validate_existing_group_materialization_context(
                    &deployment,
                    PlacementGroupDeploymentContext {
                        workload: &workload,
                        policy: &control.spec.placement_policy,
                        revision: &revision,
                        replica: &replica,
                        group,
                        members: &members,
                    },
                    &member_bindings,
                    group_binding,
                )
                .map_err(RepositoryError::Storage)?;
            }
            return materialization_from_existing(
                candidate,
                deployment,
                member_bindings,
                placement_group_binding,
            )
            .map(Some)
            .map_err(RepositoryError::Storage);
        }
        let write = match topology {
            crate::modules::workloads::domain::entities::PlacementTopology::SingleNode => {
                let member_id = WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid());
                let member = state
                    .replica_members
                    .get(&member_id)
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage("replica deployment member is missing".into())
                    })?;
                build_replica_deployment_write(
                    candidate,
                    &workload,
                    &revision,
                    &replica,
                    &member,
                    requested_at,
                )
                .map_err(RepositoryError::Conflict)?
            }
            crate::modules::workloads::domain::entities::PlacementTopology::MultiNode => {
                let group = placement_group.as_ref().ok_or_else(|| {
                    RepositoryError::Storage("placement-group Deployment plan disappeared".into())
                })?;
                let members = group
                    .members
                    .iter()
                    .map(|planned| {
                        state
                            .replica_members
                            .get(&planned.member_id)
                            .filter(|member| member.ordinal == planned.ordinal)
                            .cloned()
                            .ok_or_else(|| {
                                RepositoryError::Storage(
                                    "placement-group Deployment member is missing or inconsistent"
                                        .into(),
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                build_group_deployment_write(
                    candidate,
                    PlacementGroupDeploymentContext {
                        workload: &workload,
                        policy: &control.spec.placement_policy,
                        revision: &revision,
                        replica: &replica,
                        group,
                        members: &members,
                    },
                    requested_at,
                )
                .map_err(RepositoryError::Conflict)?
            }
        };
        if state.deployments.contains_key(&write.deployment.id)
            || state
                .deployment_replica_bindings
                .contains_key(&write.deployment.id)
            || state
                .deployment_placement_group_bindings
                .contains_key(&write.deployment.id)
        {
            return Err(RepositoryError::Storage(
                "deterministic replica deployment identity is already bound inconsistently".into(),
            ));
        }
        state
            .deployments
            .insert(write.deployment.id, write.deployment.clone());
        state
            .deployment_replica_bindings
            .insert(write.deployment.id, write.binding.clone());
        for binding in &write.member_bindings {
            state
                .deployment_replica_member_bindings
                .insert((write.deployment.id, binding.member_id), binding.clone());
        }
        if let Some(binding) = &write.placement_group_binding {
            state
                .deployment_placement_group_bindings
                .insert(write.deployment.id, binding.clone());
        }
        state.outbox.push(write.event.clone());
        Ok(Some(created_materialization(candidate, write)))
    }
}

fn replica_set_repository_error(error: ReplicaSetReconfigurationError) -> RepositoryError {
    match error {
        ReplicaSetReconfigurationError::Conflict(message) => RepositoryError::Conflict(message),
        ReplicaSetReconfigurationError::Invariant(message) => RepositoryError::Storage(message),
    }
}

#[async_trait]
impl IWorkloadReplicaEvacuationRepository for InMemoryWorkloadRepository {
    async fn has_replica_placements(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<bool, RepositoryError> {
        if node_id.as_uuid().is_nil() {
            return Err(RepositoryError::Conflict(
                "replica placement node is invalid".into(),
            ));
        }
        Ok(self
            .state
            .read()
            .await
            .replica_members
            .values()
            .any(|member| {
                member.organization_id == organization_id && member.node_id == Some(node_id)
            }))
    }

    async fn pending_replica_evacuations(
        &self,
        organization_id: OrganizationId,
        source_node_id: NodeId,
        limit: usize,
    ) -> Result<Vec<ReplicaEvacuationCandidate>, RepositoryError> {
        if source_node_id.as_uuid().is_nil() || limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "replica evacuation query is invalid".into(),
            ));
        }
        let state = self.state.read().await;
        let mut candidates = state
            .replicas
            .values()
            .filter(|replica| {
                replica.organization_id == organization_id
                    && replica.lifecycle == WorkloadReplicaLifecycle::Desired
                    && state
                        .workloads
                        .get(&replica.workload_id)
                        .is_some_and(|workload| {
                            workload.desired_state
                                == crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
                        })
                    && state
                        .controls
                        .get(&replica.workload_id)
                        .is_some_and(|control| {
                            replica.ordinal < control.spec.placement_policy.desired_replicas()
                        })
            })
            .filter_map(|replica| {
                let member_id = WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid());
                let member = state.replica_members.get(&member_id)?;
                if member.node_id != Some(source_node_id) {
                    return None;
                }
                let binding = state.deployment_replica_bindings.values().find(|binding| {
                    binding.organization_id == organization_id
                        && binding.replica_id == replica.id
                        && binding.replica_generation == replica.generation
                        && binding.member_id == member.id
                        && binding.node_id == Some(source_node_id)
                        && binding.placement_generation == member.placement_generation
                })?;
                Some((
                    replica.updated_at,
                    ReplicaEvacuationCandidate {
                        organization_id,
                        workload_id: replica.workload_id,
                        replica_id: replica.id,
                        replica_generation: replica.generation,
                        expected_replica_version: replica.aggregate_version,
                        member_id: member.id,
                        expected_member_version: member.aggregate_version,
                        source_node_id,
                        placement_generation: binding.placement_generation,
                    },
                ))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(updated_at, candidate)| {
            (*updated_at, candidate.workload_id, candidate.replica_id)
        });
        Ok(candidates
            .into_iter()
            .take(limit)
            .map(|(_, candidate)| candidate)
            .collect())
    }

    async fn request_replica_evacuation(
        &self,
        request: ReplicaEvacuationRequest,
    ) -> Result<IdempotentWrite<WorkloadReplica>, RepositoryError> {
        let mut state = self.state.write().await;
        let candidate = request.candidate;
        let workload = state
            .workloads
            .get(&candidate.workload_id)
            .filter(|workload| workload.organization_id == candidate.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let control = state
            .controls
            .get(&candidate.workload_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("evacuated Workload has no control record".into())
            })?;
        let mut replica = state
            .replicas
            .get(&candidate.replica_id)
            .filter(|replica| {
                replica.organization_id == candidate.organization_id
                    && replica.workload_id == candidate.workload_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let member = state
            .replica_members
            .get(&candidate.member_id)
            .filter(|member| member.replica_id == candidate.replica_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("evacuated replica member is missing".into())
            })?;
        if replica.generation == candidate.replica_generation
            && replica.lifecycle == WorkloadReplicaLifecycle::Retiring
            && replica.evacuation_node_id == Some(candidate.source_node_id)
        {
            return Ok(IdempotentWrite {
                value: replica,
                replayed: true,
            });
        }
        if workload.desired_state
            != crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
            || replica.generation != candidate.replica_generation
            || replica.aggregate_version != candidate.expected_replica_version
            || replica.lifecycle != WorkloadReplicaLifecycle::Desired
            || replica.ordinal >= control.spec.placement_policy.desired_replicas()
            || member.aggregate_version != candidate.expected_member_version
            || member.node_id != Some(candidate.source_node_id)
            || member.placement_generation != candidate.placement_generation
            || !state.deployment_replica_bindings.values().any(|binding| {
                binding.organization_id == candidate.organization_id
                    && binding.workload_id == candidate.workload_id
                    && binding.replica_id == candidate.replica_id
                    && binding.replica_generation == candidate.replica_generation
                    && binding.member_id == candidate.member_id
                    && binding.node_id == Some(candidate.source_node_id)
                    && binding.placement_generation == candidate.placement_generation
            })
        {
            return Err(RepositoryError::Conflict(
                "Workload replica evacuation candidate changed".into(),
            ));
        }
        let previous = replica.clone();
        replica
            .request_evacuation(&member, candidate.source_node_id, request.requested_at)
            .map_err(RepositoryError::Conflict)?;
        let event = WorkloadReplicaEvacuationRequested::envelope(
            &previous,
            &replica,
            &member,
            request.correlation_id,
        )
        .map_err(RepositoryError::Storage)?;
        state.replicas.insert(replica.id, replica.clone());
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: replica,
            replayed: false,
        })
    }
}

#[async_trait]
impl IWorkloadReplicaRetirementRepository for InMemoryWorkloadRepository {
    async fn pending_replica_retirements(
        &self,
        limit: usize,
    ) -> Result<Vec<RetiringReplicaTarget>, RepositoryError> {
        if limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "replica retirement target limit must be between 1 and 10000".into(),
            ));
        }
        let state = self.state.read().await;
        let mut replicas = state
            .replicas
            .values()
            .filter(|replica| replica.lifecycle == WorkloadReplicaLifecycle::Retiring)
            .cloned()
            .collect::<Vec<_>>();
        replicas.sort_by_key(|replica| {
            (
                replica.updated_at,
                replica.workload_id,
                replica.ordinal,
                replica.id,
            )
        });
        let mut targets = Vec::with_capacity(limit.min(replicas.len()));
        for replica in replicas.into_iter().take(limit) {
            let revision = state
                .revisions
                .get(&replica.revision_id)
                .filter(|revision| revision.workload_id == replica.workload_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "retiring replica references a missing revision".into(),
                    )
                })?;
            let member_id = WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid());
            let member = state
                .replica_members
                .get(&member_id)
                .filter(|member| member.replica_id == replica.id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "retiring replica references a missing canonical member".into(),
                    )
                })?;
            let replica_binding = state
                .deployment_replica_bindings
                .values()
                .find(|binding| {
                    binding.replica_id == replica.id
                        && binding.replica_generation == replica.generation
                })
                .cloned();
            let deployment = replica_binding
                .as_ref()
                .map(|binding| {
                    state
                        .deployments
                        .get(&binding.deployment_id)
                        .cloned()
                        .ok_or_else(|| {
                            RepositoryError::Storage(
                                "retiring replica binding references a missing deployment".into(),
                            )
                        })
                })
                .transpose()?;
            if member.node_id.is_some() && replica_binding.is_none() {
                return Err(RepositoryError::Storage(
                    "placed retiring replica has no deployment binding".into(),
                ));
            }
            targets.push(RetiringReplicaTarget {
                revision,
                replica,
                member,
                deployment,
                replica_binding,
            });
        }
        Ok(targets)
    }

    async fn dispatch_replica_retirement(
        &self,
        dispatch: ReplicaRetirementDispatch,
    ) -> Result<WorkloadReplica, RepositoryError> {
        let mut state = self.state.write().await;
        let mut replica = state
            .replicas
            .get(&dispatch.replica_id)
            .filter(|replica| {
                replica.organization_id == dispatch.organization_id
                    && replica.workload_id == dispatch.workload_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if replica.generation != dispatch.replica_generation {
            return Err(RepositoryError::Conflict(
                "replica retirement dispatch changed generation".into(),
            ));
        }
        if replica.lifecycle == WorkloadReplicaLifecycle::Retiring
            && replica.retirement_command_id == Some(dispatch.command_id)
        {
            return Ok(replica);
        }
        require_replica_version(&replica, dispatch.expected_replica_version)?;
        replica
            .dispatch_retirement(dispatch.command_id, dispatch.dispatched_at)
            .map_err(RepositoryError::Conflict)?;
        state.replicas.insert(replica.id, replica.clone());
        Ok(replica)
    }

    async fn record_replica_runtime_fenced(
        &self,
        fence: ReplicaRuntimeFence,
        writer_fence: Option<WorkloadWriterFenceCommit>,
    ) -> Result<WorkloadReplica, RepositoryError> {
        let mut state = self.state.write().await;
        let mut replica = state
            .replicas
            .get(&fence.replica_id)
            .filter(|replica| {
                replica.organization_id == fence.organization_id
                    && replica.workload_id == fence.workload_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if replica.generation != fence.replica_generation {
            return Err(RepositoryError::Conflict(
                "replica Runtime fence changed generation".into(),
            ));
        }
        let replayed = replica.lifecycle == WorkloadReplicaLifecycle::Retiring
            && replica.retirement_command_id == Some(fence.command_id)
            && replica.runtime_fenced_at == Some(canonical_timestamp(fence.fenced_at));
        let mut existing_writer_fence = None;
        if let Some(commit) = &writer_fence {
            let spec = commit.receipt.spec();
            let control = state.controls.get(&fence.workload_id).ok_or_else(|| {
                RepositoryError::Storage("writer-fenced Workload control is missing".into())
            })?;
            let member = state.replica_members.get(&spec.member_id).ok_or_else(|| {
                RepositoryError::Storage("writer-fenced Workload replica member is missing".into())
            })?;
            let binding = state
                .deployment_replica_member_bindings
                .values()
                .find(|binding| {
                    binding.workload_id == fence.workload_id
                        && binding.replica_id == fence.replica_id
                        && binding.replica_generation == fence.replica_generation
                        && binding.member_id == spec.member_id
                })
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "writer-fenced Workload replica binding is missing".into(),
                    )
                })?;
            commit
                .validate_replica_retirement(&fence, control, &replica, member, binding)
                .map_err(RepositoryError::Conflict)?;
            existing_writer_fence = state
                .writer_fences
                .get(&(fence.workload_id, fence.replica_generation))
                .cloned();
            if existing_writer_fence
                .as_ref()
                .is_some_and(|existing| existing != &commit.receipt)
            {
                return Err(RepositoryError::Conflict(
                    "Workload writer-fence receipt replay changed its exact evidence".into(),
                ));
            }
            if existing_writer_fence.is_some() && !replayed {
                return Err(RepositoryError::Storage(
                    "Workload writer-fence receipt exists before its Runtime fence".into(),
                ));
            }
            if let Some(existing) = state.writer_fence_operations.get(&commit.operation.id) {
                if !existing.has_same_definition(&commit.operation)
                    || existing.requested_at != commit.operation.requested_at
                {
                    return Err(RepositoryError::Conflict(
                        "Workload writer-fence Operation replay changed its definition".into(),
                    ));
                }
            }
        }
        if !replayed {
            require_replica_version(&replica, fence.expected_replica_version)?;
            replica
                .record_runtime_fenced(fence.command_id, fence.fenced_at)
                .map_err(RepositoryError::Conflict)?;
            state.replicas.insert(replica.id, replica.clone());
        }
        if existing_writer_fence.is_none() {
            if let Some(commit) = writer_fence {
                state
                    .writer_fence_operations
                    .insert(commit.operation.id, commit.operation);
                state.writer_fences.insert(
                    (fence.workload_id, fence.replica_generation),
                    commit.receipt,
                );
            }
        }
        Ok(replica)
    }

    async fn complete_replica_retirement(
        &self,
        completion: ReplicaRetirementCompletion,
    ) -> Result<
        crate::modules::shared_kernel::domain::IdempotentWrite<WorkloadReplica>,
        RepositoryError,
    > {
        let mut state = self.state.write().await;
        let mut replica = state
            .replicas
            .get(&completion.replica_id)
            .filter(|replica| {
                replica.organization_id == completion.organization_id
                    && replica.workload_id == completion.workload_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let mut member = state
            .replica_members
            .get(&completion.member_id)
            .filter(|member| member.replica_id == completion.replica_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("retiring replica member is missing".into()))?;
        if completion.replica_generation.checked_add(1) == Some(replica.generation)
            && replica.lifecycle == WorkloadReplicaLifecycle::Desired
            && replica.evacuation_node_id.is_none()
        {
            return Ok(IdempotentWrite {
                value: replica,
                replayed: true,
            });
        }
        if replica.generation != completion.replica_generation {
            return Err(RepositoryError::Conflict(
                "replica retirement completion changed generation".into(),
            ));
        }
        if replica.lifecycle == WorkloadReplicaLifecycle::Retired && member.node_id.is_none() {
            return Ok(crate::modules::shared_kernel::domain::IdempotentWrite {
                value: replica,
                replayed: true,
            });
        }
        require_replica_version(&replica, completion.expected_replica_version)?;
        if member.aggregate_version != completion.expected_member_version {
            return Err(RepositoryError::Conflict(format!(
                "Workload replica member changed from expected version {} to {}",
                completion.expected_member_version, member.aggregate_version
            )));
        }
        let control = state.controls.get(&replica.workload_id).ok_or_else(|| {
            RepositoryError::Storage("retiring replica Workload has no control record".into())
        })?;
        let evacuation = replica.evacuation_node_id.is_some();
        let remains_desired = replica.ordinal < control.spec.placement_policy.desired_replicas();
        if replica.lifecycle != WorkloadReplicaLifecycle::Retiring
            || evacuation != remains_desired
            || member.node_id != completion.fenced_node_id
            || replica
                .evacuation_node_id
                .is_some_and(|source| Some(source) != completion.fenced_node_id)
        {
            return Err(RepositoryError::Conflict(
                "Workload replica is no longer eligible for generation retirement completion"
                    .into(),
            ));
        }
        let dispatched_runtime = state
            .deployment_replica_bindings
            .values()
            .find(|binding| {
                binding.organization_id == completion.organization_id
                    && binding.workload_id == completion.workload_id
                    && binding.replica_id == completion.replica_id
                    && binding.replica_generation == completion.replica_generation
            })
            .map(|binding| {
                state
                    .deployments
                    .get(&binding.deployment_id)
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "retiring replica binding references a missing deployment".into(),
                        )
                    })
                    .map(|deployment| deployment.command_id.is_some())
            })
            .transpose()?
            .unwrap_or(false);
        if (dispatched_runtime || completion.fenced_node_id.is_some())
            && replica.runtime_fenced_at.is_none()
        {
            return Err(RepositoryError::Conflict(
                "Workload replica Runtime is not durably fenced".into(),
            ));
        }
        let previous_replica = replica.clone();
        let previous_member = member.clone();
        if let Some(node_id) = completion.fenced_node_id {
            member
                .release_after_fencing(node_id, completion.completed_at)
                .map_err(RepositoryError::Conflict)?;
        }
        let event = if evacuation {
            replica
                .complete_evacuation(&member, completion.completed_at)
                .map_err(RepositoryError::Conflict)?;
            WorkloadReplicaEvacuated::envelope(
                &previous_replica,
                &replica,
                &previous_member,
                &member,
                completion.correlation_id,
            )
            .map_err(RepositoryError::Storage)?
        } else {
            replica
                .complete_retirement(&member, completion.completed_at)
                .map_err(RepositoryError::Conflict)?;
            WorkloadReplicaRetired::envelope(
                &previous_replica,
                &replica,
                &previous_member,
                &member,
                completion.correlation_id,
            )
            .map_err(RepositoryError::Storage)?
        };
        state.replica_members.insert(member.id, member);
        state.replicas.insert(replica.id, replica.clone());
        state.outbox.push(event);
        Ok(crate::modules::shared_kernel::domain::IdempotentWrite {
            value: replica,
            replayed: false,
        })
    }
}

#[async_trait]
impl IWorkloadWriterFenceRepository for InMemoryWorkloadRepository {
    async fn latest_writer_fence(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Option<WorkloadWriterFenceReceipt>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .writer_fences
            .range((workload_id, 0)..=(workload_id, u64::MAX))
            .rev()
            .map(|(_, receipt)| receipt)
            .find(|receipt| receipt.spec().organization_id == organization_id)
            .cloned())
    }
}

#[async_trait]
impl IWorkloadRuntimeTargetRepository for InMemoryWorkloadRepository {
    async fn list_active_runtime_targets(
        &self,
        limit: usize,
    ) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError> {
        if limit == 0 || limit > 10_000 {
            return Err(RepositoryError::Conflict(
                "active Runtime target limit must be between 1 and 10000".into(),
            ));
        }
        let state = self.state.read().await;
        let mut deployments = state
            .deployments
            .values()
            .filter(|deployment| {
                state
                    .workloads
                    .get(&deployment.workload_id)
                    .is_some_and(|workload| {
                        workload.desired_state
                            == crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
                            && workload.active_revision_id == Some(deployment.revision_id)
                            && matches!(
                                deployment.status,
                                crate::modules::workloads::domain::entities::DeploymentStatus::Retiring
                                    | crate::modules::workloads::domain::entities::DeploymentStatus::Active
                            )
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        deployments.sort_by_key(|deployment| {
            state
                .workloads
                .get(&deployment.workload_id)
                .map(|workload| (workload.updated_at, workload.id, deployment.id))
                .unwrap_or((deployment.updated_at, deployment.workload_id, deployment.id))
        });

        let mut targets = Vec::with_capacity(limit.min(deployments.len()));
        for deployment in deployments {
            let workload = state
                .workloads
                .get(&deployment.workload_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target references a missing Workload".into(),
                    )
                })?;
            let revision = state
                .revisions
                .get(&deployment.revision_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target references a missing revision".into(),
                    )
                })?;
            let replica_binding = state
                .deployment_replica_bindings
                .get(&deployment.id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target is missing its replica binding".into(),
                    )
                })?;
            let replica = state
                .replicas
                .get(&replica_binding.replica_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target references a missing replica".into(),
                    )
                })?;
            if replica.lifecycle != WorkloadReplicaLifecycle::Desired
                || replica.revision_id != replica_binding.revision_id
                || replica.generation != replica_binding.replica_generation
            {
                continue;
            }
            let member = state
                .replica_members
                .get(&replica_binding.member_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target references a missing replica member".into(),
                    )
                })?;
            targets.push(ActiveRuntimeTarget {
                workload,
                revision,
                replica,
                member,
                deployment,
                replica_binding,
            });
            if targets.len() == limit {
                break;
            }
        }
        Ok(targets)
    }
}

fn validate_bundle(request: &CreateDeploymentBundle) -> Result<(), RepositoryError> {
    request
        .control
        .validate()
        .map_err(RepositoryError::Conflict)?;
    request
        .revision
        .validate_agent_binding_for_workload(&request.workload)
        .map_err(RepositoryError::Conflict)?;
    request
        .revision
        .validate_mcp_binding_for_workload(&request.workload)
        .map_err(RepositoryError::Conflict)?;
    request
        .revision
        .validate_skill_bindings_for_workload(&request.workload)
        .map_err(RepositoryError::Conflict)?;
    if request.control.placement_policy.topology()
        != crate::modules::workloads::domain::entities::PlacementTopology::SingleNode
        || request.revision.workload_id != request.workload.id
        || request.deployment.organization_id != request.workload.organization_id
        || request.deployment.workload_id != request.workload.id
        || request.deployment.revision_id != request.revision.id
        || request.deployment.operation_id != request.operation.id
        || request.operation.organization_id != request.workload.organization_id
        || request.operation.subject.kind() != "deployment"
        || request.operation.subject.id() != request.deployment.id.as_uuid()
        || request
            .revision
            .external_build
            .as_ref()
            .is_some_and(|external| {
                request.revision.template.is_none()
                    || external.organization_id != request.workload.organization_id
                    || external.project_id != request.workload.project_id
                    || external.environment_id != request.workload.environment_id
            })
    {
        return Err(RepositoryError::Conflict(
            "deployment creation bundle has inconsistent identities".into(),
        ));
    }
    Ok(())
}

fn validate_cancellation_bundle(
    request: &RequestDeploymentCancellationBundle,
    current: &Deployment,
) -> Result<(), RepositoryError> {
    let mut expected = current.clone();
    let at = request
        .deployment
        .cancellation_requested_at
        .ok_or_else(|| RepositoryError::Conflict("cancellation request omitted its time".into()))?;
    expected
        .request_cancellation(at)
        .map_err(RepositoryError::Conflict)?;
    if current.aggregate_version != request.expected_version
        || expected != request.deployment
        || request.event.organization_id != request.deployment.organization_id.as_uuid()
        || request.event.aggregate_id != request.deployment.id.as_uuid()
        || request.event.aggregate_version != request.deployment.aggregate_version
    {
        return Err(RepositoryError::Conflict(
            "deployment cancellation bundle is inconsistent with stored state".into(),
        ));
    }
    Ok(())
}

fn state_workload(
    state: &State,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Workload, RepositoryError> {
    state
        .workloads
        .get(&workload_id)
        .filter(|workload| workload.organization_id == organization_id)
        .cloned()
        .ok_or(RepositoryError::NotFound)
}

async fn mutate(
    state: &RwLock<State>,
    deployment_id: DeploymentId,
    expected_version: u64,
    transition: impl FnOnce(&mut Deployment) -> Result<(), String>,
) -> Result<Deployment, RepositoryError> {
    let mut state = state.write().await;
    let deployment = state
        .deployments
        .get_mut(&deployment_id)
        .ok_or(RepositoryError::NotFound)?;
    if deployment.aggregate_version != expected_version {
        return Err(version_conflict(
            expected_version,
            deployment.aggregate_version,
        ));
    }
    transition(deployment).map_err(transition_error)?;
    Ok(deployment.clone())
}

async fn mutate_desired(
    state: &RwLock<State>,
    deployment_id: DeploymentId,
    expected_version: u64,
    transition: impl FnOnce(&mut Deployment) -> Result<(), String>,
) -> Result<Deployment, RepositoryError> {
    let mut state = state.write().await;
    let mut deployment = state
        .deployments
        .get(&deployment_id)
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    if deployment.aggregate_version != expected_version {
        return Err(version_conflict(
            expected_version,
            deployment.aggregate_version,
        ));
    }
    require_current_desired_replica(&state, &deployment)?;
    transition(&mut deployment).map_err(transition_error)?;
    state.deployments.insert(deployment_id, deployment.clone());
    Ok(deployment)
}

fn require_current_desired_replica(
    state: &State,
    deployment: &Deployment,
) -> Result<(), RepositoryError> {
    let workload = state
        .workloads
        .get(&deployment.workload_id)
        .filter(|workload| workload.organization_id == deployment.organization_id)
        .ok_or_else(|| {
            RepositoryError::Storage("deployment references a missing Workload".into())
        })?;
    let control = state.controls.get(&deployment.workload_id).ok_or_else(|| {
        RepositoryError::Storage("deployment Workload is missing its control record".into())
    })?;
    let binding = state
        .deployment_replica_bindings
        .get(&deployment.id)
        .ok_or_else(|| {
            RepositoryError::Storage("deployment is missing its replica binding".into())
        })?;
    let replica = state.replicas.get(&binding.replica_id).ok_or_else(|| {
        RepositoryError::Storage("deployment binding references a missing replica".into())
    })?;
    let member = state
        .replica_members
        .get(&binding.member_id)
        .ok_or_else(|| {
            RepositoryError::Storage("deployment binding references a missing member".into())
        })?;
    if workload.desired_state
        != crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
        || binding.organization_id != deployment.organization_id
        || binding.workload_id != deployment.workload_id
        || binding.revision_id != deployment.revision_id
        || binding.replica_generation != replica.generation
        || binding.replica_id != replica.id
        || replica.lifecycle != WorkloadReplicaLifecycle::Desired
        || replica.ordinal >= control.spec.placement_policy.desired_replicas()
        || replica.revision_id != deployment.revision_id
        || member.replica_id != replica.id
        || binding.member_id != member.id
        || binding.node_id != deployment.node_id
        || binding.node_id.is_some() && binding.node_id != member.node_id
    {
        return Err(RepositoryError::Conflict(
            "deployment replica generation is no longer desired".into(),
        ));
    }
    Ok(())
}

fn version_conflict(expected: u64, actual: u64) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "deployment changed from expected version {expected} to {actual}"
    ))
}

fn require_replica_version(
    replica: &WorkloadReplica,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    if replica.aggregate_version == expected_version {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(format!(
            "Workload replica changed from expected version {expected_version} to {}",
            replica.aggregate_version
        )))
    }
}

fn transition_error(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("deployment transition was rejected: {error}"))
}
