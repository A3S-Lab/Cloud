use super::checkpoint_queries::load_checkpoint;
use super::checkpoint_writes::validate_checkpoint_authority;
use super::queries::lock_execution;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, transaction_error, PostgresPersistenceError,
};
use crate::modules::agents::domain::{
    AgentExecutionCheckpoint, AgentExecutionCheckpointObjectCaptureReservation,
    AgentExecutionCheckpointObjectLease, AgentExecutionCheckpointObjectLeasePurpose,
    AgentExecutionCheckpointObjectReconcileDisposition,
    ClaimExpiredAgentExecutionCheckpointObjectsWrite,
    CompleteAgentExecutionCheckpointObjectCleanupWrite,
    ReconcileAgentExecutionCheckpointObjectWrite, ReserveAgentExecutionCheckpointObjectWrite,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_LEASE: &str = "select object_ref, organization_id, execution_id, checkpoint_id, object_digest, object_size_bytes, purpose, lease_id, reserved_at, lease_expires_at from agent_execution_checkpoint_object_leases";

struct CheckpointObjectLeaseRow {
    object_ref: String,
    organization_id: Uuid,
    execution_id: Uuid,
    checkpoint_id: Uuid,
    object_digest: String,
    object_size_bytes: u64,
    purpose: String,
    lease_id: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
}

impl FromRow for CheckpointObjectLeaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            object_ref: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            execution_id: decode(row, 2)?,
            checkpoint_id: decode(row, 3)?,
            object_digest: decode(row, 4)?,
            object_size_bytes: decode(row, 5)?,
            purpose: decode(row, 6)?,
            lease_id: decode(row, 7)?,
            reserved_at: decode(row, 8)?,
            lease_expires_at: decode(row, 9)?,
        })
    }
}

impl CheckpointObjectLeaseRow {
    fn lease(self) -> Result<AgentExecutionCheckpointObjectLease, RepositoryError> {
        let reference =
            crate::modules::agents::domain::AgentExecutionCheckpointObjectReference::from_inventory(
                self.object_ref,
                self.object_size_bytes,
            )
            .map_err(corrupt)?;
        if reference.digest.as_str() != self.object_digest {
            return Err(corrupt(
                "Agent checkpoint object lease digest changed its path binding",
            ));
        }
        let lease = AgentExecutionCheckpointObjectLease {
            reference,
            organization_id: crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                self.organization_id,
            ),
            execution_id: crate::modules::shared_kernel::domain::AgentExecutionId::from_uuid(
                self.execution_id,
            ),
            checkpoint_id:
                crate::modules::shared_kernel::domain::AgentExecutionCheckpointId::from_uuid(
                    self.checkpoint_id,
                ),
            purpose: AgentExecutionCheckpointObjectLeasePurpose::parse(&self.purpose)
                .map_err(corrupt)?,
            lease_id: self.lease_id,
            reserved_at: self.reserved_at,
            lease_expires_at: self.lease_expires_at,
        };
        lease.validate().map_err(corrupt)?;
        Ok(lease)
    }
}

pub(super) async fn reserve(
    executor: &PostgresExecutor,
    write: ReserveAgentExecutionCheckpointObjectWrite,
) -> Result<AgentExecutionCheckpointObjectCaptureReservation, RepositoryError> {
    write.validate().map_err(invalid)?;
    let lease_expires_at = write.lease_expires_at().map_err(invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_object(transaction, &write.checkpoint.object.object_ref).await?;
                lock_execution(
                    transaction,
                    write.checkpoint.organization_id,
                    write.checkpoint.execution_id,
                )
                .await?;
                if let Some(existing) = load_checkpoint(
                    transaction,
                    write.checkpoint.organization_id,
                    write.checkpoint.id,
                )
                .await?
                {
                    if existing != write.checkpoint {
                        return Err(RepositoryError::Conflict(
                            "Agent checkpoint identity is already bound to different content"
                                .into(),
                        )
                        .into());
                    }
                    delete_lease(transaction, &existing.object.object_ref, None).await?;
                    return Ok(AgentExecutionCheckpointObjectCaptureReservation::Committed(
                        Box::new(existing),
                    ));
                }
                validate_checkpoint_authority(transaction, &write.checkpoint).await?;
                let existing =
                    load_lease_for_update(transaction, &write.checkpoint.object.object_ref).await?;
                let lease_id = match existing.as_ref() {
                    Some(existing) if existing.reference != write.checkpoint.object => {
                        return Err(RepositoryError::Conflict(
                            "Agent checkpoint object lease changed its immutable descriptor".into(),
                        )
                        .into());
                    }
                    Some(existing)
                        if existing.purpose
                            == AgentExecutionCheckpointObjectLeasePurpose::Cleanup =>
                    {
                        return Err(RepositoryError::Conflict(format!(
                            "Agent checkpoint object cleanup must complete before capture; its current lease expires at {}",
                            existing.lease_expires_at
                        ))
                        .into());
                    }
                    Some(existing)
                        if existing.purpose
                            == AgentExecutionCheckpointObjectLeasePurpose::Capture
                            && existing.lease_expires_at > write.reserved_at =>
                    {
                        existing.lease_id
                    }
                    Some(_) | None => Uuid::now_v7(),
                };
                let lease = lease_for_checkpoint(
                    &write.checkpoint,
                    AgentExecutionCheckpointObjectLeasePurpose::Capture,
                    lease_id,
                    write.reserved_at,
                    lease_expires_at,
                )?;
                store_lease(transaction, &lease, existing.is_some()).await?;
                Ok(AgentExecutionCheckpointObjectCaptureReservation::Reserved(
                    Box::new(lease),
                ))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn reconcile(
    executor: &PostgresExecutor,
    write: ReconcileAgentExecutionCheckpointObjectWrite,
) -> Result<AgentExecutionCheckpointObjectReconcileDisposition, RepositoryError> {
    write.validate().map_err(invalid)?;
    let identity = write.reference.identity().map_err(invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_object(transaction, &write.reference.object_ref).await?;
                if let Some(checkpoint) = load_checkpoint(
                    transaction,
                    identity.organization_id,
                    identity.checkpoint_id,
                )
                .await?
                {
                    if checkpoint.object == write.reference {
                        delete_lease(transaction, &write.reference.object_ref, None).await?;
                        return Ok(AgentExecutionCheckpointObjectReconcileDisposition::Referenced);
                    }
                }
                let existing =
                    load_lease_for_update(transaction, &write.reference.object_ref).await?;
                if let Some(existing) = existing.as_ref() {
                    if existing.reference != write.reference {
                        return Err(PostgresPersistenceError::Invariant(
                            "Agent checkpoint object lease changed its inventory descriptor".into(),
                        ));
                    }
                    if existing.lease_expires_at > write.observed_at {
                        return Ok(
                            AgentExecutionCheckpointObjectReconcileDisposition::Deferred {
                                retry_not_before: existing.lease_expires_at,
                            },
                        );
                    }
                } else {
                    let observation_expires_at = write.observation_expires_at().map_err(invalid)?;
                    let lease = lease_for_reference(
                        write.reference,
                        AgentExecutionCheckpointObjectLeasePurpose::Inventory,
                        Uuid::now_v7(),
                        write.observed_at,
                        observation_expires_at,
                    )?;
                    let retry_not_before = lease.lease_expires_at;
                    store_lease(transaction, &lease, false).await?;
                    return Ok(
                        AgentExecutionCheckpointObjectReconcileDisposition::Deferred {
                            retry_not_before,
                        },
                    );
                }
                let cleanup_expires_at = write.cleanup_expires_at().map_err(invalid)?;
                let lease = lease_for_reference(
                    write.reference,
                    AgentExecutionCheckpointObjectLeasePurpose::Cleanup,
                    Uuid::now_v7(),
                    write.observed_at,
                    cleanup_expires_at,
                )?;
                store_lease(transaction, &lease, true).await?;
                Ok(
                    AgentExecutionCheckpointObjectReconcileDisposition::CleanupClaimed(Box::new(
                        lease,
                    )),
                )
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn claim_expired(
    executor: &PostgresExecutor,
    write: ClaimExpiredAgentExecutionCheckpointObjectsWrite,
) -> Result<Vec<AgentExecutionCheckpointObjectLease>, RepositoryError> {
    write.validate().map_err(invalid)?;
    let cleanup_expires_at = write.cleanup_expires_at().map_err(invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let rows = fetch_all::<CheckpointObjectLeaseRow, _>(
                    transaction,
                    sql_query::<CheckpointObjectLeaseRow>(SELECT_LEASE)
                        .append(" where lease_expires_at <= ")
                        .bind(write.claimed_at)
                        .append(" order by lease_expires_at asc, object_ref asc limit ")
                        .bind(write.limit)
                        .append(" for update skip locked"),
                )
                .await?;
                let mut claims = Vec::with_capacity(rows.len());
                for row in rows {
                    let existing = row.lease()?;
                    if load_checkpoint(
                        transaction,
                        existing.organization_id,
                        existing.checkpoint_id,
                    )
                    .await?
                    .is_some_and(|checkpoint| checkpoint.object == existing.reference)
                    {
                        delete_lease(
                            transaction,
                            &existing.reference.object_ref,
                            Some(existing.lease_id),
                        )
                        .await?;
                        continue;
                    }
                    let lease = lease_for_reference(
                        existing.reference,
                        AgentExecutionCheckpointObjectLeasePurpose::Cleanup,
                        Uuid::now_v7(),
                        write.claimed_at,
                        cleanup_expires_at,
                    )?;
                    store_lease(transaction, &lease, true).await?;
                    claims.push(lease);
                }
                Ok(claims)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn complete_cleanup(
    executor: &PostgresExecutor,
    write: CompleteAgentExecutionCheckpointObjectCleanupWrite,
) -> Result<(), RepositoryError> {
    write.validate().map_err(invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_object(transaction, &write.lease.reference.object_ref).await?;
                let Some(existing) =
                    load_lease_for_update(transaction, &write.lease.reference.object_ref).await?
                else {
                    return Ok(());
                };
                if existing != write.lease
                    || existing.purpose != AgentExecutionCheckpointObjectLeasePurpose::Cleanup
                {
                    return Err(RepositoryError::Conflict(
                        "Agent checkpoint object cleanup lease is stale".into(),
                    )
                    .into());
                }
                if load_checkpoint(
                    transaction,
                    existing.organization_id,
                    existing.checkpoint_id,
                )
                .await?
                .is_some_and(|checkpoint| checkpoint.object == existing.reference)
                {
                    return Err(RepositoryError::Conflict(
                        "Agent checkpoint object became referenced during cleanup".into(),
                    )
                    .into());
                }
                delete_lease(
                    transaction,
                    &existing.reference.object_ref,
                    Some(existing.lease_id),
                )
                .await?;
                Ok(())
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn lock_object(
    transaction: &PostgresTransaction,
    object_ref: &str,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock("a3s.cloud.agent-checkpoint-object", object_ref)
        .await?;
    Ok(())
}

pub(super) async fn load_lease_for_update(
    transaction: &PostgresTransaction,
    object_ref: &str,
) -> Result<Option<AgentExecutionCheckpointObjectLease>, PostgresPersistenceError> {
    fetch_optional::<CheckpointObjectLeaseRow, _>(
        transaction,
        sql_query::<CheckpointObjectLeaseRow>(SELECT_LEASE)
            .append(" where object_ref = ")
            .bind(object_ref)
            .append(" for update"),
    )
    .await?
    .map(CheckpointObjectLeaseRow::lease)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn delete_lease(
    transaction: &PostgresTransaction,
    object_ref: &str,
    lease_id: Option<Uuid>,
) -> Result<(), PostgresPersistenceError> {
    let query =
        sql_query::<()>("delete from agent_execution_checkpoint_object_leases where object_ref = ")
            .bind(object_ref);
    let query = match lease_id {
        Some(lease_id) => query.append(" and lease_id = ").bind(lease_id),
        None => query,
    };
    let rows = execute(transaction, query).await?;
    if rows > 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "deleting Agent checkpoint object lease affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_lease(
    transaction: &PostgresTransaction,
    lease: &AgentExecutionCheckpointObjectLease,
    replace: bool,
) -> Result<(), PostgresPersistenceError> {
    lease
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let query = if replace {
        sql_query::<()>("update agent_execution_checkpoint_object_leases set organization_id = ")
            .bind(lease.organization_id.as_uuid())
            .append(", execution_id = ")
            .bind(lease.execution_id.as_uuid())
            .append(", checkpoint_id = ")
            .bind(lease.checkpoint_id.as_uuid())
            .append(", object_digest = ")
            .bind(lease.reference.digest.as_str())
            .append(", object_size_bytes = ")
            .bind(lease.reference.size_bytes)
            .append(", purpose = ")
            .bind(lease.purpose.as_str())
            .append(", lease_id = ")
            .bind(lease.lease_id)
            .append(", reserved_at = ")
            .bind(lease.reserved_at)
            .append(", lease_expires_at = ")
            .bind(lease.lease_expires_at)
            .append(" where object_ref = ")
            .bind(lease.reference.object_ref.as_str())
    } else {
        sql_query::<()>(
            "insert into agent_execution_checkpoint_object_leases (object_ref, organization_id, execution_id, checkpoint_id, object_digest, object_size_bytes, purpose, lease_id, reserved_at, lease_expires_at) values (",
        )
        .bind(lease.reference.object_ref.as_str())
        .append(", ")
        .bind(lease.organization_id.as_uuid())
        .append(", ")
        .bind(lease.execution_id.as_uuid())
        .append(", ")
        .bind(lease.checkpoint_id.as_uuid())
        .append(", ")
        .bind(lease.reference.digest.as_str())
        .append(", ")
        .bind(lease.reference.size_bytes)
        .append(", ")
        .bind(lease.purpose.as_str())
        .append(", ")
        .bind(lease.lease_id)
        .append(", ")
        .bind(lease.reserved_at)
        .append(", ")
        .bind(lease.lease_expires_at)
        .append(")")
    };
    let rows = execute(transaction, query).await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "persisting Agent checkpoint object lease affected {rows} rows"
        )));
    }
    Ok(())
}

fn lease_for_checkpoint(
    checkpoint: &AgentExecutionCheckpoint,
    purpose: AgentExecutionCheckpointObjectLeasePurpose,
    lease_id: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> Result<AgentExecutionCheckpointObjectLease, PostgresPersistenceError> {
    lease_for_reference(
        checkpoint.object.clone(),
        purpose,
        lease_id,
        reserved_at,
        lease_expires_at,
    )
}

fn lease_for_reference(
    reference: crate::modules::agents::domain::AgentExecutionCheckpointObjectReference,
    purpose: AgentExecutionCheckpointObjectLeasePurpose,
    lease_id: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> Result<AgentExecutionCheckpointObjectLease, PostgresPersistenceError> {
    let identity = reference.identity().map_err(invalid)?;
    let lease = AgentExecutionCheckpointObjectLease {
        reference,
        organization_id: identity.organization_id,
        execution_id: identity.execution_id,
        checkpoint_id: identity.checkpoint_id,
        purpose,
        lease_id,
        reserved_at,
        lease_expires_at,
    };
    lease.validate().map_err(invalid)?;
    Ok(lease)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn invalid(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "invalid Agent checkpoint object lease write: {}",
        message.into()
    ))
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(format!(
        "stored Agent checkpoint object lease is corrupt: {}",
        message.into()
    ))
}
