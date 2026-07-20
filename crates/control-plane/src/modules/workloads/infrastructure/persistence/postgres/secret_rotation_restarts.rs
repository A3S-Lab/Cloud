use super::{create, queries};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::SecretChanged;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, IdempotencyRequest, OperationId, OrganizationId,
    RepositoryError, SecretId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{Deployment, WorkloadDesiredState};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, DeploymentBundle, SecretRotation, SecretRotationCompletion,
    SecretRotationReconciliation,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor, PostgresTransaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

type RotationRow = (Uuid, Uuid, Uuid, DateTime<Utc>, Uuid, serde_json::Value);

pub(super) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<SecretRotation>, RepositoryError> {
    let limit = checked_limit(limit, "Secret rotation event")?;
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<RotationRow>(
                "select e.event_id, e.organization_id, e.aggregate_id, e.occurred_at, e.correlation_id, e.payload from outbox_events e left join secret_rotation_reconciliations r on r.secret_event_id = e.event_id where e.event_key = 'secret.version.created' and r.secret_event_id is null order by e.occurred_at asc, e.event_id asc limit ",
            )
            .bind(limit),
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
    workload_limit: i64,
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
        sql_query::<RotationRow>(
            "select event_id, organization_id, aggregate_id, occurred_at, correlation_id, payload from outbox_events where event_id = ",
        )
        .bind(rotation.event_id)
        .append(" and event_key = 'secret.version.created'"),
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
        sql_query::<(u64, String, String, Uuid, Uuid)>(
            "select s.current_version, s.state, v.state, s.project_id, s.environment_id from secrets s join secret_versions v on v.secret_id = s.id and v.version = ",
        )
        .bind(rotation.version)
        .append(" where s.organization_id = ")
        .bind(rotation.organization_id.as_uuid())
        .append(" and s.id = ")
        .bind(rotation.secret_id.as_uuid())
        .append(" for update of s, v"),
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
        let generation = next_generation(transaction, workload.id).await?;
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
            WorkflowIdentity::new("cloud.deployment", "2").map_err(|error| {
                PostgresPersistenceError::Invariant(format!(
                    "could not create Secret rotation workflow identity: {error}"
                ))
            })?,
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
        let response = create::deployment_in_transaction(
            transaction,
            CreateDeploymentBundle {
                workload,
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
    limit: i64,
) -> Result<Vec<(Uuid, Uuid)>, PostgresPersistenceError> {
    let rows = fetch_all::<(Uuid, Option<Uuid>), _>(
        transaction,
        affected_workloads_query(rotation)
            .append(
                " and not exists (select 1 from deployments pending where pending.workload_id = w.id and pending.status not in ('active', 'failed', 'orphaned', 'cancelled')) order by w.updated_at asc, w.id asc for update of w skip locked limit ",
            )
            .bind(limit),
    )
    .await?;
    rows.into_iter()
        .map(|(workload_id, revision_id)| {
            revision_id
                .map(|revision_id| (workload_id, revision_id))
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "affected workload omitted its active revision".into(),
                    )
                })
        })
        .collect()
}

async fn affected_workload_count(
    transaction: &PostgresTransaction,
    rotation: &SecretRotation,
) -> Result<i64, PostgresPersistenceError> {
    fetch_optional::<i64, _>(
        transaction,
        sql_query::<i64>(
            "select count(*) from workloads w join workload_revisions r on r.workload_id = w.id and r.id = w.active_revision_id where w.organization_id = ",
        )
        .bind(rotation.organization_id.as_uuid())
        .append(" and w.project_id = ")
        .bind(rotation.project_id.as_uuid())
        .append(" and w.environment_id = ")
        .bind(rotation.environment_id.as_uuid())
        .append(" and w.desired_state = 'running' and w.active_revision_id is not null and exists (select 1 from deployments active where active.workload_id = w.id and active.revision_id = w.active_revision_id and active.status = 'active') and not exists (select 1 from secret_rotation_restarts handled where handled.secret_event_id = ")
        .bind(rotation.event_id)
        .append(" and handled.workload_id = w.id) and exists (select 1 from jsonb_array_elements(r.template_request -> 'secrets') binding where binding ->> 'secret_id' = ")
        .bind(rotation.secret_id.to_string())
        .append(" and (binding ->> 'version')::bigint < ")
        .bind(rotation.version)
        .append(")"),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Secret rotation affected-workload count returned no row".into(),
        )
    })
}

fn affected_workloads_query(rotation: &SecretRotation) -> a3s_orm::SqlQuery<(Uuid, Option<Uuid>)> {
    sql_query::<(Uuid, Option<Uuid>)>(
        "select w.id, w.active_revision_id from workloads w join workload_revisions r on r.workload_id = w.id and r.id = w.active_revision_id where w.organization_id = ",
    )
    .bind(rotation.organization_id.as_uuid())
    .append(" and w.project_id = ")
    .bind(rotation.project_id.as_uuid())
    .append(" and w.environment_id = ")
    .bind(rotation.environment_id.as_uuid())
    .append(" and w.desired_state = 'running' and w.active_revision_id is not null and exists (select 1 from deployments active where active.workload_id = w.id and active.revision_id = w.active_revision_id and active.status = 'active') and not exists (select 1 from secret_rotation_restarts handled where handled.secret_event_id = ")
    .bind(rotation.event_id)
    .append(" and handled.workload_id = w.id) and exists (select 1 from jsonb_array_elements(r.template_request -> 'secrets') binding where binding ->> 'secret_id' = ")
    .bind(rotation.secret_id.to_string())
    .append(" and (binding ->> 'version')::bigint < ")
    .bind(rotation.version)
    .append(")")
}

async fn next_generation(
    transaction: &PostgresTransaction,
    workload_id: WorkloadId,
) -> Result<u64, PostgresPersistenceError> {
    let latest = fetch_optional::<Option<i64>, _>(
        transaction,
        sql_query::<Option<i64>>(
            "select max(generation) from workload_revisions where workload_id = ",
        )
        .bind(workload_id.as_uuid()),
    )
    .await?
    .flatten()
    .unwrap_or_default();
    u64::try_from(latest)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| PostgresPersistenceError::Invariant("workload generation overflowed".into()))
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
            sql_query::<()>(
                "insert into secret_rotation_restarts (secret_event_id, organization_id, secret_id, secret_version, workload_id, source_revision_id, target_revision_id, deployment_id, operation_id, created_at) values (",
            )
            .bind(rotation.event_id)
            .append(", ")
            .bind(rotation.organization_id.as_uuid())
            .append(", ")
            .bind(rotation.secret_id.as_uuid())
            .append(", ")
            .bind(rotation.version)
            .append(", ")
            .bind(response.workload.id.as_uuid())
            .append(", ")
            .bind(source_revision_id.as_uuid())
            .append(", ")
            .bind(response.revision.id.as_uuid())
            .append(", ")
            .bind(response.deployment.id.as_uuid())
            .append(", ")
            .bind(response.operation.id.as_uuid())
            .append(", ")
            .bind(created_at)
            .append(")"),
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
        sql_query::<i64>("select count(*) from secret_rotation_restarts where secret_event_id = ")
            .bind(event_id),
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
            sql_query::<()>(
                "insert into secret_rotation_reconciliations (secret_event_id, organization_id, secret_id, secret_version, outcome, restart_count, reconciled_at) values (",
            )
            .bind(rotation.event_id)
            .append(", ")
            .bind(rotation.organization_id.as_uuid())
            .append(", ")
            .bind(rotation.secret_id.as_uuid())
            .append(", ")
            .bind(rotation.version)
            .append(", ")
            .bind(completion_name(outcome))
            .append(", ")
            .bind(restart_count)
            .append(", ")
            .bind(reconciled_at)
            .append(")"),
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
        sql_query::<String>(
            "select outcome from secret_rotation_reconciliations where secret_event_id = ",
        )
        .bind(event_id),
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
    let locked = fetch_optional::<i32, _>(
        transaction,
        sql_query::<i32>("select 1 from (select pg_advisory_xact_lock(hashtext(")
            .bind("cloud.secret-rotation-restart")
            .append("), hashtext(")
            .bind(event_id.to_string())
            .append("))) as locked"),
    )
    .await?;
    if locked == Some(1) {
        Ok(())
    } else {
        Err(PostgresPersistenceError::Invariant(
            "Secret rotation advisory lock did not return a row".into(),
        ))
    }
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

fn checked_limit(limit: usize, label: &str) -> Result<i64, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(format!(
            "{label} limit must be between 1 and 10000"
        )));
    }
    i64::try_from(limit).map_err(|_| RepositoryError::Conflict(format!("{label} limit is invalid")))
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
