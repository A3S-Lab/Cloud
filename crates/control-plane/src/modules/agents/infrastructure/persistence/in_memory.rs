use crate::modules::agents::domain::{
    AcceptAgentCodeEventBatchWrite, AgentCodeRunWrite, AgentConversation, AgentConversationStatus,
    AgentConversationWrite, AgentConversationWriteReference, AgentExecution,
    AgentExecutionChangeSet, AgentExecutionEvent, AgentExecutionEventDraft,
    AgentExecutionEventKind, AgentExecutionEventsWrite, AgentExecutionEventsWriteReference,
    AgentExecutionWrite, AgentExecutionWriteReference, AppendAgentExecutionEventsWrite,
    BindAgentCodeRunWrite, CreateAgentConversationWrite, IAgentRepository,
    RecoverAgentCodeRunWrite, RequestAgentExecutionCancellationWrite, StartAgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, EnvironmentId, IdempotencyRequest, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_cloud_contracts::NodeCodeAgentEventReceiptV1;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryAgentRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    conversations: BTreeMap<(OrganizationId, AgentConversationId), AgentConversation>,
    executions: BTreeMap<(OrganizationId, AgentExecutionId), AgentExecution>,
    change_sets: BTreeMap<(OrganizationId, AgentExecutionId), AgentExecutionChangeSet>,
    events: BTreeMap<(OrganizationId, AgentConversationId, u64), AgentExecutionEvent>,
    idempotency: BTreeMap<(String, String), IdempotencyEntry>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

#[derive(Clone)]
struct IdempotencyEntry {
    request_digest: String,
    response: IdempotencyResponse,
}

#[derive(Clone)]
enum IdempotencyResponse {
    Conversation(AgentConversationWriteReference),
    Execution(AgentExecutionWriteReference),
    Events(AgentExecutionEventsWriteReference),
    CodeEvents(NodeCodeAgentEventReceiptV1),
}

impl InMemoryAgentRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IAgentRepository for InMemoryAgentRepository {
    async fn create_conversation(
        &self,
        write: CreateAgentConversationWrite,
    ) -> Result<AgentConversationWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;

        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::Conversation(reference) = response else {
                return Err(corrupt("Agent conversation replay type changed"));
            };
            let conversation = state
                .conversations
                .get(&(reference.organization_id, reference.conversation_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent conversation replay target is missing"))?;
            return Ok(AgentConversationWrite {
                conversation,
                replayed: true,
            });
        }

        let identity = (write.conversation.organization_id, write.conversation.id);
        if state.conversations.contains_key(&identity) {
            return Err(RepositoryError::Conflict(
                "Agent conversation identity is already in use".into(),
            ));
        }
        let reference = AgentConversationWriteReference {
            organization_id: write.conversation.organization_id,
            conversation_id: write.conversation.id,
        };
        state
            .conversations
            .insert(identity, write.conversation.clone());
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::Conversation(reference),
        );
        state.outbox.push(write.event);
        Ok(AgentConversationWrite {
            conversation: write.conversation,
            replayed: false,
        })
    }

    async fn start_execution(
        &self,
        write: StartAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;

        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::Execution(reference) = response else {
                return Err(corrupt("Agent execution replay type changed"));
            };
            let execution = state
                .executions
                .get(&(reference.organization_id, reference.execution_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent execution replay target is missing"))?;
            let conversation = state
                .conversations
                .get(&(execution.organization_id, execution.conversation_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent execution conversation is missing"))?;
            return Ok(AgentExecutionWrite {
                conversation,
                execution,
                replayed: true,
            });
        }

        let conversation_key = (
            write.execution.organization_id,
            write.execution.conversation_id,
        );
        let mut conversation = state
            .conversations
            .get(&conversation_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if conversation.status != AgentConversationStatus::Active {
            return Err(RepositoryError::Conflict(
                "closed Agent conversation cannot start an execution".into(),
            ));
        }
        let execution_key = (write.execution.organization_id, write.execution.id);
        if state.executions.contains_key(&execution_key)
            || state
                .executions
                .values()
                .any(|execution| execution.operation_id == write.execution.operation_id)
        {
            return Err(RepositoryError::Conflict(
                "Agent execution identity is already in use".into(),
            ));
        }
        let first_sequence = conversation
            .allocate_event_sequences(1, write.initial_event.occurred_at)
            .map_err(RepositoryError::Conflict)?;
        let initial_event = AgentExecutionEvent::from_draft(
            write.execution.organization_id,
            write.execution.conversation_id,
            write.execution.id,
            first_sequence,
            write.initial_event,
        )
        .map_err(invalid_repository_write)?;
        let event_key = (
            initial_event.organization_id,
            initial_event.conversation_id,
            initial_event.sequence,
        );
        if state.events.contains_key(&event_key) {
            return Err(corrupt("Agent event sequence is already committed"));
        }

        let reference = AgentExecutionWriteReference {
            organization_id: write.execution.organization_id,
            execution_id: write.execution.id,
        };
        state
            .conversations
            .insert(conversation_key, conversation.clone());
        state
            .executions
            .insert(execution_key, write.execution.clone());
        state.events.insert(event_key, initial_event);
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::Execution(reference),
        );
        state.outbox.push(write.event);
        Ok(AgentExecutionWrite {
            conversation,
            execution: write.execution,
            replayed: false,
        })
    }

    async fn request_cancellation(
        &self,
        write: RequestAgentExecutionCancellationWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;

        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::Execution(reference) = response else {
                return Err(corrupt("Agent cancellation replay type changed"));
            };
            let execution = state
                .executions
                .get(&(reference.organization_id, reference.execution_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent cancellation replay target is missing"))?;
            let conversation = state
                .conversations
                .get(&(execution.organization_id, execution.conversation_id))
                .cloned()
                .ok_or_else(|| corrupt("Agent execution conversation is missing"))?;
            return Ok(AgentExecutionWrite {
                conversation,
                execution,
                replayed: true,
            });
        }

        let key = (write.execution.organization_id, write.execution.id);
        let existing = state
            .executions
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let mut expected = existing.clone();
        if existing.aggregate_version != write.expected_version
            || expected
                .request_cancellation(write.execution.updated_at)
                .is_err()
            || expected != write.execution
        {
            return Err(RepositoryError::Conflict(
                "Agent execution changed while requesting cancellation".into(),
            ));
        }
        let conversation = state
            .conversations
            .get(&(
                write.execution.organization_id,
                write.execution.conversation_id,
            ))
            .cloned()
            .ok_or_else(|| corrupt("Agent execution conversation is missing"))?;
        let reference = AgentExecutionWriteReference {
            organization_id: write.execution.organization_id,
            execution_id: write.execution.id,
        };
        state.executions.insert(key, write.execution.clone());
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::Execution(reference),
        );
        state.outbox.push(write.event);
        Ok(AgentExecutionWrite {
            conversation,
            execution: write.execution,
            replayed: false,
        })
    }

    async fn append_events(
        &self,
        write: AppendAgentExecutionEventsWrite,
    ) -> Result<AgentExecutionEventsWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::Events(reference) = response else {
                return Err(corrupt("Agent event replay type changed"));
            };
            return replay_events(&state, reference);
        }

        let conversation_key = (write.organization_id, write.conversation_id);
        let execution_key = (write.organization_id, write.execution_id);
        let mut conversation = state
            .conversations
            .get(&conversation_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let mut execution = state
            .executions
            .get(&execution_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if execution.conversation_id != write.conversation_id {
            return Err(RepositoryError::NotFound);
        }

        for event in &write.events {
            execution
                .apply_event(event)
                .map_err(RepositoryError::Conflict)?;
        }
        let last_occurred_at = write
            .events
            .last()
            .expect("validated non-empty Agent event batch")
            .occurred_at;
        let first_sequence = conversation
            .allocate_event_sequences(write.events.len(), last_occurred_at)
            .map_err(RepositoryError::Conflict)?;
        let events = write
            .events
            .into_iter()
            .enumerate()
            .map(|(offset, draft)| {
                let offset = u64::try_from(offset)
                    .map_err(|_| corrupt("Agent event sequence offset overflowed"))?;
                let sequence = first_sequence
                    .checked_add(offset)
                    .ok_or_else(|| corrupt("Agent event sequence overflowed"))?;
                AgentExecutionEvent::from_draft(
                    write.organization_id,
                    write.conversation_id,
                    write.execution_id,
                    sequence,
                    draft,
                )
                .map_err(invalid_repository_write)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for event in &events {
            let key = (event.organization_id, event.conversation_id, event.sequence);
            if state.events.contains_key(&key) {
                return Err(corrupt("Agent event sequence is already committed"));
            }
        }
        let last_sequence = events
            .last()
            .expect("validated non-empty Agent event batch")
            .sequence;
        let reference = AgentExecutionEventsWriteReference {
            organization_id: write.organization_id,
            conversation_id: write.conversation_id,
            execution_id: write.execution_id,
            first_sequence,
            last_sequence,
        };
        state
            .conversations
            .insert(conversation_key, conversation.clone());
        state.executions.insert(execution_key, execution.clone());
        for event in &events {
            state.events.insert(
                (event.organization_id, event.conversation_id, event.sequence),
                event.clone(),
            );
        }
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::Events(reference),
        );
        Ok(AgentExecutionEventsWrite {
            conversation,
            execution,
            events,
            replayed: false,
        })
    }

    async fn bind_code_run(
        &self,
        write: BindAgentCodeRunWrite,
    ) -> Result<AgentCodeRunWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        let key = (write.organization_id, write.execution_id);
        let mut execution = state
            .executions
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let changed = execution
            .bind_code_run(write.binding)
            .map_err(RepositoryError::Conflict)?;
        if changed {
            state.executions.insert(key, execution.clone());
        }
        Ok(AgentCodeRunWrite {
            execution,
            replayed: !changed,
        })
    }

    async fn recover_code_run(
        &self,
        write: RecoverAgentCodeRunWrite,
    ) -> Result<AgentCodeRunWrite, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        let key = (write.organization_id, write.execution_id);
        let mut execution = state
            .executions
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let changed = execution
            .recover_code_run(&write.expected_binding, write.recovered_at)
            .map_err(RepositoryError::Conflict)?;
        if changed {
            state.executions.insert(key, execution.clone());
        }
        Ok(AgentCodeRunWrite {
            execution,
            replayed: !changed,
        })
    }

    async fn accept_code_event_batch(
        &self,
        write: AcceptAgentCodeEventBatchWrite,
    ) -> Result<NodeCodeAgentEventReceiptV1, RepositoryError> {
        write.validate().map_err(invalid_repository_write)?;
        let mut state = self.state.write().await;
        if let Some(response) = replay(&state, &write.idempotency)? {
            let IdempotencyResponse::CodeEvents(mut receipt) = response else {
                return Err(corrupt("Code Agent event replay type changed"));
            };
            receipt.replayed = true;
            receipt.validate_for(&write.batch).map_err(|error| {
                corrupt(format!(
                    "Code Agent event replay changed its immutable receipt: {error}"
                ))
            })?;
            return Ok(receipt);
        }

        let execution_id = AgentExecutionId::from_uuid(write.batch.binding.execution_id);
        let execution_key = (write.organization_id, execution_id);
        let mut execution = state
            .executions
            .get(&execution_key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let conversation_key = (write.organization_id, execution.conversation_id);
        let mut conversation = state
            .conversations
            .get(&conversation_key)
            .cloned()
            .ok_or_else(|| corrupt("Agent execution conversation is missing"))?;
        let binding = execution
            .code
            .as_ref()
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if binding.node_id() != write.authenticated_node_id {
            return Err(RepositoryError::NotFound);
        }
        if binding.node_runtime_binding(execution.id.as_uuid()) != write.batch.binding {
            if binding
                .can_settle_recovery_predecessor_runtime_binding(&write.batch.binding, execution.id)
            {
                let receipt = write.receipt(false).map_err(corrupt)?;
                store_replay(
                    &mut state,
                    write.idempotency,
                    IdempotencyResponse::CodeEvents(receipt.clone()),
                );
                return Ok(receipt);
            }
            return Err(RepositoryError::Conflict(
                "Code Agent event batch changed its bound Runtime or run identity".into(),
            ));
        }

        let projected_at = write.accepted_at.max(execution.updated_at);
        let provider_page = crate::modules::agents::infrastructure::project_code_event_page(
            &binding,
            &write.batch.page,
        )
        .map_err(invalid_repository_write)?;
        let drafts = if write.batch.page.retention_gap {
            if write.batch.change_set.is_some() {
                return Err(RepositoryError::Conflict(
                    "Code Agent retention gap cannot carry a terminal change set".into(),
                ));
            }
            binding
                .validate_provider_recovery_page(&provider_page)
                .map_err(RepositoryError::Conflict)?;
            execution
                .recover_code_run(&binding, projected_at)
                .map_err(RepositoryError::Conflict)?;
            Vec::new()
        } else {
            let drafts =
                AgentExecutionEventDraft::semantic_from_provider_page(&provider_page, projected_at)
                    .map_err(invalid_repository_write)?;
            execution
                .accept_provider_event_page(&provider_page, projected_at, &drafts)
                .map_err(RepositoryError::Conflict)?;
            drafts
        };

        let events = if drafts.is_empty() {
            Vec::new()
        } else {
            let last_occurred_at = drafts
                .last()
                .expect("non-empty Code event page")
                .occurred_at;
            let first_sequence = conversation
                .allocate_event_sequences(drafts.len(), last_occurred_at)
                .map_err(RepositoryError::Conflict)?;
            drafts
                .into_iter()
                .enumerate()
                .map(|(offset, draft)| {
                    let offset = u64::try_from(offset)
                        .map_err(|_| corrupt("Agent event sequence offset overflowed"))?;
                    let sequence = first_sequence
                        .checked_add(offset)
                        .ok_or_else(|| corrupt("Agent event sequence overflowed"))?;
                    AgentExecutionEvent::from_draft(
                        write.organization_id,
                        conversation.id,
                        execution.id,
                        sequence,
                        draft,
                    )
                    .map_err(invalid_repository_write)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        for event in &events {
            let key = (event.organization_id, event.conversation_id, event.sequence);
            if state.events.contains_key(&key) {
                return Err(corrupt("Agent event sequence is already committed"));
            }
        }

        let receipt = write.receipt(false).map_err(corrupt)?;

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
                ));
            }
            if state
                .change_sets
                .get(&execution_key)
                .is_some_and(|existing| existing != change_set)
            {
                return Err(RepositoryError::Conflict(
                    "Agent execution change set is immutable".into(),
                ));
            }
        }

        if !events.is_empty() {
            state.conversations.insert(conversation_key, conversation);
        }
        state.executions.insert(execution_key, execution);
        if let Some(change_set) = change_set {
            state.change_sets.insert(execution_key, change_set);
        }
        for event in events {
            state.events.insert(
                (event.organization_id, event.conversation_id, event.sequence),
                event,
            );
        }
        store_replay(
            &mut state,
            write.idempotency,
            IdempotencyResponse::CodeEvents(receipt.clone()),
        );
        Ok(receipt)
    }

    async fn replay_conversation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentConversation>, RepositoryError> {
        let state = self.state.read().await;
        let Some(response) = replay(&state, idempotency)? else {
            return Ok(None);
        };
        let IdempotencyResponse::Conversation(reference) = response else {
            return Err(corrupt("Agent conversation replay type changed"));
        };
        state
            .conversations
            .get(&(reference.organization_id, reference.conversation_id))
            .cloned()
            .map(Some)
            .ok_or_else(|| corrupt("Agent conversation replay target is missing"))
    }

    async fn replay_execution(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecution>, RepositoryError> {
        let state = self.state.read().await;
        let Some(response) = replay(&state, idempotency)? else {
            return Ok(None);
        };
        let IdempotencyResponse::Execution(reference) = response else {
            return Err(corrupt("Agent execution replay type changed"));
        };
        state
            .executions
            .get(&(reference.organization_id, reference.execution_id))
            .cloned()
            .map(Some)
            .ok_or_else(|| corrupt("Agent execution replay target is missing"))
    }

    async fn find_conversation(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
    ) -> Result<Option<AgentConversation>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .conversations
            .get(&(organization_id, conversation_id))
            .cloned())
    }

    async fn list_conversations(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<AgentConversation>, RepositoryError> {
        let mut conversations = self
            .state
            .read()
            .await
            .conversations
            .values()
            .filter(|conversation| {
                conversation.organization_id == organization_id
                    && conversation.project_id == project_id
                    && conversation.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        conversations.sort_by_key(|conversation| {
            std::cmp::Reverse((conversation.created_at, conversation.id))
        });
        conversations.truncate(limit);
        Ok(conversations)
    }

    async fn find_execution(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecution>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .executions
            .get(&(organization_id, execution_id))
            .cloned())
    }

    async fn find_execution_change_set(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecutionChangeSet>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .change_sets
            .get(&(organization_id, execution_id))
            .cloned())
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<AgentExecution>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut executions = self
            .state
            .read()
            .await
            .executions
            .values()
            .filter(|execution| {
                matches!(
                    execution.status,
                    crate::modules::agents::domain::AgentExecutionStatus::Pending
                        | crate::modules::agents::domain::AgentExecutionStatus::Cancelling
                ) && execution.code.is_none()
            })
            .cloned()
            .collect::<Vec<_>>();
        executions.sort_by_key(|execution| (execution.requested_at, execution.id));
        executions.truncate(limit);
        Ok(executions)
    }

    async fn find_execution_request(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecutionEvent>, RepositoryError> {
        let state = self.state.read().await;
        let mut requests = state.events.values().filter(|event| {
            event.organization_id == organization_id
                && event.execution_id == execution_id
                && event.kind == AgentExecutionEventKind::ExecutionRequested
        });
        let request = requests.next().cloned();
        if requests.next().is_some() {
            return Err(corrupt(
                "Agent execution has more than one execution_requested event",
            ));
        }
        Ok(request)
    }

    async fn list_executions(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        limit: usize,
    ) -> Result<Vec<AgentExecution>, RepositoryError> {
        let mut executions = self
            .state
            .read()
            .await
            .executions
            .values()
            .filter(|execution| {
                execution.organization_id == organization_id
                    && execution.conversation_id == conversation_id
            })
            .cloned()
            .collect::<Vec<_>>();
        executions
            .sort_by_key(|execution| std::cmp::Reverse((execution.requested_at, execution.id)));
        executions.truncate(limit);
        Ok(executions)
    }

    async fn list_events(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError> {
        let after_sequence = after_sequence.unwrap_or(0);
        Ok(self
            .state
            .read()
            .await
            .events
            .range(
                (
                    organization_id,
                    conversation_id,
                    after_sequence.saturating_add(1),
                )..=(organization_id, conversation_id, u64::MAX),
            )
            .take(limit)
            .map(|(_, event)| event.clone())
            .collect())
    }
}

fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotencyResponse>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some(entry) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if entry.request_digest != idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    Ok(Some(entry.response.clone()))
}

fn store_replay(state: &mut State, idempotency: IdempotencyRequest, response: IdempotencyResponse) {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    state.idempotency.insert(
        key,
        IdempotencyEntry {
            request_digest: idempotency.request_digest,
            response,
        },
    );
}

fn replay_events(
    state: &State,
    reference: AgentExecutionEventsWriteReference,
) -> Result<AgentExecutionEventsWrite, RepositoryError> {
    let conversation = state
        .conversations
        .get(&(reference.organization_id, reference.conversation_id))
        .cloned()
        .ok_or_else(|| corrupt("Agent event replay conversation is missing"))?;
    let execution = state
        .executions
        .get(&(reference.organization_id, reference.execution_id))
        .cloned()
        .ok_or_else(|| corrupt("Agent event replay execution is missing"))?;
    let events = (reference.first_sequence..=reference.last_sequence)
        .map(|sequence| {
            state
                .events
                .get(&(
                    reference.organization_id,
                    reference.conversation_id,
                    sequence,
                ))
                .cloned()
                .ok_or_else(|| corrupt("Agent event replay range is incomplete"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if events
        .iter()
        .any(|event| event.execution_id != reference.execution_id)
    {
        return Err(corrupt("Agent event replay range changed execution"));
    }
    Ok(AgentExecutionEventsWrite {
        conversation,
        execution,
        events,
        replayed: true,
    })
}

fn invalid_repository_write(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "invalid Agent repository write: {}",
        message.into()
    ))
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(message.into())
}

#[cfg(test)]
#[path = "in_memory/tests.rs"]
mod tests;
