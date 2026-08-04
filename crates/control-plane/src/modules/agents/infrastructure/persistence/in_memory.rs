use crate::modules::agents::domain::{
    AgentConversation, AgentConversationStatus, AgentConversationWrite,
    AgentConversationWriteReference, AgentExecution, AgentExecutionEvent,
    AgentExecutionEventsWrite, AgentExecutionEventsWriteReference, AgentExecutionWrite,
    AgentExecutionWriteReference, AppendAgentExecutionEventsWrite, CreateAgentConversationWrite,
    IAgentRepository, StartAgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, EnvironmentId, IdempotencyRequest, OrganizationId,
    ProjectId, RepositoryError,
};
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
    events: BTreeMap<(OrganizationId, AgentConversationId, u64), AgentExecutionEvent>,
    idempotency: BTreeMap<(String, String), IdempotencyEntry>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

#[derive(Clone)]
struct IdempotencyEntry {
    request_digest: String,
    response: IdempotencyResponse,
}

#[derive(Clone, Copy)]
enum IdempotencyResponse {
    Conversation(AgentConversationWriteReference),
    Execution(AgentExecutionWriteReference),
    Events(AgentExecutionEventsWriteReference),
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
    Ok(Some(entry.response))
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
