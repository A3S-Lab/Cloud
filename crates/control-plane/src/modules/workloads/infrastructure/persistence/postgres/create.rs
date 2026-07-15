use super::queries;
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::domain::entities::{DeploymentStatus, Workload};
use crate::modules::workloads::domain::repositories::{CreateDeploymentBundle, DeploymentBundle};
use a3s_orm::{sql_query, PostgresExecutor, PostgresTransaction};

pub(super) async fn deployment(
    executor: &PostgresExecutor,
    request: CreateDeploymentBundle,
) -> Result<DeploymentBundle, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) =
                    idempotency_replay::<DeploymentBundle>(transaction, &request.idempotency)
                        .await?
                {
                    let mut response = replay.value;
                    response.replayed = true;
                    return Ok(response);
                }
                validate(&request)?;
                let workload = lock_or_insert_workload(transaction, &request.workload).await?;
                require_next_generation(transaction, &request).await?;
                insert_revision(transaction, &request).await?;
                insert_operation(transaction, &request).await?;
                insert_deployment(transaction, &request).await?;

                let response = DeploymentBundle {
                    workload,
                    revision: request.revision,
                    deployment: request.deployment,
                    operation: request.operation,
                    replayed: false,
                };
                store_outbox(transaction, &request.event).await?;
                store_idempotency(transaction, &request.idempotency, &response).await?;
                Ok(response)
            })
        })
        .await
        .map_err(transaction_error)
}

fn validate(request: &CreateDeploymentBundle) -> Result<(), PostgresPersistenceError> {
    let workload = &request.workload;
    let revision = &request.revision;
    let deployment = &request.deployment;
    let operation = &request.operation;
    let event = &request.event;
    if revision.workload_id != workload.id
        || deployment.organization_id != workload.organization_id
        || deployment.workload_id != workload.id
        || deployment.revision_id != revision.id
        || deployment.operation_id != operation.id
        || operation.organization_id != workload.organization_id
        || operation.subject.kind() != "deployment"
        || operation.subject.id() != deployment.id.as_uuid()
        || operation.requested_at != deployment.requested_at
        || deployment.status != DeploymentStatus::Queued
        || deployment.node_id.is_some()
        || deployment.command_id.is_some()
        || deployment.cleanup_command_id.is_some()
        || deployment.failure.is_some()
        || deployment.activated_at.is_some()
        || deployment.cancellation_requested_at.is_some()
        || deployment.cancelled_at.is_some()
        || deployment.aggregate_version != 1
        || event.organization_id != workload.organization_id.as_uuid()
        || event.aggregate_id != deployment.id.as_uuid()
        || event.aggregate_version != deployment.aggregate_version
    {
        return Err(RepositoryError::Conflict(
            "deployment creation bundle has inconsistent identities or state".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_or_insert_workload(
    transaction: &PostgresTransaction,
    workload: &Workload,
) -> Result<Workload, PostgresPersistenceError> {
    if let Some(existing) =
        queries::workload_in_transaction(transaction, workload.organization_id, workload.id, true)
            .await?
    {
        if &existing != workload {
            return Err(RepositoryError::Conflict(
                "workload changed before a new revision was requested".into(),
            )
            .into());
        }
        return Ok(existing);
    }

    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into workloads (id, organization_id, project_id, environment_id, name, name_key, desired_state, active_revision_id, aggregate_version, created_at, updated_at) values (",
        )
        .bind(workload.id.as_uuid())
        .append(", ")
        .bind(workload.organization_id.as_uuid())
        .append(", ")
        .bind(workload.project_id.as_uuid())
        .append(", ")
        .bind(workload.environment_id.as_uuid())
        .append(", ")
        .bind(workload.name.as_str())
        .append(", ")
        .bind(workload.name.key())
        .append(", ")
        .bind(workload.desired_state.as_str())
        .append(", ")
        .bind(workload.active_revision_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(workload.aggregate_version)
        .append(", ")
        .bind(workload.created_at)
        .append(", ")
        .bind(workload.updated_at)
        .append(")"),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("workload", rows)?,
        Err(error) if is_foreign_key_violation(&error) => {
            return Err(RepositoryError::NotFound.into())
        }
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "workload name or identity is already in use".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }
    Ok(workload.clone())
}

async fn require_next_generation(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    let latest = fetch_optional::<Option<i64>, _>(
        transaction,
        sql_query::<Option<i64>>(
            "select max(generation) from workload_revisions where workload_id = ",
        )
        .bind(request.workload.id.as_uuid()),
    )
    .await?
    .flatten()
    .unwrap_or_default();
    let next = u64::try_from(latest)
        .ok()
        .and_then(|generation| generation.checked_add(1))
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant("workload generation overflowed".into())
        })?;
    if request.revision.generation != next {
        return Err(RepositoryError::Conflict(format!(
            "workload revision generation must be {next}"
        ))
        .into());
    }
    Ok(())
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    let revision = &request.revision;
    let artifact = revision
        .template
        .as_ref()
        .map(|template| &template.artifact);
    let template = revision
        .template
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into workload_revisions (id, workload_id, generation, resolution_state, artifact_source_uri, expected_artifact_digest, template_request, request_digest, artifact_uri, artifact_digest, artifact_media_type, template, template_digest, created_at, resolved_at) values (",
        )
        .bind(revision.id.as_uuid())
        .append(", ")
        .bind(revision.workload_id.as_uuid())
        .append(", ")
        .bind(revision.generation)
        .append(", ")
        .bind(if revision.template.is_some() {
            "resolved"
        } else {
            "pending"
        })
        .append(", ")
        .bind(revision.request.artifact.uri.as_str())
        .append(", ")
        .bind(revision.request.artifact.expected_digest.as_deref())
        .append(", ")
        .bind(serde_json::to_value(&revision.request)?)
        .append(", ")
        .bind(revision.request_digest.as_str())
        .append(", ")
        .bind(artifact.map(|artifact| artifact.uri.as_str()))
        .append(", ")
        .bind(artifact.map(|artifact| artifact.digest.as_str()))
        .append(", ")
        .bind(artifact.map(|artifact| artifact.media_type.as_str()))
        .append(", ")
        .bind(template)
        .append(", ")
        .bind(revision.template_digest.as_deref())
        .append(", ")
        .bind(revision.created_at)
        .append(", ")
        .bind(revision.resolved_at)
        .append(")"),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("workload revision", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "workload revision identity or generation is already in use".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}

async fn insert_operation(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    let operation = &request.operation;
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into operation_requests (operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at) values (",
        )
        .bind(operation.id.as_uuid())
        .append(", ")
        .bind(operation.organization_id.as_uuid())
        .append(", ")
        .bind(operation.subject.kind())
        .append(", ")
        .bind(operation.subject.id())
        .append(", ")
        .bind(operation.workflow.name())
        .append(", ")
        .bind(operation.workflow.version())
        .append(", ")
        .bind(operation.input.clone())
        .append(", ")
        .bind(operation.requested_at)
        .append(")"),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("deployment operation request", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "deployment operation identity is already in use".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}

async fn insert_deployment(
    transaction: &PostgresTransaction,
    request: &CreateDeploymentBundle,
) -> Result<(), PostgresPersistenceError> {
    let deployment = &request.deployment;
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into deployments (id, organization_id, workload_id, revision_id, operation_id, node_id, command_id, cleanup_command_id, status, failure, aggregate_version, requested_at, updated_at, activated_at, cancellation_requested_at, cancelled_at) values (",
        )
        .bind(deployment.id.as_uuid())
        .append(", ")
        .bind(deployment.organization_id.as_uuid())
        .append(", ")
        .bind(deployment.workload_id.as_uuid())
        .append(", ")
        .bind(deployment.revision_id.as_uuid())
        .append(", ")
        .bind(deployment.operation_id.as_uuid())
        .append(", ")
        .bind(deployment.node_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(deployment.command_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(deployment.cleanup_command_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(deployment.status.as_str())
        .append(", ")
        .bind(deployment.failure.as_deref())
        .append(", ")
        .bind(deployment.aggregate_version)
        .append(", ")
        .bind(deployment.requested_at)
        .append(", ")
        .bind(deployment.updated_at)
        .append(", ")
        .bind(deployment.activated_at)
        .append(", ")
        .bind(deployment.cancellation_requested_at)
        .append(", ")
        .bind(deployment.cancelled_at)
        .append(")"),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("deployment", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "deployment identity or revision is already in use".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}
