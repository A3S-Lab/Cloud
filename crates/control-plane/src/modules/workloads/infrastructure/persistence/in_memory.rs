use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, OciArtifact, Workload, WorkloadControl, WorkloadReplica,
    WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    ActiveRuntimeTarget, CreateDeploymentBundle, DeploymentBundle, IWorkloadRepository,
    IWorkloadRuntimeTargetRepository, RequestDeploymentCancellationBundle,
    RequestWorkloadStopBundle, WorkloadStopBundle,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

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
    deployment_replica_bindings: BTreeMap<DeploymentId, DeploymentReplicaBinding>,
    idempotency: BTreeMap<(String, String), (String, DeploymentBundle)>,
    cancellation_idempotency: BTreeMap<(String, String), (String, Deployment)>,
    stop_idempotency: BTreeMap<(String, String), (String, WorkloadStopBundle)>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

impl InMemoryWorkloadRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
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
        let is_new_workload = !state.workloads.contains_key(&request.workload.id);
        let (workload, control, replica, member) = if let Some(existing) =
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
            let control = state.controls.get(&existing.id).cloned().ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its durable control record".into())
            })?;
            control
                .require_authority(&request.control)
                .map_err(RepositoryError::Conflict)?;
            let replica_id = WorkloadReplicaId::from_uuid(existing.id.as_uuid());
            let mut replica = state.replicas.get(&replica_id).cloned().ok_or_else(|| {
                RepositoryError::Storage("Workload is missing its canonical replica".into())
            })?;
            replica
                .advance(&request.revision, request.revision.created_at)
                .map_err(RepositoryError::Conflict)?;
            let member_id = WorkloadReplicaMemberId::from_uuid(existing.id.as_uuid());
            let member = state
                .replica_members
                .get(&member_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Workload is missing its canonical replica member".into(),
                    )
                })?;
            (existing.clone(), control, replica, member)
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
            let replica = WorkloadReplica::canonical(&request.workload, &request.revision)
                .map_err(RepositoryError::Conflict)?;
            let member = WorkloadReplicaMember::canonical(&request.workload, &replica)
                .map_err(RepositoryError::Conflict)?;
            (request.workload.clone(), control, replica, member)
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
        let binding = DeploymentReplicaBinding::create(
            &request.deployment,
            &request.revision,
            &replica,
            &member,
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
            state.replica_members.insert(member.id, member);
        }
        state.replicas.insert(replica.id, replica);
        state
            .revisions
            .insert(request.revision.id, request.revision.clone());
        state
            .deployments
            .insert(request.deployment.id, request.deployment.clone());
        state
            .deployment_replica_bindings
            .insert(request.deployment.id, binding);
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
        mutate(&self.state, deployment_id, expected_version, |deployment| {
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
            .insert(deployment_id, binding);
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
        mutate(&self.state, deployment_id, expected_version, |deployment| {
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
        mutate(&self.state, deployment_id, expected_version, |deployment| {
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
        let mut workload = state
            .workloads
            .get(&workload_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        deployment
            .activate(retirement_required, at)
            .map_err(transition_error)?;
        workload
            .activate(revision_id, at)
            .map_err(transition_error)?;
        state.deployments.insert(deployment_id, deployment.clone());
        state.workloads.insert(workload_id, workload.clone());
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
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.fail(reason, at)
        })
        .await
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
        let mut workloads = state
            .workloads
            .values()
            .filter(|workload| {
                workload.desired_state
                    == crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
                    && workload.active_revision_id.is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        workloads.sort_by_key(|workload| (workload.updated_at, workload.id));
        workloads.truncate(limit);

        workloads
            .into_iter()
            .map(|workload| {
                let revision_id = workload.active_revision_id.ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target omitted its selected revision".into(),
                    )
                })?;
                let revision = state.revisions.get(&revision_id).cloned().ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Runtime target references a missing revision".into(),
                    )
                })?;
                let deployment = state
                    .deployments
                    .values()
                    .find(|deployment| {
                        deployment.workload_id == workload.id
                            && deployment.revision_id == revision_id
                            && matches!(
                                deployment.status,
                                crate::modules::workloads::domain::entities::DeploymentStatus::Retiring
                                    | crate::modules::workloads::domain::entities::DeploymentStatus::Active
                            )
                    })
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "active Runtime target has no active deployment".into(),
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
                Ok(ActiveRuntimeTarget {
                    workload,
                    revision,
                    deployment,
                    replica_binding,
                })
            })
            .collect()
    }
}

fn validate_bundle(request: &CreateDeploymentBundle) -> Result<(), RepositoryError> {
    request
        .control
        .validate()
        .map_err(RepositoryError::Conflict)?;
    if request.revision.workload_id != request.workload.id
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

fn version_conflict(expected: u64, actual: u64) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "deployment changed from expected version {expected} to {actual}"
    ))
}

fn transition_error(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("deployment transition was rejected: {error}"))
}
