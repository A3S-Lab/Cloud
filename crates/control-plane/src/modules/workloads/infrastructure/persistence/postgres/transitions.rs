use super::schema::{Deployments, WorkloadRevisions, Workloads};
use super::{queries, replicas};
use crate::infrastructure::{
    execute, idempotency_replay, require_one_row, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, IdempotentWrite, NodeCommandId, NodeId, OrganizationId,
    RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, OciArtifact, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::RequestDeploymentCancellationBundle;
use a3s_orm::{update_table, PostgresExecutor, PostgresTransaction};
use chrono::{DateTime, Utc};

pub(super) enum DeploymentMutation {
    Resolve {
        at: DateTime<Utc>,
    },
    Schedule {
        node_id: NodeId,
        at: DateTime<Utc>,
    },
    Dispatch {
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    },
    Verify {
        at: DateTime<Utc>,
    },
    DispatchRetirement {
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    },
    CompleteRetirement {
        at: DateTime<Utc>,
    },
    Fail {
        reason: String,
        at: DateTime<Utc>,
    },
    RequestCancellation {
        at: DateTime<Utc>,
    },
    BeginCleanup {
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    },
    RetryCleanup {
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    },
    Cancel {
        at: DateTime<Utc>,
    },
}

impl DeploymentMutation {
    fn already_applied(&self, deployment: &Deployment) -> bool {
        match self {
            Self::Resolve { .. } => matches!(
                deployment.status,
                DeploymentStatus::Resolving
                    | DeploymentStatus::Scheduled
                    | DeploymentStatus::Applying
                    | DeploymentStatus::Verifying
                    | DeploymentStatus::Retiring
                    | DeploymentStatus::Active
            ),
            Self::Schedule { node_id, .. } => {
                deployment.node_id == Some(*node_id)
                    && matches!(
                        deployment.status,
                        DeploymentStatus::Scheduled
                            | DeploymentStatus::Applying
                            | DeploymentStatus::Verifying
                            | DeploymentStatus::Retiring
                            | DeploymentStatus::Active
                    )
            }
            Self::Dispatch { command_id, .. } => {
                deployment.command_id == Some(*command_id)
                    && matches!(
                        deployment.status,
                        DeploymentStatus::Applying
                            | DeploymentStatus::Verifying
                            | DeploymentStatus::Retiring
                            | DeploymentStatus::Active
                    )
            }
            Self::Verify { .. } => matches!(
                deployment.status,
                DeploymentStatus::Verifying | DeploymentStatus::Retiring | DeploymentStatus::Active
            ),
            Self::DispatchRetirement { command_id, .. } => {
                deployment.retirement_command_id == Some(*command_id)
                    && matches!(
                        deployment.status,
                        DeploymentStatus::Retiring
                            | DeploymentStatus::Active
                            | DeploymentStatus::Orphaned
                    )
            }
            Self::CompleteRetirement { .. } => deployment.status == DeploymentStatus::Active,
            Self::Fail { reason, .. } => {
                matches!(
                    deployment.status,
                    DeploymentStatus::Failed | DeploymentStatus::Orphaned
                ) && deployment.failure.as_ref() == Some(reason)
            }
            Self::RequestCancellation { .. } => matches!(
                deployment.status,
                DeploymentStatus::Cancelling
                    | DeploymentStatus::CleanupPending
                    | DeploymentStatus::Cancelled
            ),
            Self::BeginCleanup { command_id, .. } => {
                deployment.cleanup_command_id == Some(*command_id)
                    && matches!(
                        deployment.status,
                        DeploymentStatus::CleanupPending | DeploymentStatus::Cancelled
                    )
            }
            Self::RetryCleanup { command_id, .. } => {
                deployment.cleanup_command_id == Some(*command_id)
                    && matches!(
                        deployment.status,
                        DeploymentStatus::CleanupPending | DeploymentStatus::Cancelled
                    )
            }
            Self::Cancel { .. } => deployment.status == DeploymentStatus::Cancelled,
        }
    }

    fn apply(self, deployment: &mut Deployment) -> Result<(), String> {
        match self {
            Self::Resolve { at } => deployment.resolve(at),
            Self::Schedule { node_id, at } => deployment.schedule(node_id, at),
            Self::Dispatch { command_id, at } => deployment.dispatch(command_id, at),
            Self::Verify { at } => deployment.verify(at),
            Self::DispatchRetirement { command_id, at } => {
                deployment.dispatch_retirement(command_id, at)
            }
            Self::CompleteRetirement { at } => deployment.complete_retirement(at),
            Self::Fail { reason, at } => deployment.fail(reason, at),
            Self::RequestCancellation { at } => deployment.request_cancellation(at),
            Self::BeginCleanup { command_id, at } => deployment.begin_cleanup(command_id, at),
            Self::RetryCleanup { command_id, at } => deployment.retry_cleanup(command_id, at),
            Self::Cancel { at } => deployment.cancel(at),
        }
    }
}

pub(super) async fn mutate(
    executor: &PostgresExecutor,
    deployment_id: DeploymentId,
    expected_version: u64,
    mutation: DeploymentMutation,
) -> Result<Deployment, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut deployment =
                    queries::deployment_in_transaction(transaction, deployment_id, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                if mutation.already_applied(&deployment) {
                    return Ok(deployment);
                }
                require_expected_version(&deployment, expected_version)?;
                let previous_version = deployment.aggregate_version;
                let updates_placement = matches!(&mutation, DeploymentMutation::Schedule { .. });
                mutation.apply(&mut deployment).map_err(|error| {
                    RepositoryError::Conflict(format!(
                        "deployment transition was rejected: {error}"
                    ))
                })?;
                persist_deployment(transaction, &deployment, previous_version).await?;
                if updates_placement {
                    replicas::place(transaction, &deployment).await?;
                }
                Ok(deployment)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn request_cancellation(
    executor: &PostgresExecutor,
    request: RequestDeploymentCancellationBundle,
) -> Result<IdempotentWrite<Deployment>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) =
                    idempotency_replay::<Deployment>(transaction, &request.idempotency).await?
                {
                    return Ok(IdempotentWrite {
                        value: replay.value,
                        replayed: true,
                    });
                }
                let current =
                    queries::deployment_in_transaction(transaction, request.deployment.id, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                replicas::require_direct_mutation(
                    transaction,
                    current.organization_id,
                    current.workload_id,
                )
                .await?;
                if current.aggregate_version != request.expected_version {
                    return Err(RepositoryError::Conflict(format!(
                        "deployment changed from expected version {} to {}",
                        request.expected_version, current.aggregate_version
                    ))
                    .into());
                }
                let at = request
                    .deployment
                    .cancellation_requested_at
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "cancellation request omitted its persisted time".into(),
                        )
                    })?;
                let mut expected = current.clone();
                expected.request_cancellation(at).map_err(|error| {
                    RepositoryError::Conflict(format!(
                        "deployment cancellation was rejected: {error}"
                    ))
                })?;
                if expected != request.deployment
                    || request.event.organization_id != request.deployment.organization_id.as_uuid()
                    || request.event.aggregate_id != request.deployment.id.as_uuid()
                    || request.event.aggregate_version != request.deployment.aggregate_version
                {
                    return Err(RepositoryError::Conflict(
                        "deployment cancellation bundle is inconsistent with stored state".into(),
                    )
                    .into());
                }
                persist_deployment(transaction, &request.deployment, request.expected_version)
                    .await?;
                store_outbox(transaction, &request.event).await?;
                store_idempotency(transaction, &request.idempotency, &request.deployment).await?;
                Ok(IdempotentWrite {
                    value: request.deployment,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn cancellation_replay(
    executor: &PostgresExecutor,
    idempotency: &IdempotencyRequest,
) -> Result<Option<Deployment>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                Ok(idempotency_replay::<Deployment>(transaction, &idempotency)
                    .await?
                    .map(|replay| replay.value))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn resolve_revision(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
    artifact: OciArtifact,
    resolved_at: DateTime<Utc>,
) -> Result<WorkloadRevision, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut revision = queries::revision_in_transaction(
                    transaction,
                    organization_id,
                    revision_id,
                    true,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let was_resolved = revision.template.is_some();
                revision.resolve(artifact, resolved_at).map_err(|error| {
                    RepositoryError::Conflict(format!(
                        "workload revision resolution was rejected: {error}"
                    ))
                })?;
                if was_resolved {
                    return Ok(revision);
                }

                let template = revision.template.as_ref().ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "resolved workload revision omitted its template".into(),
                    )
                })?;
                let template_digest = revision.template_digest.as_deref().ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "resolved workload revision omitted its template digest".into(),
                    )
                })?;
                let resolved_at = revision.resolved_at.ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "resolved workload revision omitted its resolution time".into(),
                    )
                })?;
                let rows = execute(
                    transaction,
                    update_table::<WorkloadRevisions>()
                        .set(WorkloadRevisions::resolution_state(), "resolved")
                        .set(
                            WorkloadRevisions::artifact_uri(),
                            template.artifact.uri.clone(),
                        )
                        .set(
                            WorkloadRevisions::artifact_digest(),
                            template.artifact.digest.clone(),
                        )
                        .set(
                            WorkloadRevisions::artifact_media_type(),
                            template.artifact.media_type.clone(),
                        )
                        .set(
                            WorkloadRevisions::template(),
                            serde_json::to_value(template)?,
                        )
                        .set(
                            WorkloadRevisions::template_digest(),
                            template_digest.to_owned(),
                        )
                        .set(WorkloadRevisions::resolved_at(), resolved_at)
                        .filter(WorkloadRevisions::id().eq(revision.id.as_uuid()))
                        .filter(WorkloadRevisions::resolution_state().eq("pending")),
                )
                .await?;
                require_one_row("workload revision resolution", rows)?;
                Ok(revision)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn activate(
    executor: &PostgresExecutor,
    deployment_id: DeploymentId,
    expected_version: u64,
    retirement_required: bool,
    at: DateTime<Utc>,
) -> Result<(Workload, Deployment), RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut deployment =
                    queries::deployment_in_transaction(transaction, deployment_id, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                let mut workload = queries::workload_in_transaction(
                    transaction,
                    deployment.organization_id,
                    deployment.workload_id,
                    true,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;

                if matches!(
                    deployment.status,
                    DeploymentStatus::Retiring | DeploymentStatus::Active
                ) {
                    deployment
                        .activate(retirement_required, at)
                        .map_err(|error| {
                            RepositoryError::Conflict(format!(
                                "deployment activation replay was rejected: {error}"
                            ))
                        })?;
                    if workload.active_revision_id != Some(deployment.revision_id) {
                        return Err(PostgresPersistenceError::Invariant(
                            "active deployment is not selected by its workload".into(),
                        ));
                    }
                    return Ok((workload, deployment));
                }
                require_expected_version(&deployment, expected_version)?;
                let previous_deployment_version = deployment.aggregate_version;
                let previous_workload_version = workload.aggregate_version;
                deployment
                    .activate(retirement_required, at)
                    .map_err(|error| {
                        RepositoryError::Conflict(format!(
                            "deployment activation was rejected: {error}"
                        ))
                    })?;
                workload
                    .activate(deployment.revision_id, at)
                    .map_err(|error| {
                        RepositoryError::Conflict(format!(
                            "workload activation was rejected: {error}"
                        ))
                    })?;
                persist_deployment(transaction, &deployment, previous_deployment_version).await?;
                persist_workload(transaction, &workload, previous_workload_version).await?;
                Ok((workload, deployment))
            })
        })
        .await
        .map_err(transaction_error)
}

fn require_expected_version(
    deployment: &Deployment,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    if deployment.aggregate_version == expected_version {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(format!(
            "deployment changed from expected version {expected_version} to {}",
            deployment.aggregate_version
        ))
        .into())
    }
}

async fn persist_deployment(
    transaction: &PostgresTransaction,
    deployment: &Deployment,
    previous_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<Deployments>()
            .set(
                Deployments::node_id(),
                deployment.node_id.map(|id| id.as_uuid()),
            )
            .set(
                Deployments::command_id(),
                deployment.command_id.map(|id| id.as_uuid()),
            )
            .set(
                Deployments::cleanup_command_id(),
                deployment.cleanup_command_id.map(|id| id.as_uuid()),
            )
            .set(
                Deployments::retirement_command_id(),
                deployment.retirement_command_id.map(|id| id.as_uuid()),
            )
            .set(Deployments::status(), deployment.status.as_str())
            .set(Deployments::failure(), deployment.failure.clone())
            .set(
                Deployments::aggregate_version(),
                deployment.aggregate_version,
            )
            .set(Deployments::updated_at(), deployment.updated_at)
            .set(Deployments::activated_at(), deployment.activated_at)
            .set(
                Deployments::cancellation_requested_at(),
                deployment.cancellation_requested_at,
            )
            .set(Deployments::cancelled_at(), deployment.cancelled_at)
            .filter(Deployments::id().eq(deployment.id.as_uuid()))
            .filter(Deployments::aggregate_version().eq(previous_version)),
    )
    .await?;
    require_one_row("deployment transition", rows)
}

pub(super) async fn persist_workload(
    transaction: &PostgresTransaction,
    workload: &Workload,
    previous_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<Workloads>()
            .set(Workloads::desired_state(), workload.desired_state.as_str())
            .set(
                Workloads::active_revision_id(),
                workload.active_revision_id.map(|id| id.as_uuid()),
            )
            .set(Workloads::aggregate_version(), workload.aggregate_version)
            .set(Workloads::updated_at(), workload.updated_at)
            .filter(Workloads::organization_id().eq(workload.organization_id.as_uuid()))
            .filter(Workloads::id().eq(workload.id.as_uuid()))
            .filter(Workloads::aggregate_version().eq(previous_version)),
    )
    .await?;
    require_one_row("workload activation", rows)
}
