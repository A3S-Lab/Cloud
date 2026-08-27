use super::rows::{CheckpointSelection, EventSelection};
use super::schema::{AgentExecutionCheckpoints, AgentExecutionEvents};
use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::agents::domain::{AgentExecutionCheckpoint, AgentExecutionEvent};
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, OrganizationId, RepositoryError,
};
use a3s_orm::{
    select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
};

pub(super) async fn find_checkpoint(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    checkpoint_id: AgentExecutionCheckpointId,
) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError> {
    let row = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<AgentExecutionCheckpoints>()
                .select(CheckpointSelection)
                .filter(AgentExecutionCheckpoints::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentExecutionCheckpoints::id().eq(checkpoint_id.as_uuid())),
        )
        .await
        .map_err(storage)?;
    row.map(|row| row.checkpoint()).transpose()
}

pub(super) async fn list_checkpoints(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    limit: usize,
) -> Result<Vec<AgentExecutionCheckpoint>, RepositoryError> {
    if limit == 0 || limit > 1_000 {
        return Err(RepositoryError::Storage(
            "Agent execution checkpoint list limit is invalid".into(),
        ));
    }
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<AgentExecutionCheckpoints>()
                .select(CheckpointSelection)
                .filter(AgentExecutionCheckpoints::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentExecutionCheckpoints::execution_id().eq(execution_id.as_uuid()))
                .order_by(
                    AgentExecutionCheckpoints::through_event_sequence(),
                    OrderDirection::Desc,
                )
                .order_by(AgentExecutionCheckpoints::id(), OrderDirection::Desc)
                .limit(u64::try_from(limit).map_err(|_| {
                    RepositoryError::Storage("Agent checkpoint limit overflowed".into())
                })?),
        )
        .await
        .map_err(storage)?
        .rows;
    rows.into_iter().map(|row| row.checkpoint()).collect()
}

pub(super) async fn list_trajectory_events(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    after_sequence: Option<u64>,
    through_sequence: Option<u64>,
    limit: usize,
) -> Result<Vec<AgentExecutionEvent>, RepositoryError> {
    if limit == 0 || limit > 1_001 {
        return Err(RepositoryError::Storage(
            "Agent trajectory event limit is invalid".into(),
        ));
    }
    let mut query = select_from::<AgentExecutionEvents>()
        .select(EventSelection)
        .filter(AgentExecutionEvents::organization_id().eq(organization_id.as_uuid()))
        .filter(AgentExecutionEvents::execution_id().eq(execution_id.as_uuid()))
        .filter(AgentExecutionEvents::sequence().gt(after_sequence.unwrap_or(0)))
        .order_by(AgentExecutionEvents::sequence(), OrderDirection::Asc)
        .limit(u64::try_from(limit).map_err(|_| {
            RepositoryError::Storage("Agent trajectory event limit overflowed".into())
        })?);
    if let Some(through_sequence) = through_sequence {
        query = query.filter(AgentExecutionEvents::sequence().lte(through_sequence));
    }
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(query)
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(|row| row.event())
        .collect()
}

pub(super) async fn load_checkpoint(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    checkpoint_id: AgentExecutionCheckpointId,
) -> Result<Option<AgentExecutionCheckpoint>, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentExecutionCheckpoints>()
            .select(CheckpointSelection)
            .filter(AgentExecutionCheckpoints::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentExecutionCheckpoints::id().eq(checkpoint_id.as_uuid())),
    )
    .await?;
    row.map(|row| row.checkpoint().map_err(PostgresPersistenceError::from))
        .transpose()
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
