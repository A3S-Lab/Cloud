use super::approval_rows::ApprovalCheckpointSelection;
use super::schema::AgentApprovalCheckpoints;
use crate::infrastructure::{fetch_all, fetch_optional, PostgresPersistenceError};
use crate::modules::agents::domain::{AgentApprovalCheckpoint, AgentApprovalCheckpointStatus};
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentExecutionId, OrganizationId, RepositoryError,
};
use a3s_orm::{
    select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
};

pub(super) async fn find_checkpoint(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    checkpoint_id: AgentApprovalCheckpointId,
) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
    let row = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<AgentApprovalCheckpoints>()
                .select(ApprovalCheckpointSelection)
                .filter(AgentApprovalCheckpoints::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentApprovalCheckpoints::id().eq(checkpoint_id.as_uuid())),
        )
        .await
        .map_err(storage)?;
    row.map(|row| row.aggregate()).transpose()
}

pub(super) async fn find_active_checkpoint(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(active_query(organization_id, execution_id).limit(2))
        .await
        .map_err(storage)?
        .rows;
    exact_optional(rows).map_err(RepositoryError::Storage)
}

pub(super) async fn list_checkpoints(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    status: Option<AgentApprovalCheckpointStatus>,
    limit: usize,
) -> Result<Vec<AgentApprovalCheckpoint>, RepositoryError> {
    if limit == 0 || limit > 1_000 {
        return Err(RepositoryError::Storage(
            "Agent approval checkpoint list limit is invalid".into(),
        ));
    }
    let mut query = select_from::<AgentApprovalCheckpoints>()
        .select(ApprovalCheckpointSelection)
        .filter(AgentApprovalCheckpoints::organization_id().eq(organization_id.as_uuid()))
        .filter(AgentApprovalCheckpoints::execution_id().eq(execution_id.as_uuid()))
        .order_by(
            AgentApprovalCheckpoints::requested_at(),
            OrderDirection::Desc,
        )
        .order_by(AgentApprovalCheckpoints::id(), OrderDirection::Desc)
        .limit(u64::try_from(limit).map_err(|_| {
            RepositoryError::Storage("Agent approval checkpoint limit overflowed".into())
        })?);
    if let Some(status) = status {
        query = query.filter(AgentApprovalCheckpoints::status().eq(status.as_str()));
    }
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(query)
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(|row| row.aggregate())
        .collect()
}

pub(super) async fn load_checkpoint(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    checkpoint_id: AgentApprovalCheckpointId,
) -> Result<Option<AgentApprovalCheckpoint>, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentApprovalCheckpoints>()
            .select(ApprovalCheckpointSelection)
            .filter(AgentApprovalCheckpoints::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentApprovalCheckpoints::id().eq(checkpoint_id.as_uuid())),
    )
    .await?;
    row.map(|row| row.aggregate().map_err(PostgresPersistenceError::from))
        .transpose()
}

pub(super) async fn lock_checkpoint(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    checkpoint_id: AgentApprovalCheckpointId,
) -> Result<AgentApprovalCheckpoint, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentApprovalCheckpoints>()
            .select(ApprovalCheckpointSelection)
            .filter(AgentApprovalCheckpoints::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentApprovalCheckpoints::id().eq(checkpoint_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    row.aggregate().map_err(PostgresPersistenceError::from)
}

pub(super) async fn lock_active_checkpoint(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> Result<Option<AgentApprovalCheckpoint>, PostgresPersistenceError> {
    let rows = fetch_all(
        transaction,
        active_query(organization_id, execution_id)
            .limit(2)
            .for_update(),
    )
    .await?;
    exact_optional(rows).map_err(PostgresPersistenceError::Invariant)
}

pub(super) async fn lock_latest_checkpoint(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> Result<Option<AgentApprovalCheckpoint>, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentApprovalCheckpoints>()
            .select(ApprovalCheckpointSelection)
            .filter(AgentApprovalCheckpoints::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentApprovalCheckpoints::execution_id().eq(execution_id.as_uuid()))
            .order_by(
                AgentApprovalCheckpoints::requested_at(),
                OrderDirection::Desc,
            )
            .order_by(AgentApprovalCheckpoints::id(), OrderDirection::Desc)
            .limit(1)
            .for_update(),
    )
    .await?;
    row.map(|row| row.aggregate().map_err(PostgresPersistenceError::from))
        .transpose()
}

fn active_query(
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> a3s_orm::query::SelectQuery<
    AgentApprovalCheckpoints,
    super::approval_rows::ApprovalCheckpointRow,
> {
    select_from::<AgentApprovalCheckpoints>()
        .select(ApprovalCheckpointSelection)
        .filter(AgentApprovalCheckpoints::organization_id().eq(organization_id.as_uuid()))
        .filter(AgentApprovalCheckpoints::execution_id().eq(execution_id.as_uuid()))
        .filter(AgentApprovalCheckpoints::status().ne("resumed"))
        .filter(AgentApprovalCheckpoints::status().ne("cancelled"))
}

fn exact_optional(
    mut rows: Vec<super::approval_rows::ApprovalCheckpointRow>,
) -> Result<Option<AgentApprovalCheckpoint>, String> {
    match rows.len() {
        0 => Ok(None),
        1 => rows
            .pop()
            .ok_or_else(|| "Agent approval checkpoint row disappeared".to_owned())?
            .aggregate()
            .map(Some)
            .map_err(|error| error.to_string()),
        _ => Err("Agent execution has multiple active approval checkpoints".into()),
    }
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
