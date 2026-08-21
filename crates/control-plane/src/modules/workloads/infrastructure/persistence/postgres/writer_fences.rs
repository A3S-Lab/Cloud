use super::schema::WorkloadWriterFenceReceipts;
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError,
    Sha256Digest, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    ManagedOwnerKind, ManagedOwnerReference, WorkloadWriterFenceReceipt,
    WorkloadWriterFenceReceiptSpec, WORKLOAD_WRITER_FENCE_RECEIPT_SCHEMA,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct ReceiptSelection;

impl Selection for ReceiptSelection {
    type Output = ReceiptRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkloadWriterFenceReceipts::organization_id().expression(),
            WorkloadWriterFenceReceipts::project_id().expression(),
            WorkloadWriterFenceReceipts::environment_id().expression(),
            WorkloadWriterFenceReceipts::workload_id().expression(),
            WorkloadWriterFenceReceipts::workload_revision_id().expression(),
            WorkloadWriterFenceReceipts::workload_revision_generation().expression(),
            WorkloadWriterFenceReceipts::replica_id().expression(),
            WorkloadWriterFenceReceipts::replica_ordinal().expression(),
            WorkloadWriterFenceReceipts::writer_epoch().expression(),
            WorkloadWriterFenceReceipts::member_id().expression(),
            WorkloadWriterFenceReceipts::placement_generation().expression(),
            WorkloadWriterFenceReceipts::managed_owner_kind().expression(),
            WorkloadWriterFenceReceipts::managed_owner_id().expression(),
            WorkloadWriterFenceReceipts::managed_owner_generation().expression(),
            WorkloadWriterFenceReceipts::managed_owner_spec_digest().expression(),
            WorkloadWriterFenceReceipts::node_id().expression(),
            WorkloadWriterFenceReceipts::runtime_unit_id().expression(),
            WorkloadWriterFenceReceipts::command_id().expression(),
            WorkloadWriterFenceReceipts::command_kind().expression(),
            WorkloadWriterFenceReceipts::command_payload_digest().expression(),
            WorkloadWriterFenceReceipts::acknowledgement_digest().expression(),
            WorkloadWriterFenceReceipts::continuation_operation_id().expression(),
            WorkloadWriterFenceReceipts::receipt_schema().expression(),
            WorkloadWriterFenceReceipts::receipt_digest().expression(),
            WorkloadWriterFenceReceipts::fenced_at().expression(),
        ]
    }
}

struct ReceiptRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    workload_revision_generation: u64,
    replica_id: Uuid,
    replica_ordinal: u32,
    writer_epoch: u64,
    member_id: Uuid,
    placement_generation: u64,
    managed_owner_kind: String,
    managed_owner_id: Uuid,
    managed_owner_generation: u64,
    managed_owner_spec_digest: String,
    node_id: Uuid,
    runtime_unit_id: String,
    command_id: Uuid,
    command_kind: String,
    command_payload_digest: String,
    acknowledgement_digest: String,
    continuation_operation_id: Uuid,
    receipt_schema: String,
    receipt_digest: String,
    fenced_at: DateTime<Utc>,
}

impl FromRow for ReceiptRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            workload_id: decode(row, 3)?,
            workload_revision_id: decode(row, 4)?,
            workload_revision_generation: decode(row, 5)?,
            replica_id: decode(row, 6)?,
            replica_ordinal: decode(row, 7)?,
            writer_epoch: decode(row, 8)?,
            member_id: decode(row, 9)?,
            placement_generation: decode(row, 10)?,
            managed_owner_kind: decode(row, 11)?,
            managed_owner_id: decode(row, 12)?,
            managed_owner_generation: decode(row, 13)?,
            managed_owner_spec_digest: decode(row, 14)?,
            node_id: decode(row, 15)?,
            runtime_unit_id: decode(row, 16)?,
            command_id: decode(row, 17)?,
            command_kind: decode(row, 18)?,
            command_payload_digest: decode(row, 19)?,
            acknowledgement_digest: decode(row, 20)?,
            continuation_operation_id: decode(row, 21)?,
            receipt_schema: decode(row, 22)?,
            receipt_digest: decode(row, 23)?,
            fenced_at: decode(row, 24)?,
        })
    }
}

pub(super) async fn insert(
    transaction: &PostgresTransaction,
    receipt: &WorkloadWriterFenceReceipt,
) -> Result<(), PostgresPersistenceError> {
    receipt.validate().map_err(RepositoryError::Conflict)?;
    let spec = receipt.spec();
    let result = execute(
        transaction,
        insert_into::<WorkloadWriterFenceReceipts>()
            .value(
                WorkloadWriterFenceReceipts::organization_id(),
                spec.organization_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::project_id(),
                spec.project_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::environment_id(),
                spec.environment_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::workload_id(),
                spec.workload_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::workload_revision_id(),
                spec.workload_revision_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::workload_revision_generation(),
                spec.workload_revision_generation,
            )
            .value(
                WorkloadWriterFenceReceipts::replica_id(),
                spec.replica_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::replica_ordinal(),
                spec.replica_ordinal,
            )
            .value(
                WorkloadWriterFenceReceipts::writer_epoch(),
                spec.writer_epoch,
            )
            .value(
                WorkloadWriterFenceReceipts::member_id(),
                spec.member_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::placement_generation(),
                spec.placement_generation,
            )
            .value(
                WorkloadWriterFenceReceipts::managed_owner_kind(),
                spec.managed_owner.kind().as_str(),
            )
            .value(
                WorkloadWriterFenceReceipts::managed_owner_id(),
                spec.managed_owner.owner_id(),
            )
            .value(
                WorkloadWriterFenceReceipts::managed_owner_generation(),
                spec.managed_owner.owner_generation(),
            )
            .value(
                WorkloadWriterFenceReceipts::managed_owner_spec_digest(),
                spec.managed_owner.owner_spec_digest(),
            )
            .value(
                WorkloadWriterFenceReceipts::node_id(),
                spec.node_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::runtime_unit_id(),
                spec.runtime_unit_id.as_str(),
            )
            .value(
                WorkloadWriterFenceReceipts::command_id(),
                spec.command_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::command_kind(),
                "runtime_remove",
            )
            .value(
                WorkloadWriterFenceReceipts::command_payload_digest(),
                spec.command_payload_digest.as_str(),
            )
            .value(
                WorkloadWriterFenceReceipts::acknowledgement_digest(),
                spec.acknowledgement_digest.as_str(),
            )
            .value(
                WorkloadWriterFenceReceipts::continuation_operation_id(),
                spec.continuation_operation_id.as_uuid(),
            )
            .value(
                WorkloadWriterFenceReceipts::receipt_schema(),
                WORKLOAD_WRITER_FENCE_RECEIPT_SCHEMA,
            )
            .value(
                WorkloadWriterFenceReceipts::receipt_digest(),
                receipt.digest().as_str(),
            )
            .value(WorkloadWriterFenceReceipts::fenced_at(), spec.fenced_at),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("Workload writer-fence receipt", rows),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::Conflict(
            "Workload writer-fence receipt references stale authority".into(),
        )
        .into()),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Workload writer-fence receipt identity is already in use".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn latest(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Option<WorkloadWriterFenceReceipt>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<WorkloadWriterFenceReceipts>()
                .select(ReceiptSelection)
                .filter(
                    WorkloadWriterFenceReceipts::organization_id().eq(organization_id.as_uuid()),
                )
                .filter(WorkloadWriterFenceReceipts::workload_id().eq(workload_id.as_uuid()))
                .order_by(
                    WorkloadWriterFenceReceipts::writer_epoch(),
                    OrderDirection::Desc,
                )
                .limit(1),
        )
        .await
        .map_err(storage)?
        .map(decode_receipt)
        .transpose()
}

pub(super) async fn find_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    writer_epoch: u64,
) -> Result<Option<WorkloadWriterFenceReceipt>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<WorkloadWriterFenceReceipts>()
            .select(ReceiptSelection)
            .filter(WorkloadWriterFenceReceipts::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadWriterFenceReceipts::workload_id().eq(workload_id.as_uuid()))
            .filter(WorkloadWriterFenceReceipts::writer_epoch().eq(writer_epoch)),
    )
    .await?
    .map(decode_receipt)
    .transpose()
    .map_err(Into::into)
}

fn decode_receipt(row: ReceiptRow) -> Result<WorkloadWriterFenceReceipt, RepositoryError> {
    if row.receipt_schema != WORKLOAD_WRITER_FENCE_RECEIPT_SCHEMA {
        return Err(RepositoryError::Storage(
            "stored Workload writer-fence receipt schema is unsupported".into(),
        ));
    }
    if row.command_kind != "runtime_remove" {
        return Err(RepositoryError::Storage(
            "stored Workload writer-fence command kind is unsupported".into(),
        ));
    }
    let managed_owner = ManagedOwnerReference::new(
        ManagedOwnerKind::parse(row.managed_owner_kind).map_err(RepositoryError::Storage)?,
        row.managed_owner_id,
        row.managed_owner_generation,
        row.managed_owner_spec_digest,
    )
    .map_err(RepositoryError::Storage)?;
    WorkloadWriterFenceReceipt::restore(
        WorkloadWriterFenceReceiptSpec {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            project_id: ProjectId::from_uuid(row.project_id),
            environment_id: EnvironmentId::from_uuid(row.environment_id),
            workload_id: WorkloadId::from_uuid(row.workload_id),
            workload_revision_id: WorkloadRevisionId::from_uuid(row.workload_revision_id),
            workload_revision_generation: row.workload_revision_generation,
            replica_id: WorkloadReplicaId::from_uuid(row.replica_id),
            replica_ordinal: row.replica_ordinal,
            writer_epoch: row.writer_epoch,
            member_id: WorkloadReplicaMemberId::from_uuid(row.member_id),
            placement_generation: row.placement_generation,
            managed_owner,
            node_id: NodeId::from_uuid(row.node_id),
            runtime_unit_id: row.runtime_unit_id,
            command_id: NodeCommandId::from_uuid(row.command_id),
            command_payload_digest: Sha256Digest::parse(row.command_payload_digest)
                .map_err(RepositoryError::Storage)?,
            acknowledgement_digest: Sha256Digest::parse(row.acknowledgement_digest)
                .map_err(RepositoryError::Storage)?,
            continuation_operation_id: OperationId::from_uuid(row.continuation_operation_id),
            fenced_at: row.fenced_at,
        },
        &row.receipt_digest,
    )
    .map_err(RepositoryError::Storage)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
