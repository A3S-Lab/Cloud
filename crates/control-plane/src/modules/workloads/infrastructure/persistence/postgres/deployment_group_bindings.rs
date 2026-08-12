use super::schema::{
    DeploymentPlacementGroupBindings, DeploymentReplicaMemberBindings, WorkloadReplicaMembers,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadPlacementGroupId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    DeploymentPlacementGroupBinding, DeploymentReplicaBinding,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct MemberBindingSelection;
struct GroupBindingSelection;

impl Selection for MemberBindingSelection {
    type Output = MemberBindingRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            DeploymentReplicaMemberBindings::deployment_id().expression(),
            DeploymentReplicaMemberBindings::organization_id().expression(),
            DeploymentReplicaMemberBindings::project_id().expression(),
            DeploymentReplicaMemberBindings::environment_id().expression(),
            DeploymentReplicaMemberBindings::workload_id().expression(),
            DeploymentReplicaMemberBindings::revision_id().expression(),
            DeploymentReplicaMemberBindings::replica_id().expression(),
            DeploymentReplicaMemberBindings::replica_generation().expression(),
            DeploymentReplicaMemberBindings::member_id().expression(),
            DeploymentReplicaMemberBindings::node_id().expression(),
            DeploymentReplicaMemberBindings::placement_generation().expression(),
            DeploymentReplicaMemberBindings::runtime_unit_id().expression(),
            DeploymentReplicaMemberBindings::runtime_generation().expression(),
            DeploymentReplicaMemberBindings::created_at().expression(),
            DeploymentReplicaMemberBindings::updated_at().expression(),
        ]
    }
}

impl Selection for GroupBindingSelection {
    type Output = GroupBindingRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            DeploymentPlacementGroupBindings::deployment_id().expression(),
            DeploymentPlacementGroupBindings::organization_id().expression(),
            DeploymentPlacementGroupBindings::project_id().expression(),
            DeploymentPlacementGroupBindings::environment_id().expression(),
            DeploymentPlacementGroupBindings::workload_id().expression(),
            DeploymentPlacementGroupBindings::revision_id().expression(),
            DeploymentPlacementGroupBindings::revision_generation().expression(),
            DeploymentPlacementGroupBindings::replica_id().expression(),
            DeploymentPlacementGroupBindings::replica_generation().expression(),
            DeploymentPlacementGroupBindings::group_id().expression(),
            DeploymentPlacementGroupBindings::group_plan_digest().expression(),
            DeploymentPlacementGroupBindings::member_count().expression(),
            DeploymentPlacementGroupBindings::created_at().expression(),
            DeploymentPlacementGroupBindings::updated_at().expression(),
        ]
    }
}

struct MemberBindingRow {
    deployment_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    revision_id: Uuid,
    replica_id: Uuid,
    replica_generation: u64,
    member_id: Uuid,
    node_id: Option<Uuid>,
    placement_generation: u64,
    runtime_unit_id: String,
    runtime_generation: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct GroupBindingRow {
    deployment_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    revision_id: Uuid,
    revision_generation: u64,
    replica_id: Uuid,
    replica_generation: u64,
    group_id: Uuid,
    group_plan_digest: String,
    member_count: u32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for MemberBindingRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            deployment_id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            workload_id: decode(row, 4)?,
            revision_id: decode(row, 5)?,
            replica_id: decode(row, 6)?,
            replica_generation: decode(row, 7)?,
            member_id: decode(row, 8)?,
            node_id: decode(row, 9)?,
            placement_generation: decode(row, 10)?,
            runtime_unit_id: decode(row, 11)?,
            runtime_generation: decode(row, 12)?,
            created_at: decode(row, 13)?,
            updated_at: decode(row, 14)?,
        })
    }
}

impl FromRow for GroupBindingRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            deployment_id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            workload_id: decode(row, 4)?,
            revision_id: decode(row, 5)?,
            revision_generation: decode(row, 6)?,
            replica_id: decode(row, 7)?,
            replica_generation: decode(row, 8)?,
            group_id: decode(row, 9)?,
            group_plan_digest: decode(row, 10)?,
            member_count: decode(row, 11)?,
            created_at: decode(row, 12)?,
            updated_at: decode(row, 13)?,
        })
    }
}

pub(super) async fn list_member_bindings(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Vec<DeploymentReplicaBinding>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(member_bindings_query(organization_id, deployment_id))
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(MemberBindingRow::binding)
        .collect()
}

pub(super) async fn list_member_bindings_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
    for_update: bool,
) -> Result<Vec<DeploymentReplicaBinding>, PostgresPersistenceError> {
    let query = member_bindings_query(organization_id, deployment_id);
    let rows = if for_update {
        fetch_all(transaction, query.for_update()).await?
    } else {
        fetch_all(transaction, query).await?
    };
    rows.into_iter()
        .map(MemberBindingRow::binding)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(super) async fn find_member_binding_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
    member_id: WorkloadReplicaMemberId,
) -> Result<Option<DeploymentReplicaBinding>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<DeploymentReplicaMemberBindings>()
            .select(MemberBindingSelection)
            .filter(
                DeploymentReplicaMemberBindings::organization_id().eq(organization_id.as_uuid()),
            )
            .filter(DeploymentReplicaMemberBindings::deployment_id().eq(deployment_id.as_uuid()))
            .filter(DeploymentReplicaMemberBindings::member_id().eq(member_id.as_uuid())),
    )
    .await?
    .map(MemberBindingRow::binding)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn find_group_binding(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<DeploymentPlacementGroupBinding, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(group_binding_query(organization_id, deployment_id))
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(GroupBindingRow::binding)
}

pub(super) async fn find_group_binding_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
    for_update: bool,
) -> Result<Option<DeploymentPlacementGroupBinding>, PostgresPersistenceError> {
    let query = group_binding_query(organization_id, deployment_id);
    let row = if for_update {
        fetch_optional(transaction, query.for_update()).await?
    } else {
        fetch_optional(transaction, query).await?
    };
    row.map(GroupBindingRow::binding)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn insert_member_binding(
    transaction: &PostgresTransaction,
    binding: &DeploymentReplicaBinding,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        insert_into::<DeploymentReplicaMemberBindings>()
            .value(
                DeploymentReplicaMemberBindings::deployment_id(),
                binding.deployment_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::organization_id(),
                binding.organization_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::project_id(),
                binding.project_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::environment_id(),
                binding.environment_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::workload_id(),
                binding.workload_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::revision_id(),
                binding.revision_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::replica_id(),
                binding.replica_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::replica_generation(),
                binding.replica_generation,
            )
            .value(
                DeploymentReplicaMemberBindings::member_id(),
                binding.member_id.as_uuid(),
            )
            .value(
                DeploymentReplicaMemberBindings::node_id(),
                binding.node_id.map(NodeId::as_uuid),
            )
            .value(
                DeploymentReplicaMemberBindings::placement_generation(),
                binding.placement_generation,
            )
            .value(
                DeploymentReplicaMemberBindings::runtime_unit_id(),
                binding.runtime_unit_id.as_str(),
            )
            .value(
                DeploymentReplicaMemberBindings::runtime_generation(),
                binding.runtime_generation,
            )
            .value(
                DeploymentReplicaMemberBindings::created_at(),
                binding.created_at,
            )
            .value(
                DeploymentReplicaMemberBindings::updated_at(),
                binding.updated_at,
            ),
    )
    .await?;
    require_one_row("Deployment replica member binding", rows)
}

pub(super) async fn persist_member_assignment(
    transaction: &PostgresTransaction,
    binding: &DeploymentReplicaBinding,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<DeploymentReplicaMemberBindings>()
            .set(
                DeploymentReplicaMemberBindings::node_id(),
                binding.node_id.map(NodeId::as_uuid),
            )
            .set(
                DeploymentReplicaMemberBindings::placement_generation(),
                binding.placement_generation,
            )
            .set(
                DeploymentReplicaMemberBindings::updated_at(),
                binding.updated_at,
            )
            .filter(
                DeploymentReplicaMemberBindings::deployment_id()
                    .eq(binding.deployment_id.as_uuid()),
            )
            .filter(DeploymentReplicaMemberBindings::member_id().eq(binding.member_id.as_uuid())),
    )
    .await?;
    require_one_row("Deployment replica member placement binding", rows)
}

pub(super) async fn insert_group_binding(
    transaction: &PostgresTransaction,
    binding: &DeploymentPlacementGroupBinding,
) -> Result<(), PostgresPersistenceError> {
    binding
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        insert_into::<DeploymentPlacementGroupBindings>()
            .value(
                DeploymentPlacementGroupBindings::deployment_id(),
                binding.deployment_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::organization_id(),
                binding.organization_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::project_id(),
                binding.project_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::environment_id(),
                binding.environment_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::workload_id(),
                binding.workload_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::revision_id(),
                binding.revision_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::revision_generation(),
                binding.revision_generation,
            )
            .value(
                DeploymentPlacementGroupBindings::replica_id(),
                binding.replica_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::replica_generation(),
                binding.replica_generation,
            )
            .value(
                DeploymentPlacementGroupBindings::group_id(),
                binding.group_id.as_uuid(),
            )
            .value(
                DeploymentPlacementGroupBindings::group_plan_digest(),
                binding.group_plan_digest.as_str(),
            )
            .value(
                DeploymentPlacementGroupBindings::member_count(),
                binding.member_count,
            )
            .value(
                DeploymentPlacementGroupBindings::created_at(),
                binding.created_at,
            )
            .value(
                DeploymentPlacementGroupBindings::updated_at(),
                binding.updated_at,
            ),
    )
    .await?;
    require_one_row("Deployment placement-group binding", rows)
}

fn member_bindings_query(
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> a3s_orm::query::SelectQuery<DeploymentReplicaMemberBindings, MemberBindingRow> {
    select_from::<DeploymentReplicaMemberBindings>()
        .select(MemberBindingSelection)
        .inner_join::<WorkloadReplicaMembers>(
            WorkloadReplicaMembers::organization_id()
                .eq_column(DeploymentReplicaMemberBindings::organization_id())
                .and(
                    WorkloadReplicaMembers::replica_id()
                        .eq_column(DeploymentReplicaMemberBindings::replica_id()),
                )
                .and(
                    WorkloadReplicaMembers::id()
                        .eq_column(DeploymentReplicaMemberBindings::member_id()),
                ),
        )
        .filter(DeploymentReplicaMemberBindings::organization_id().eq(organization_id.as_uuid()))
        .filter(DeploymentReplicaMemberBindings::deployment_id().eq(deployment_id.as_uuid()))
        .order_by(WorkloadReplicaMembers::ordinal(), OrderDirection::Asc)
        .order_by(
            DeploymentReplicaMemberBindings::member_id(),
            OrderDirection::Asc,
        )
}

fn group_binding_query(
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> a3s_orm::query::SelectQuery<DeploymentPlacementGroupBindings, GroupBindingRow> {
    select_from::<DeploymentPlacementGroupBindings>()
        .select(GroupBindingSelection)
        .filter(DeploymentPlacementGroupBindings::organization_id().eq(organization_id.as_uuid()))
        .filter(DeploymentPlacementGroupBindings::deployment_id().eq(deployment_id.as_uuid()))
}

impl MemberBindingRow {
    fn binding(self) -> Result<DeploymentReplicaBinding, RepositoryError> {
        validate_member_binding_row(&self)?;
        Ok(DeploymentReplicaBinding {
            deployment_id: DeploymentId::from_uuid(self.deployment_id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            revision_id: WorkloadRevisionId::from_uuid(self.revision_id),
            replica_id: WorkloadReplicaId::from_uuid(self.replica_id),
            replica_generation: self.replica_generation,
            member_id: WorkloadReplicaMemberId::from_uuid(self.member_id),
            node_id: self.node_id.map(NodeId::from_uuid),
            placement_generation: self.placement_generation,
            runtime_unit_id: self.runtime_unit_id,
            runtime_generation: self.runtime_generation,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

impl GroupBindingRow {
    fn binding(self) -> Result<DeploymentPlacementGroupBinding, RepositoryError> {
        let binding = DeploymentPlacementGroupBinding {
            deployment_id: DeploymentId::from_uuid(self.deployment_id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            revision_id: WorkloadRevisionId::from_uuid(self.revision_id),
            revision_generation: self.revision_generation,
            replica_id: WorkloadReplicaId::from_uuid(self.replica_id),
            replica_generation: self.replica_generation,
            group_id: WorkloadPlacementGroupId::from_uuid(self.group_id),
            group_plan_digest: self.group_plan_digest,
            member_count: self.member_count,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        binding.validate().map_err(RepositoryError::Storage)?;
        Ok(binding)
    }
}

fn validate_member_binding_row(row: &MemberBindingRow) -> Result<(), RepositoryError> {
    if row.deployment_id.is_nil()
        || row.organization_id.is_nil()
        || row.project_id.is_nil()
        || row.environment_id.is_nil()
        || row.workload_id.is_nil()
        || row.revision_id.is_nil()
        || row.replica_id.is_nil()
        || row.replica_generation == 0
        || row.member_id.is_nil()
        || row.runtime_unit_id.trim().is_empty()
        || row.runtime_unit_id.len() > 512
        || row.runtime_unit_id.contains(['\0', '\r', '\n'])
        || row.runtime_generation != row.replica_generation
        || row.node_id.is_some() && row.placement_generation == 0
        || row.updated_at < row.created_at
    {
        return Err(RepositoryError::Storage(
            "stored Deployment replica member binding is invalid".into(),
        ));
    }
    Ok(())
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
