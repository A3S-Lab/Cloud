use super::queries::{load_execution_by_id, lock_conversation, lock_execution};
use super::writes::{
    insert_change_set, insert_event, materialize_event_drafts, persist_conversation,
    persist_execution,
};
use crate::infrastructure::{
    idempotency_replay, store_idempotency, transaction_error, PostgresPersistenceError,
};
use crate::modules::agents::domain::{
    AcceptAgentCodeEventBatchWrite, AgentCodeRunWrite, AgentExecutionChangeSet,
    AgentExecutionEventDraft, RecoverAgentCodeRunWrite,
};
use crate::modules::shared_kernel::domain::{AgentExecutionId, RepositoryError};
use a3s_cloud_contracts::NodeCodeAgentEventReceiptV1;
use a3s_orm::{PostgresExecutor, PostgresTransaction};

pub(super) async fn recover_code_run(
    executor: &PostgresExecutor,
    write: RecoverAgentCodeRunWrite,
) -> Result<AgentCodeRunWrite, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut execution =
                    lock_execution(transaction, write.organization_id, write.execution_id).await?;
                let previous_version = execution.aggregate_version;
                let changed = execution
                    .recover_code_run(&write.expected_binding, write.recovered_at)
                    .map_err(RepositoryError::Conflict)?;
                if changed {
                    persist_execution(transaction, &execution, previous_version).await?;
                }
                Ok(AgentCodeRunWrite {
                    execution,
                    replayed: !changed,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn accept_code_event_batch(
    executor: &PostgresExecutor,
    write: AcceptAgentCodeEventBatchWrite,
) -> Result<NodeCodeAgentEventReceiptV1, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(receipt) = replay_code_event_batch(transaction, &write).await? {
                    return Ok(receipt);
                }
                let execution_id = AgentExecutionId::from_uuid(write.batch.binding.execution_id);
                let probe = load_execution_by_id(transaction, write.organization_id, execution_id)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                let mut conversation =
                    lock_conversation(transaction, write.organization_id, probe.conversation_id)
                        .await?;
                let previous_conversation_version = conversation.aggregate_version;
                let mut execution =
                    lock_execution(transaction, write.organization_id, execution_id).await?;
                let previous_execution_version = execution.aggregate_version;
                let binding = execution
                    .code
                    .as_ref()
                    .cloned()
                    .ok_or(RepositoryError::NotFound)?;
                if binding.node_id() != write.authenticated_node_id {
                    return Err(RepositoryError::NotFound.into());
                }
                if binding.node_runtime_binding(execution.id.as_uuid()) != write.batch.binding {
                    if binding.can_settle_recovery_predecessor_runtime_binding(
                        &write.batch.binding,
                        execution.id,
                    ) {
                        let receipt = write
                            .receipt(false)
                            .map_err(PostgresPersistenceError::Invariant)?;
                        store_idempotency(transaction, &write.idempotency, &receipt).await?;
                        return Ok(receipt);
                    }
                    return Err(RepositoryError::Conflict(
                        "Code Agent event batch changed its bound Runtime or run identity".into(),
                    )
                    .into());
                }

                let projected_at = write.accepted_at.max(execution.updated_at);
                let provider_page =
                    crate::modules::agents::infrastructure::project_code_event_page(
                        &binding,
                        &write.batch.page,
                    )
                    .map_err(invalid_repository_write)?;
                let drafts = if write.batch.page.retention_gap {
                    if write.batch.change_set.is_some() {
                        return Err(RepositoryError::Conflict(
                            "Code Agent retention gap cannot carry a terminal change set".into(),
                        )
                        .into());
                    }
                    binding
                        .validate_provider_recovery_page(&provider_page)
                        .map_err(RepositoryError::Conflict)?;
                    execution
                        .recover_code_run(&binding, projected_at)
                        .map_err(RepositoryError::Conflict)?;
                    Vec::new()
                } else {
                    let drafts = AgentExecutionEventDraft::semantic_from_provider_page(
                        &provider_page,
                        projected_at,
                    )
                    .map_err(invalid_repository_write)?;
                    execution
                        .accept_provider_event_page(&provider_page, projected_at, &drafts)
                        .map_err(RepositoryError::Conflict)?;
                    drafts
                };

                let change_set = write
                    .batch
                    .change_set
                    .clone()
                    .map(|change_set| {
                        AgentExecutionChangeSet::new(
                            write.organization_id,
                            execution.id,
                            write.batch.batch_id,
                            write.authenticated_node_id,
                            change_set,
                            write.accepted_at,
                        )
                        .map_err(invalid_repository_write)
                    })
                    .transpose()?;
                if let Some(change_set) = &change_set {
                    if execution.code.as_ref().map(|binding| binding.identity())
                        != Some(&change_set.change_set.identity)
                        || !execution.status.is_terminal()
                    {
                        return Err(RepositoryError::Conflict(
                            "Code Agent change set does not match its terminal execution".into(),
                        )
                        .into());
                    }
                }

                let events = if drafts.is_empty() {
                    Vec::new()
                } else {
                    let last_occurred_at = drafts
                        .last()
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "non-empty Code event page omitted its last draft".into(),
                            )
                        })?
                        .occurred_at;
                    let first_sequence = conversation
                        .allocate_event_sequences(drafts.len(), last_occurred_at)
                        .map_err(RepositoryError::Conflict)?;
                    materialize_event_drafts(
                        write.organization_id,
                        conversation.id,
                        execution.id,
                        drafts,
                        first_sequence,
                    )?
                };

                if !events.is_empty() {
                    persist_conversation(transaction, &conversation, previous_conversation_version)
                        .await?;
                }
                persist_execution(transaction, &execution, previous_execution_version).await?;
                for event in &events {
                    insert_event(transaction, event).await?;
                }
                if let Some(change_set) = &change_set {
                    insert_change_set(transaction, change_set).await?;
                }

                let receipt = write
                    .receipt(false)
                    .map_err(PostgresPersistenceError::Invariant)?;
                store_idempotency(transaction, &write.idempotency, &receipt).await?;
                Ok(receipt)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn replay_code_event_batch(
    transaction: &PostgresTransaction,
    write: &AcceptAgentCodeEventBatchWrite,
) -> Result<Option<NodeCodeAgentEventReceiptV1>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<NodeCodeAgentEventReceiptV1>(transaction, &write.idempotency).await?
    else {
        return Ok(None);
    };
    let mut receipt = replay.value;
    receipt.replayed = true;
    receipt.validate_for(&write.batch).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "Code Agent event replay changed its immutable receipt: {error}"
        ))
    })?;
    Ok(Some(receipt))
}

fn invalid_repository_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("invalid Agent repository write: {error}"))
}
