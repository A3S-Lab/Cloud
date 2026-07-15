use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, OciArtifact, Workload, WorkloadRevision,
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
        let workload = if let Some(existing) = state.workloads.get(&request.workload.id) {
            if existing != &request.workload {
                return Err(RepositoryError::Conflict(
                    "workload changed before a new revision was requested".into(),
                ));
            }
            existing.clone()
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
            state.names.insert(name_key, request.workload.id);
            state
                .workloads
                .insert(request.workload.id, request.workload.clone());
            request.workload.clone()
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
        {
            return Err(RepositoryError::Conflict(
                "workload revision or deployment identity is already in use".into(),
            ));
        }
        state
            .revisions
            .insert(request.revision.id, request.revision.clone());
        state
            .deployments
            .insert(request.deployment.id, request.deployment.clone());
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
        mutate(&self.state, deployment_id, expected_version, |deployment| {
            deployment.schedule(node_id, at)
        })
        .await
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
        at: DateTime<Utc>,
    ) -> Result<(Workload, Deployment), RepositoryError> {
        let mut state = self.state.write().await;
        let (workload_id, revision_id) = {
            let deployment = state
                .deployments
                .get(&deployment_id)
                .ok_or(RepositoryError::NotFound)?;
            if deployment.aggregate_version != expected_version {
                if deployment.status
                    == crate::modules::workloads::domain::entities::DeploymentStatus::Active
                {
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
        deployment.activate(at).map_err(transition_error)?;
        workload
            .activate(revision_id, at)
            .map_err(transition_error)?;
        state.deployments.insert(deployment_id, deployment.clone());
        state.workloads.insert(workload_id, workload.clone());
        Ok((workload, deployment))
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
                            && deployment.status
                                == crate::modules::workloads::domain::entities::DeploymentStatus::Active
                    })
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "active Runtime target has no active deployment".into(),
                        )
                    })?;
                Ok(ActiveRuntimeTarget {
                    workload,
                    revision,
                    deployment,
                })
            })
            .collect()
    }
}

fn validate_bundle(request: &CreateDeploymentBundle) -> Result<(), RepositoryError> {
    if request.revision.workload_id != request.workload.id
        || request.deployment.organization_id != request.workload.organization_id
        || request.deployment.workload_id != request.workload.id
        || request.deployment.revision_id != request.revision.id
        || request.deployment.operation_id != request.operation.id
        || request.operation.organization_id != request.workload.organization_id
        || request.operation.subject.kind() != "deployment"
        || request.operation.subject.id() != request.deployment.id.as_uuid()
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
