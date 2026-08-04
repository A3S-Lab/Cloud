use super::rows::{ConversationSelection, EventSelection, ExecutionSelection};
use super::schema::{AgentConversations, AgentExecutionEvents, AgentExecutions};
use crate::infrastructure::{fetch_all, fetch_optional, PostgresPersistenceError};
use crate::modules::agents::domain::{AgentConversation, AgentExecution, AgentExecutionEvent};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_orm::{
    select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
};

pub(super) async fn find_conversation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
) -> Result<Option<AgentConversation>, RepositoryError> {
    let row = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<AgentConversations>()
                .select(ConversationSelection)
                .filter(AgentConversations::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentConversations::id().eq(conversation_id.as_uuid())),
        )
        .await
        .map_err(storage)?;
    row.map(|row| row.aggregate()).transpose()
}

pub(super) async fn list_conversations(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    limit: usize,
) -> Result<Vec<AgentConversation>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<AgentConversations>()
                .select(ConversationSelection)
                .filter(AgentConversations::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentConversations::project_id().eq(project_id.as_uuid()))
                .filter(AgentConversations::environment_id().eq(environment_id.as_uuid()))
                .order_by(AgentConversations::created_at(), OrderDirection::Desc)
                .order_by(AgentConversations::id(), OrderDirection::Desc)
                .limit(limit_u64(limit)?),
        )
        .await
        .map_err(storage)?
        .rows;
    rows.into_iter().map(|row| row.aggregate()).collect()
}

pub(super) async fn find_execution(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> Result<Option<AgentExecution>, RepositoryError> {
    let row = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<AgentExecutions>()
                .select(ExecutionSelection)
                .filter(AgentExecutions::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentExecutions::id().eq(execution_id.as_uuid())),
        )
        .await
        .map_err(storage)?;
    row.map(|row| row.aggregate()).transpose()
}

pub(super) async fn list_executions(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
    limit: usize,
) -> Result<Vec<AgentExecution>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<AgentExecutions>()
                .select(ExecutionSelection)
                .filter(AgentExecutions::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentExecutions::conversation_id().eq(conversation_id.as_uuid()))
                .order_by(AgentExecutions::requested_at(), OrderDirection::Desc)
                .order_by(AgentExecutions::id(), OrderDirection::Desc)
                .limit(limit_u64(limit)?),
        )
        .await
        .map_err(storage)?
        .rows;
    rows.into_iter().map(|row| row.aggregate()).collect()
}

pub(super) async fn list_events(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
    after_sequence: Option<u64>,
    limit: usize,
) -> Result<Vec<AgentExecutionEvent>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<AgentExecutionEvents>()
                .select(EventSelection)
                .filter(AgentExecutionEvents::organization_id().eq(organization_id.as_uuid()))
                .filter(AgentExecutionEvents::conversation_id().eq(conversation_id.as_uuid()))
                .filter(AgentExecutionEvents::sequence().gt(after_sequence.unwrap_or(0)))
                .order_by(AgentExecutionEvents::sequence(), OrderDirection::Asc)
                .limit(limit_u64(limit)?),
        )
        .await
        .map_err(storage)?
        .rows;
    rows.into_iter().map(|row| row.event()).collect()
}

pub(super) async fn load_conversation_by_id(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
) -> Result<Option<AgentConversation>, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentConversations>()
            .select(ConversationSelection)
            .filter(AgentConversations::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentConversations::id().eq(conversation_id.as_uuid())),
    )
    .await?;
    row.map(|row| row.aggregate().map_err(PostgresPersistenceError::from))
        .transpose()
}

pub(super) async fn lock_conversation(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
) -> Result<AgentConversation, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentConversations>()
            .select(ConversationSelection)
            .filter(AgentConversations::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentConversations::id().eq(conversation_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    row.aggregate().map_err(PostgresPersistenceError::from)
}

pub(super) async fn load_execution_by_id(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> Result<Option<AgentExecution>, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentExecutions>()
            .select(ExecutionSelection)
            .filter(AgentExecutions::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentExecutions::id().eq(execution_id.as_uuid())),
    )
    .await?;
    row.map(|row| row.aggregate().map_err(PostgresPersistenceError::from))
        .transpose()
}

pub(super) async fn lock_execution(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
) -> Result<AgentExecution, PostgresPersistenceError> {
    let row = fetch_optional(
        transaction,
        select_from::<AgentExecutions>()
            .select(ExecutionSelection)
            .filter(AgentExecutions::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentExecutions::id().eq(execution_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    row.aggregate().map_err(PostgresPersistenceError::from)
}

pub(super) async fn load_event_range(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
    execution_id: AgentExecutionId,
    first_sequence: u64,
    last_sequence: u64,
) -> Result<Vec<AgentExecutionEvent>, PostgresPersistenceError> {
    let rows = fetch_all(
        transaction,
        select_from::<AgentExecutionEvents>()
            .select(EventSelection)
            .filter(AgentExecutionEvents::organization_id().eq(organization_id.as_uuid()))
            .filter(AgentExecutionEvents::conversation_id().eq(conversation_id.as_uuid()))
            .filter(AgentExecutionEvents::execution_id().eq(execution_id.as_uuid()))
            .filter(AgentExecutionEvents::sequence().gte(first_sequence))
            .filter(AgentExecutionEvents::sequence().lte(last_sequence))
            .order_by(AgentExecutionEvents::sequence(), OrderDirection::Asc),
    )
    .await?;
    rows.into_iter()
        .map(|row| row.event().map_err(PostgresPersistenceError::from))
        .collect()
}

fn limit_u64(limit: usize) -> Result<u64, RepositoryError> {
    u64::try_from(limit)
        .map_err(|_| RepositoryError::Storage("query limit exceeds supported bounds".into()))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
