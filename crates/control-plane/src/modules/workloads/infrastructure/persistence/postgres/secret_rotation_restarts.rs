use super::schema::{
    ActiveWorkloads, Deployments, SecretRotationReconciliations, SecretRotationRestarts,
    SecretVersions, Secrets, WorkloadRevisions, Workloads,
};
use super::{create, queries, replicas};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error, OutboxEvents,
    PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::SecretChanged;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, IdempotencyRequest, OperationId, OrganizationId,
    RepositoryError, SecretId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{Deployment, WorkloadDesiredState};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, DeploymentBundle, SecretRotation, SecretRotationCompletion,
    SecretRotationReconciliation,
};
use a3s_orm::{
    bound, cast, count_all, exists, insert_into, not, select_from, select_from_as, sql_function,
    Database, Expression, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

type RotationRow = (Uuid, Uuid, Uuid, DateTime<Utc>, Uuid, serde_json::Value);

struct JsonPath;

pub(super) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<SecretRotation>, RepositoryError> {
    let limit = checked_limit(limit, "Secret rotation event")?;
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<OutboxEvents>()
                .select((
                    OutboxEvents::event_id(),
                    OutboxEvents::organization_id(),
                    OutboxEvents::aggregate_id(),
                    OutboxEvents::occurred_at(),
                    OutboxEvents::correlation_id(),
                    OutboxEvents::payload(),
                ))
                .left_join::<SecretRotationReconciliations>(
                    SecretRotationReconciliations::secret_event_id()
                        .eq_column(OutboxEvents::event_id()),
                )
                .filter(OutboxEvents::event_key().eq("secret.version.created"))
                .filter(SecretRotationReconciliations::secret_event_id().is_null())
                .order_by(OutboxEvents::occurred_at(), OrderDirection::Asc)
                .order_by(OutboxEvents::event_id(), OrderDirection::Asc)
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(decode_rotation)
        .collect()
}

pub(super) async fn reconcile(
    executor: &PostgresExecutor,
    rotation: SecretRotation,
    workload_limit: usize,
    reconciled_at: DateTime<Utc>,
) -> Result<SecretRotationReconciliation, RepositoryError> {
    rotation.validate().map_err(RepositoryError::Conflict)?;
    let workload_limit = checked_limit(workload_limit, "Secret rotation workload")?;
    let reconciled_at = canonical_timestamp(reconciled_at.max(rotation.occurred_at));
    executor
        .transaction(move |transaction| {
            Box::pin(reconcile_in_transaction(
                transaction,
                rotation,
                workload_limit,
                reconciled_at,
            ))
        })
        .await
        .map_err(transaction_error)
}

async fn reconcile_in_transaction(
    transaction: &PostgresTransaction,
    rotation: SecretRotation,
    workload_limit: u64,
    reconciled_at: DateTime<Utc>,
) -> Result<SecretRotationReconciliation, PostgresPersistenceError> {
    lock_rotation(transaction, rotation.event_id).await?;
    if let Some(completion) = stored_completion(transaction, rotation.event_id).await? {
        return Ok(SecretRotationReconciliation {
            scheduled: Vec::new(),
            completion: Some(completion),
        });
    }
    let authoritative = fetch_optional::<RotationRow, _>(
        transaction,
        select_from::<OutboxEvents>()
            .select((
                OutboxEvents::event_id(),
                OutboxEvents::organization_id(),
                OutboxEvents::aggregate_id(),
                OutboxEvents::occurred_at(),
                OutboxEvents::correlation_id(),
                OutboxEvents::payload(),
            ))
            .filter(OutboxEvents::event_id().eq(rotation.event_id))
            .filter(OutboxEvents::event_key().eq("secret.version.created")),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let authoritative = decode_rotation(authoritative)?;
    if authoritative != rotation {
        return Err(PostgresPersistenceError::Invariant(
            "Secret rotation restart input does not match its durable event".into(),
        ));
    }

    let secret = fetch_optional::<(u64, String, String, Uuid, Uuid), _>(
        transaction,
        secret_version_for_update(&rotation),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let (current_version, secret_state, version_state, project_id, environment_id) = secret;
    if project_id != rotation.project_id.as_uuid()
        || environment_id != rotation.environment_id.as_uuid()
    {
        return Err(PostgresPersistenceError::Invariant(
            "Secret rotation scope changed after its event was committed".into(),
        ));
    }
    if current_version < rotation.version {
        return Err(PostgresPersistenceError::Invariant(
            "Secret current version regressed behind its rotation event".into(),
        ));
    }
    if current_version > rotation.version {
        store_completion(
            transaction,
            &rotation,
            SecretRotationCompletion::Superseded,
            restart_count(transaction, rotation.event_id).await?,
            reconciled_at,
        )
        .await?;
        return Ok(SecretRotationReconciliation {
            scheduled: Vec::new(),
            completion: Some(SecretRotationCompletion::Superseded),
        });
    }
    if secret_state != "active" || version_state != "active" {
        store_completion(
            transaction,
            &rotation,
            SecretRotationCompletion::Unavailable,
            restart_count(transaction, rotation.event_id).await?,
            reconciled_at,
        )
        .await?;
        return Ok(SecretRotationReconciliation {
            scheduled: Vec::new(),
            completion: Some(SecretRotationCompletion::Unavailable),
        });
    }

    let candidates = candidate_workloads(transaction, &rotation, workload_limit).await?;
    let mut scheduled = Vec::with_capacity(candidates.len());
    for (workload_id, source_revision_id) in candidates {
        let workload_id = WorkloadId::from_uuid(workload_id);
        let source_revision_id = WorkloadRevisionId::from_uuid(source_revision_id);
        let workload = queries::workload_in_transaction(
            transaction,
            rotation.organization_id,
            workload_id,
            false,
        )
        .await?
        .ok_or(RepositoryError::NotFound)?;
        if workload.desired_state != WorkloadDesiredState::Running
            || workload.project_id != rotation.project_id
            || workload.environment_id != rotation.environment_id
            || workload.active_revision_id != Some(source_revision_id)
        {
            return Err(PostgresPersistenceError::Invariant(
                "locked Secret rotation workload changed before restart derivation".into(),
            ));
        }
        let source_revision = queries::revision_in_transaction(
            transaction,
            rotation.organization_id,
            source_revision_id,
            false,
        )
        .await?
        .ok_or(RepositoryError::NotFound)?;
        if source_revision.workload_id != workload.id {
            return Err(PostgresPersistenceError::Invariant(
                "Secret rotation source revision belongs to another workload".into(),
            ));
        }
        let generation = queries::next_revision_generation(transaction, workload.id).await?;
        let requested_at = canonical_timestamp(
            reconciled_at
                .max(workload.updated_at)
                .max(source_revision.created_at),
        );
        let revision = source_revision
            .restart_for_secret_rotation(
                WorkloadRevisionId::new(),
                generation,
                rotation.secret_id,
                rotation.version,
                requested_at,
            )
            .map_err(|error| {
                RepositoryError::Conflict(format!(
                    "could not derive Secret rotation revision: {error}"
                ))
            })?;
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
            OperationSubject::new("deployment", deployment.id.as_uuid()).map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "could not create Secret rotation operation subject: {error}"
                ))
            })?,
            WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION).map_err(
                |error| {
                    PostgresPersistenceError::Invariant(format!(
                        "could not create Secret rotation workflow identity: {error}"
                    ))
                },
            )?,
            serde_json::json!({
                "deploymentId": deployment.id,
                "organizationId": workload.organization_id,
                "revisionId": revision.id,
                "workloadId": workload.id,
            }),
            requested_at,
        );
        let canonical = serde_json::to_vec(&serde_json::json!({
            "secretEventId": rotation.event_id,
            "secretId": rotation.secret_id,
            "secretVersion": rotation.version,
            "sourceRevisionId": source_revision.id,
            "workloadId": workload.id,
        }))?;
        let idempotency = IdempotencyRequest::new(
            format!("secret-rotation-events/{}/workloads", rotation.event_id),
            workload.id.to_string(),
            &canonical,
        )
        .map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "could not create Secret rotation idempotency identity: {error}"
            ))
        })?;
        let event = DeploymentRequested::caused_by(
            &deployment,
            &revision,
            rotation.correlation_id,
            rotation.event_id,
        )?;
        let control = replicas::control_spec_in_transaction(
            transaction,
            workload.organization_id,
            workload.id,
        )
        .await?;
        let response = create::deployment_in_transaction(
            transaction,
            CreateDeploymentBundle {
                workload,
                control,
                revision,
                deployment,
                operation,
                idempotency,
                event,
            },
        )
        .await?;
        store_restart(
            transaction,
            &rotation,
            source_revision_id,
            &response,
            requested_at,
        )
        .await?;
        scheduled.push(response);
    }

    let completion = if affected_workload_count(transaction, &rotation).await? == 0 {
        let count = restart_count(transaction, rotation.event_id).await?;
        let outcome = if count == 0 {
            SecretRotationCompletion::NoTargets
        } else {
            SecretRotationCompletion::Scheduled
        };
        store_completion(transaction, &rotation, outcome, count, reconciled_at).await?;
        Some(outcome)
    } else {
        None
    };
    Ok(SecretRotationReconciliation {
        scheduled,
        completion,
    })
}

async fn candidate_workloads(
    transaction: &PostgresTransaction,
    rotation: &SecretRotation,
    limit: u64,
) -> Result<Vec<(Uuid, Uuid)>, PostgresPersistenceError> {
    fetch_all::<(Uuid, Uuid), _>(transaction, candidate_workloads_query(rotation, limit)).await
}

async fn affected_workload_count(
    transaction: &PostgresTransaction,
    rotation: &SecretRotation,
) -> Result<i64, PostgresPersistenceError> {
    fetch_optional::<i64, _>(
        transaction,
        affected_workloads(rotation).select(count_all()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Secret rotation affected-workload count returned no row".into(),
        )
    })
}

async fn store_restart(
    transaction: &PostgresTransaction,
    rotation: &SecretRotation,
    source_revision_id: WorkloadRevisionId,
    response: &DeploymentBundle,
    created_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Secret rotation restart",
        execute(
            transaction,
            insert_into::<SecretRotationRestarts>()
                .value(SecretRotationRestarts::secret_event_id(), rotation.event_id)
                .value(
                    SecretRotationRestarts::organization_id(),
                    rotation.organization_id.as_uuid(),
                )
                .value(
                    SecretRotationRestarts::secret_id(),
                    rotation.secret_id.as_uuid(),
                )
                .value(SecretRotationRestarts::secret_version(), rotation.version)
                .value(
                    SecretRotationRestarts::workload_id(),
                    response.workload.id.as_uuid(),
                )
                .value(
                    SecretRotationRestarts::source_revision_id(),
                    source_revision_id.as_uuid(),
                )
                .value(
                    SecretRotationRestarts::target_revision_id(),
                    response.revision.id.as_uuid(),
                )
                .value(
                    SecretRotationRestarts::deployment_id(),
                    response.deployment.id.as_uuid(),
                )
                .value(
                    SecretRotationRestarts::operation_id(),
                    response.operation.id.as_uuid(),
                )
                .value(SecretRotationRestarts::created_at(), created_at),
        )
        .await?,
    )
}

async fn restart_count(
    transaction: &PostgresTransaction,
    event_id: Uuid,
) -> Result<i64, PostgresPersistenceError> {
    fetch_optional::<i64, _>(
        transaction,
        select_from::<SecretRotationRestarts>()
            .select(count_all())
            .filter(SecretRotationRestarts::secret_event_id().eq(event_id)),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Secret rotation restart count returned no row".into())
    })
}

async fn store_completion(
    transaction: &PostgresTransaction,
    rotation: &SecretRotation,
    outcome: SecretRotationCompletion,
    restart_count: i64,
    reconciled_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Secret rotation reconciliation",
        execute(
            transaction,
            insert_into::<SecretRotationReconciliations>()
                .value(
                    SecretRotationReconciliations::secret_event_id(),
                    rotation.event_id,
                )
                .value(
                    SecretRotationReconciliations::organization_id(),
                    rotation.organization_id.as_uuid(),
                )
                .value(
                    SecretRotationReconciliations::secret_id(),
                    rotation.secret_id.as_uuid(),
                )
                .value(
                    SecretRotationReconciliations::secret_version(),
                    rotation.version,
                )
                .value(
                    SecretRotationReconciliations::outcome(),
                    completion_name(outcome),
                )
                .value(
                    SecretRotationReconciliations::restart_count(),
                    restart_count,
                )
                .value(
                    SecretRotationReconciliations::reconciled_at(),
                    reconciled_at,
                ),
        )
        .await?,
    )
}

async fn stored_completion(
    transaction: &PostgresTransaction,
    event_id: Uuid,
) -> Result<Option<SecretRotationCompletion>, PostgresPersistenceError> {
    fetch_optional::<String, _>(
        transaction,
        select_from::<SecretRotationReconciliations>()
            .select(SecretRotationReconciliations::outcome())
            .filter(SecretRotationReconciliations::secret_event_id().eq(event_id)),
    )
    .await?
    .map(|outcome| {
        parse_completion(&outcome).ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "stored Secret rotation reconciliation outcome is invalid".into(),
            )
        })
    })
    .transpose()
}

async fn lock_rotation(
    transaction: &PostgresTransaction,
    event_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock("cloud.secret-rotation-restart", &event_id.to_string())
        .await?;
    Ok(())
}

fn secret_version_for_update(
    rotation: &SecretRotation,
) -> a3s_orm::query::SelectQuery<Secrets, (u64, String, String, Uuid, Uuid)> {
    select_from::<Secrets>()
        .select((
            Secrets::current_version(),
            Secrets::state(),
            SecretVersions::state(),
            Secrets::project_id(),
            Secrets::environment_id(),
        ))
        .inner_join::<SecretVersions>(Secrets::id().eq_column(SecretVersions::secret_id()))
        .filter(SecretVersions::version().eq(rotation.version))
        .filter(Secrets::organization_id().eq(rotation.organization_id.as_uuid()))
        .filter(Secrets::id().eq(rotation.secret_id.as_uuid()))
        .for_update_of::<Secrets>()
        .for_update_of::<SecretVersions>()
}

fn affected_workloads(
    rotation: &SecretRotation,
) -> a3s_orm::query::SelectQuery<ActiveWorkloads, (Uuid, Uuid)> {
    let active_deployment = select_from::<Deployments>()
        .select(Deployments::id())
        .filter(Deployments::workload_id().eq_column(ActiveWorkloads::id()))
        .filter(Deployments::revision_id().eq_column(ActiveWorkloads::active_revision_id()))
        .filter(Deployments::status().eq("active"));
    let handled_restart = select_from::<SecretRotationRestarts>()
        .select(SecretRotationRestarts::workload_id())
        .filter(SecretRotationRestarts::secret_event_id().eq(rotation.event_id))
        .filter(SecretRotationRestarts::workload_id().eq_column(ActiveWorkloads::id()));

    select_from_as::<Workloads, ActiveWorkloads>()
        .select((ActiveWorkloads::id(), ActiveWorkloads::active_revision_id()))
        .inner_join::<WorkloadRevisions>(
            WorkloadRevisions::workload_id()
                .eq_column(ActiveWorkloads::id())
                .and(WorkloadRevisions::id().eq_column(ActiveWorkloads::active_revision_id())),
        )
        .filter(ActiveWorkloads::organization_id().eq(rotation.organization_id.as_uuid()))
        .filter(ActiveWorkloads::project_id().eq(rotation.project_id.as_uuid()))
        .filter(ActiveWorkloads::environment_id().eq(rotation.environment_id.as_uuid()))
        .filter(ActiveWorkloads::desired_state().eq("running"))
        .filter(exists(active_deployment))
        .filter(not(exists(handled_restart)))
        .filter(references_rotated_secret(rotation))
}

fn candidate_workloads_query(
    rotation: &SecretRotation,
    limit: u64,
) -> a3s_orm::query::SelectQuery<ActiveWorkloads, (Uuid, Uuid)> {
    let pending_deployment = select_from::<Deployments>()
        .select(Deployments::id())
        .filter(Deployments::workload_id().eq_column(ActiveWorkloads::id()))
        .filter(Deployments::status().ne("active"))
        .filter(Deployments::status().ne("failed"))
        .filter(Deployments::status().ne("orphaned"))
        .filter(Deployments::status().ne("cancelled"));

    affected_workloads(rotation)
        .filter(not(exists(pending_deployment)))
        .order_by(ActiveWorkloads::updated_at(), OrderDirection::Asc)
        .order_by(ActiveWorkloads::id(), OrderDirection::Asc)
        .for_update_of::<ActiveWorkloads>()
        .skip_locked()
        .limit(limit)
}

fn references_rotated_secret(rotation: &SecretRotation) -> Expression {
    let path = cast::<String, JsonPath>(
        cast::<String, String>(
            bound::<String>("$.secrets[*] ? (@.secret_id == $secret_id && @.version < $version)"),
            "text",
        ),
        "jsonpath",
    );
    let variables = serde_json::json!({
        "secret_id": rotation.secret_id.to_string(),
        "version": rotation.version,
    });
    sql_function::<bool>(
        "jsonb_path_exists",
        [
            WorkloadRevisions::template_request().expression(),
            path.expression(),
            bound::<serde_json::Value>(variables).expression(),
        ],
    )
    .eq(true)
}

fn decode_rotation(row: RotationRow) -> Result<SecretRotation, RepositoryError> {
    let (event_id, organization_id, aggregate_id, occurred_at, correlation_id, payload) = row;
    let payload: SecretChanged = serde_json::from_value(payload).map_err(|error| {
        RepositoryError::Storage(format!("stored Secret rotation event is invalid: {error}"))
    })?;
    let rotation = SecretRotation {
        event_id,
        correlation_id,
        organization_id: OrganizationId::from_uuid(organization_id),
        project_id: payload.project_id,
        environment_id: payload.environment_id,
        secret_id: SecretId::from_uuid(aggregate_id),
        version: payload.version,
        occurred_at,
    };
    rotation.validate().map_err(|error| {
        RepositoryError::Storage(format!("stored Secret rotation event is invalid: {error}"))
    })?;
    if payload.organization_id != rotation.organization_id
        || payload.secret_id != rotation.secret_id
        || payload.state != "active"
        || payload.version_state != "active"
    {
        return Err(RepositoryError::Storage(
            "stored Secret rotation event metadata is inconsistent".into(),
        ));
    }
    Ok(rotation)
}

fn checked_limit(limit: usize, label: &str) -> Result<u64, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(format!(
            "{label} limit must be between 1 and 10000"
        )));
    }
    u64::try_from(limit).map_err(|_| RepositoryError::Conflict(format!("{label} limit is invalid")))
}

const fn completion_name(completion: SecretRotationCompletion) -> &'static str {
    match completion {
        SecretRotationCompletion::Scheduled => "scheduled",
        SecretRotationCompletion::NoTargets => "no_targets",
        SecretRotationCompletion::Superseded => "superseded",
        SecretRotationCompletion::Unavailable => "unavailable",
    }
}

fn parse_completion(value: &str) -> Option<SecretRotationCompletion> {
    match value {
        "scheduled" => Some(SecretRotationCompletion::Scheduled),
        "no_targets" => Some(SecretRotationCompletion::NoTargets),
        "superseded" => Some(SecretRotationCompletion::Superseded),
        "unavailable" => Some(SecretRotationCompletion::Unavailable),
        _ => None,
    }
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
    use a3s_orm::{PostgresDialect, Query, Value};

    #[test]
    fn typed_rotation_queries_preserve_locks_and_bound_jsonpath_values() {
        let rotation = rotation();
        let secret = secret_version_for_update(&rotation)
            .compile(&PostgresDialect)
            .expect("Secret version lock query");
        assert!(secret
            .sql
            .ends_with("for update of \"secrets\", \"secret_versions\""));
        assert_eq!(secret.parameters.len(), 3);

        let candidates = candidate_workloads_query(&rotation, 25)
            .compile(&PostgresDialect)
            .expect("Secret rotation candidate query");
        assert!(candidates.sql.contains("\"jsonb_path_exists\""));
        assert!(candidates
            .sql
            .ends_with("for update of \"active_workloads\" skip locked"));
        assert!(!candidates.sql.contains(&rotation.secret_id.to_string()));
        assert_eq!(candidates.parameters.len(), 14);
        assert!(candidates
            .parameters
            .iter()
            .any(|parameter| matches!(parameter, Value::Json(_))));

        let affected = affected_workloads(&rotation)
            .select(count_all())
            .compile(&PostgresDialect)
            .expect("Secret rotation affected count query");
        assert!(affected.sql.contains("\"jsonb_path_exists\""));
        assert!(!affected.sql.contains("for update"));
        assert_eq!(affected.parameters.len(), 9);
    }

    fn rotation() -> SecretRotation {
        SecretRotation {
            event_id: Uuid::new_v4(),
            correlation_id: Uuid::new_v4(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            secret_id: SecretId::new(),
            version: 2,
            occurred_at: Utc::now(),
        }
    }
}
