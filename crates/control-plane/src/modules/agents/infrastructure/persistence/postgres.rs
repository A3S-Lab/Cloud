mod approval_queries;
mod approval_rows;
mod approval_writes;
mod checkpoint_queries;
mod checkpoint_writes;
mod code_agent_writes;
mod provider_event_writes;
mod queries;
mod rows;
mod schema;
mod writes;

use crate::infrastructure::{idempotency_replay, transaction_error};
use crate::modules::agents::domain::{
    AcceptAgentCodeEventBatchWrite, AcceptAgentProviderEventBatchWrite, AgentApprovalCheckpoint,
    AgentApprovalCheckpointStatus, AgentApprovalCheckpointWrite, AgentCodeRunWrite,
    AgentConversation, AgentConversationWrite, AgentConversationWriteReference, AgentExecution,
    AgentExecutionChangeSet, AgentExecutionCheckpoint, AgentExecutionCheckpointWrite,
    AgentExecutionEvent, AgentExecutionEventsWrite, AgentExecutionWrite,
    AgentExecutionWriteReference, AppendAgentExecutionEventsWrite, BindAgentCodeRunWrite,
    CancelActiveAgentApprovalCheckpointWrite, CommitAgentExecutionCheckpointWrite,
    CreateAgentConversationWrite, DecideAgentApprovalCheckpointWrite,
    ExpireAgentApprovalCheckpointWrite, ForkAgentExecutionWrite,
    IAgentApprovalCheckpointRepository, IAgentExecutionCheckpointRepository, IAgentRepository,
    RecoverAgentCodeRunWrite, RequestAgentExecutionCancellationWrite,
    ResumeAgentApprovalCheckpointWrite, StartAgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentConversationId, AgentExecutionCheckpointId, AgentExecutionId,
    EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{NodeAgentProviderEventReceiptV1, NodeCodeAgentEventReceiptV1};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;

#[derive(Clone)]
pub struct PostgresAgentRepository {
    executor: PostgresExecutor,
}

#[async_trait]
impl IAgentApprovalCheckpointRepository for PostgresAgentRepository {
    async fn replay_checkpoint_decision(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
        approval_writes::replay_checkpoint_decision(&self.executor, idempotency).await
    }

    async fn decide_checkpoint(
        &self,
        write: DecideAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
        approval_writes::decide_checkpoint(&self.executor, write).await
    }

    async fn expire_checkpoint(
        &self,
        write: ExpireAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
        approval_writes::expire_checkpoint(&self.executor, write).await
    }

    async fn mark_checkpoint_resumed(
        &self,
        write: ResumeAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError> {
        approval_writes::mark_checkpoint_resumed(&self.executor, write).await
    }

    async fn cancel_active_checkpoint(
        &self,
        write: CancelActiveAgentApprovalCheckpointWrite,
    ) -> Result<Option<AgentApprovalCheckpointWrite>, RepositoryError> {
        approval_writes::cancel_active_checkpoint(&self.executor, write).await
    }

    async fn find_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentApprovalCheckpointId,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
        approval_queries::find_checkpoint(&self.executor, organization_id, checkpoint_id).await
    }

    async fn find_active_checkpoint(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError> {
        approval_queries::find_active_checkpoint(&self.executor, organization_id, execution_id)
            .await
    }

    async fn list_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        status: Option<AgentApprovalCheckpointStatus>,
        limit: usize,
    ) -> Result<Vec<AgentApprovalCheckpoint>, RepositoryError> {
        approval_queries::list_checkpoints(
            &self.executor,
            organization_id,
            execution_id,
            status,
            limit,
        )
        .await
    }
}

#[async_trait]
impl IAgentExecutionCheckpointRepository for PostgresAgentRepository {
    async fn commit_execution_checkpoint(
        &self,
        write: CommitAgentExecutionCheckpointWrite,
    ) -> Result<AgentExecutionCheckpointWrite, RepositoryError> {
        checkpoint_writes::commit_checkpoint(&self.executor, write).await
    }

    async fn fork_execution(
        &self,
        write: ForkAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError> {
        checkpoint_writes::fork_execution(&self.executor, write).await
    }

    async fn replay_execution_checkpoint(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError> {
        checkpoint_writes::replay_checkpoint(&self.executor, idempotency).await
    }

    async fn find_execution_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentExecutionCheckpointId,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError> {
        checkpoint_queries::find_checkpoint(&self.executor, organization_id, checkpoint_id).await
    }

    async fn list_execution_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        limit: usize,
    ) -> Result<Vec<AgentExecutionCheckpoint>, RepositoryError> {
        checkpoint_queries::list_checkpoints(&self.executor, organization_id, execution_id, limit)
            .await
    }

    async fn list_execution_trajectory_events(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        after_sequence: Option<u64>,
        through_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError> {
        checkpoint_queries::list_trajectory_events(
            &self.executor,
            organization_id,
            execution_id,
            after_sequence,
            through_sequence,
            limit,
        )
        .await
    }
}

impl PostgresAgentRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAgentRepository for PostgresAgentRepository {
    async fn create_conversation(
        &self,
        write: CreateAgentConversationWrite,
    ) -> Result<AgentConversationWrite, RepositoryError> {
        writes::create_conversation(&self.executor, write).await
    }

    async fn start_execution(
        &self,
        write: StartAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError> {
        writes::start_execution(&self.executor, write).await
    }

    async fn request_cancellation(
        &self,
        write: RequestAgentExecutionCancellationWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError> {
        writes::request_cancellation(&self.executor, write).await
    }

    async fn append_events(
        &self,
        write: AppendAgentExecutionEventsWrite,
    ) -> Result<AgentExecutionEventsWrite, RepositoryError> {
        writes::append_events(&self.executor, write).await
    }

    async fn bind_code_run(
        &self,
        write: BindAgentCodeRunWrite,
    ) -> Result<AgentCodeRunWrite, RepositoryError> {
        writes::bind_code_run(&self.executor, write).await
    }

    async fn recover_code_run(
        &self,
        write: RecoverAgentCodeRunWrite,
    ) -> Result<AgentCodeRunWrite, RepositoryError> {
        code_agent_writes::recover_code_run(&self.executor, write).await
    }

    async fn accept_code_event_batch(
        &self,
        write: AcceptAgentCodeEventBatchWrite,
    ) -> Result<NodeCodeAgentEventReceiptV1, RepositoryError> {
        code_agent_writes::accept_code_event_batch(&self.executor, write).await
    }

    async fn accept_provider_event_batch(
        &self,
        write: AcceptAgentProviderEventBatchWrite,
    ) -> Result<NodeAgentProviderEventReceiptV1, RepositoryError> {
        provider_event_writes::accept_provider_event_batch(&self.executor, write).await
    }

    async fn replay_conversation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentConversation>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) = idempotency_replay::<AgentConversationWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    queries::load_conversation_by_id(
                        transaction,
                        replay.value.organization_id,
                        replay.value.conversation_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_execution(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecution>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replay) = idempotency_replay::<AgentExecutionWriteReference>(
                        transaction,
                        &idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    queries::load_execution_by_id(
                        transaction,
                        replay.value.organization_id,
                        replay.value.execution_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_conversation(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
    ) -> Result<Option<AgentConversation>, RepositoryError> {
        queries::find_conversation(&self.executor, organization_id, conversation_id).await
    }

    async fn list_conversations(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<AgentConversation>, RepositoryError> {
        queries::list_conversations(
            &self.executor,
            organization_id,
            project_id,
            environment_id,
            limit,
        )
        .await
    }

    async fn find_execution(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecution>, RepositoryError> {
        queries::find_execution(&self.executor, organization_id, execution_id).await
    }

    async fn find_execution_change_set(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecutionChangeSet>, RepositoryError> {
        queries::find_execution_change_set(&self.executor, organization_id, execution_id).await
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<AgentExecution>, RepositoryError> {
        queries::pending_operation_starts(&self.executor, limit).await
    }

    async fn find_execution_request(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecutionEvent>, RepositoryError> {
        queries::find_execution_request(&self.executor, organization_id, execution_id).await
    }

    async fn list_executions(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        limit: usize,
    ) -> Result<Vec<AgentExecution>, RepositoryError> {
        queries::list_executions(&self.executor, organization_id, conversation_id, limit).await
    }

    async fn list_events(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError> {
        queries::list_events(
            &self.executor,
            organization_id,
            conversation_id,
            after_sequence,
            limit,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_persistence_uses_only_typed_a3s_orm_queries() {
        for (name, source) in [
            ("repository", include_str!("postgres.rs")),
            (
                "approval queries",
                include_str!("postgres/approval_queries.rs"),
            ),
            ("approval rows", include_str!("postgres/approval_rows.rs")),
            (
                "approval writes",
                include_str!("postgres/approval_writes.rs"),
            ),
            (
                "checkpoint queries",
                include_str!("postgres/checkpoint_queries.rs"),
            ),
            (
                "checkpoint writes",
                include_str!("postgres/checkpoint_writes.rs"),
            ),
            ("queries", include_str!("postgres/queries.rs")),
            ("rows", include_str!("postgres/rows.rs")),
            ("schema", include_str!("postgres/schema.rs")),
            ("writes", include_str!("postgres/writes.rs")),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap_or(source);
            for forbidden in [
                "sql_query",
                "tokio_postgres",
                "sqlx::",
                "diesel::",
                "sea_orm::",
            ] {
                assert!(
                    !production.contains(forbidden),
                    "Agent {name} persistence must use typed A3S ORM builders; found {forbidden}"
                );
            }
        }
    }

    #[test]
    fn migration_keeps_one_conversation_head_and_no_duplicate_authority() {
        let migration = include_str!(
            "../../../../../../../migrations/068_agent_conversations_and_executions.sql"
        );
        for table in [
            "create table agent_conversations",
            "create table agent_executions",
            "create table agent_execution_events",
        ] {
            assert!(migration.contains(table), "missing {table}");
        }
        assert_eq!(migration.matches("last_event_sequence").count(), 2);
        for forbidden in [
            "agent_execution_heads",
            "agent_event_contents",
            "agent_idempotency",
            "agent_jobs",
            "agent_queues",
        ] {
            assert!(
                !migration.contains(forbidden),
                "found forbidden {forbidden}"
            );
        }
    }

    #[test]
    fn cancellation_migration_extends_the_existing_execution_row_only() {
        let migration =
            include_str!("../../../../../../../migrations/070_agent_execution_cancellation.sql");
        assert!(migration.contains("add column cancellation_requested_at timestamptz"));
        assert!(migration.contains("'cancelling'"));
        for forbidden in [
            "create table",
            "agent_cancellation",
            "agent_commands",
            "agent_runs",
        ] {
            assert!(
                !migration.contains(forbidden),
                "cancellation migration introduced duplicate authority {forbidden}"
            );
        }
    }
}
