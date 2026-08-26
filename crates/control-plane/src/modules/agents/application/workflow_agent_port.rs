use super::support::idempotency;
use crate::modules::agents::domain::{
    AgentConversation, AgentConversationCreated, AgentConversationStatus, AgentEventContent,
    AgentExecution, AgentExecutionCancellationRequested, AgentExecutionEventDraft,
    AgentExecutionEventKind, AgentExecutionStarted, AgentExecutionStatus, AgentReleaseBinding,
    CreateAgentConversationWrite, IAgentRepository, RequestAgentExecutionCancellationWrite,
    StartAgentExecutionWrite, MAX_INLINE_AGENT_EVENT_BYTES,
};
use crate::modules::artifacts::IHostedArtifactQueryPort;
use crate::modules::assets::{load_deployable_agent_release, IAssetRepository};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentConversationId, AgentExecutionId, AssetId, AssetReleaseId,
    EnvironmentId, OperationId, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowRunId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;

pub const WORKFLOW_AGENT_CAPABILITY: &str = "agent.execute";
const WORKFLOW_AGENT_MAX_INPUT_BYTES: usize = MAX_INLINE_AGENT_EVENT_BYTES;
const WORKFLOW_AGENT_MAX_OUTPUT_TEXT_BYTES: usize = 192 * 1024;
const WORKFLOW_AGENT_EVENT_PAGE_SIZE: usize = 64;
const WORKFLOW_AGENT_MAX_EVENTS: usize = 4_096;

/// Exact Agents-owned request produced by one immutable Workflow step attempt.
///
/// The Workflow context owns orchestration only. Agents remains authoritative
/// for the conversation, execution, provider binding, event stream, and
/// cancellation lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAgentRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub agent_asset_id: AssetId,
    pub agent_asset_release_id: AssetReleaseId,
    pub agent_release_digest: Sha256Digest,
    pub capability: String,
    pub input: serde_json::Value,
    pub requested_at: DateTime<Utc>,
}

impl WorkflowAgentRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.agent_asset_id.as_uuid().is_nil()
            || self.agent_asset_release_id.as_uuid().is_nil()
            || self.step_attempt == 0
            || self.capability != WORKFLOW_AGENT_CAPABILITY
            || self.step_id.is_empty()
            || self.step_id.len() > 96
            || !self
                .step_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || self.requested_at != canonical_timestamp(self.requested_at)
            || Sha256Digest::parse(self.plan_digest.as_str())? != self.plan_digest
            || Sha256Digest::parse(self.agent_release_digest.as_str())? != self.agent_release_digest
        {
            return Err("Workflow Agent request authority is invalid".into());
        }
        let encoded = serde_json::to_vec(&self.input)
            .map_err(|error| format!("Workflow Agent input is not serializable: {error}"))?;
        if encoded.len() > WORKFLOW_AGENT_MAX_INPUT_BYTES {
            return Err("Workflow Agent input exceeds its bound".into());
        }
        Ok(())
    }

    fn idempotency_key(&self, suffix: &str) -> String {
        format!(
            "workflow:{}:step:{}:attempt:{}:{suffix}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }

    fn conversation_idempotency(
        &self,
    ) -> ApplicationResult<crate::modules::shared_kernel::domain::IdempotencyRequest> {
        idempotency(
            format!(
                "organizations/{}/projects/{}/environments/{}/agent-conversations",
                self.organization_id, self.project_id, self.environment_id
            ),
            self.idempotency_key("conversation"),
            self,
        )
    }

    fn execution_idempotency(
        &self,
        conversation_id: AgentConversationId,
    ) -> ApplicationResult<crate::modules::shared_kernel::domain::IdempotencyRequest> {
        idempotency(
            format!(
                "organizations/{}/agent-conversations/{conversation_id}/executions",
                self.organization_id
            ),
            self.idempotency_key("execution"),
            &(conversation_id, self),
        )
    }

    fn cancellation_idempotency(
        &self,
        execution_id: AgentExecutionId,
    ) -> ApplicationResult<crate::modules::shared_kernel::domain::IdempotencyRequest> {
        idempotency(
            format!(
                "organizations/{}/agent-executions/{execution_id}/cancel",
                self.organization_id
            ),
            self.idempotency_key("cancel"),
            &(
                execution_id,
                self.workflow_run_id,
                &self.step_id,
                self.step_attempt,
            ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowAgentTerminalObservation {
    pub execution: AgentExecution,
    pub output_text: String,
    pub terminal_event_sequence: u64,
}

#[async_trait]
pub trait IWorkflowAgentPort: Send + Sync {
    async fn start_or_adopt(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentExecution>;

    async fn adopt(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<Option<AgentExecution>>;

    async fn request_cancellation(
        &self,
        request: &WorkflowAgentRequest,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<AgentExecution>>;

    async fn terminal_observation(
        &self,
        request: &WorkflowAgentRequest,
        execution: &AgentExecution,
    ) -> ApplicationResult<Option<WorkflowAgentTerminalObservation>>;
}

#[derive(Clone)]
pub struct WorkflowAgentApplicationService {
    environments: Arc<dyn IEnvironmentRepository>,
    agents: Arc<dyn IAgentRepository>,
    assets: Arc<dyn IAssetRepository>,
    artifacts: Arc<dyn IHostedArtifactQueryPort>,
}

impl WorkflowAgentApplicationService {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        agents: Arc<dyn IAgentRepository>,
        assets: Arc<dyn IAssetRepository>,
        artifacts: Arc<dyn IHostedArtifactQueryPort>,
    ) -> Self {
        Self {
            environments,
            agents,
            assets,
            artifacts,
        }
    }

    async fn adopt_conversation(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<Option<AgentConversation>> {
        let idempotency = request.conversation_idempotency()?;
        let conversation = self.agents.replay_conversation(&idempotency).await?;
        match conversation {
            Some(conversation)
                if conversation.organization_id == request.organization_id
                    && conversation.project_id == request.project_id
                    && conversation.environment_id == request.environment_id
                    && conversation.created_at == request.requested_at =>
            {
                Ok(Some(conversation))
            }
            Some(_) => Err(ApplicationError::Conflict(
                "adopted Workflow Agent conversation changed its immutable authority".into(),
            )),
            None => Ok(None),
        }
    }

    async fn create_or_adopt_conversation(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentConversation> {
        if let Some(conversation) = self.adopt_conversation(request).await? {
            return Ok(conversation);
        }
        if self
            .environments
            .find(
                request.organization_id,
                request.project_id,
                request.environment_id,
            )
            .await?
            .is_none()
        {
            return Err(ApplicationError::NotFound("environment not found".into()));
        }
        let conversation = AgentConversation::create(
            request.organization_id,
            request.project_id,
            request.environment_id,
            AgentConversationId::new(),
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let event =
            AgentConversationCreated::envelope(&conversation, request.workflow_run_id.as_uuid())
                .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        match self
            .agents
            .create_conversation(CreateAgentConversationWrite {
                conversation,
                event,
                idempotency: request.conversation_idempotency()?,
            })
            .await
        {
            Ok(write) => self
                .adopt_conversation(request)
                .await?
                .filter(|adopted| adopted == &write.conversation)
                .ok_or_else(|| {
                    ApplicationError::Conflict(
                        "Workflow Agent conversation commit could not be adopted".into(),
                    )
                }),
            Err(crate::modules::shared_kernel::domain::RepositoryError::IdempotencyConflict) => {
                self.adopt_conversation(request).await?.ok_or_else(|| {
                    ApplicationError::Conflict(
                        "Workflow Agent conversation idempotency authority conflicted".into(),
                    )
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn validate_adopted(
        &self,
        request: &WorkflowAgentRequest,
        conversation: &AgentConversation,
        execution: AgentExecution,
    ) -> ApplicationResult<AgentExecution> {
        if conversation.organization_id != request.organization_id
            || conversation.project_id != request.project_id
            || conversation.environment_id != request.environment_id
            || execution.organization_id != request.organization_id
            || execution.conversation_id != conversation.id
            || execution.agent.asset_id() != request.agent_asset_id
            || execution.agent.asset_release_id() != request.agent_asset_release_id
            || execution.agent.artifact_digest() != &request.agent_release_digest
            || execution.requested_at != request.requested_at
        {
            return Err(ApplicationError::Conflict(
                "adopted Workflow Agent execution changed its immutable authority".into(),
            ));
        }
        Ok(execution)
    }

    async fn admit_release(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentReleaseBinding> {
        let deployable = load_deployable_agent_release(
            self.assets.as_ref(),
            self.artifacts.as_ref(),
            request.organization_id,
            request.agent_asset_id,
            request.agent_asset_release_id,
        )
        .await?;
        if deployable.artifact_digest() != request.agent_release_digest.as_str() {
            return Err(ApplicationError::Conflict(
                "Workflow Agent release digest does not match its exact published artifact".into(),
            ));
        }
        AgentReleaseBinding::new(
            request.organization_id,
            deployable.asset_id(),
            deployable.asset_release_id(),
            deployable.build_run_id(),
            deployable.artifact_uri(),
            Sha256Digest::parse(deployable.artifact_digest())
                .map_err(ApplicationError::Internal)?,
            deployable.artifact_media_type(),
            deployable.artifact_size_bytes(),
        )
        .map_err(ApplicationError::Internal)
    }

    async fn start_execution(
        &self,
        request: &WorkflowAgentRequest,
        conversation: &AgentConversation,
        binding: AgentReleaseBinding,
    ) -> ApplicationResult<AgentExecution> {
        if conversation.status != AgentConversationStatus::Active {
            return Err(ApplicationError::Conflict(
                "closed Workflow Agent conversation cannot start an execution".into(),
            ));
        }
        let execution = AgentExecution::create(
            request.organization_id,
            conversation.id,
            AgentExecutionId::new(),
            OperationId::new(),
            binding,
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let initial_event = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ExecutionRequested,
            AgentEventContent::inline_json(request.input.clone())
                .map_err(ApplicationError::Invalid)?,
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let event = AgentExecutionStarted::envelope(&execution, request.workflow_run_id.as_uuid())
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .agents
            .start_execution(StartAgentExecutionWrite {
                execution,
                initial_event,
                event,
                idempotency: request.execution_idempotency(conversation.id)?,
            })
            .await?;
        self.validate_adopted(request, conversation, write.execution)
            .await
    }
}

#[async_trait]
impl IWorkflowAgentPort for WorkflowAgentApplicationService {
    async fn start_or_adopt(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentExecution> {
        request.validate().map_err(ApplicationError::Invalid)?;
        if let Some(execution) = self.adopt(request).await? {
            return Ok(execution);
        }
        let binding = self.admit_release(request).await?;
        let conversation = self.create_or_adopt_conversation(request).await?;
        match self.start_execution(request, &conversation, binding).await {
            Ok(execution) => Ok(execution),
            Err(error @ ApplicationError::Conflict(_)) => self.adopt(request).await?.ok_or(error),
            Err(error) => Err(error),
        }
    }

    async fn adopt(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<Option<AgentExecution>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let Some(conversation) = self.adopt_conversation(request).await? else {
            return Ok(None);
        };
        let idempotency = request.execution_idempotency(conversation.id)?;
        let Some(execution) = self.agents.replay_execution(&idempotency).await? else {
            return Ok(None);
        };
        self.validate_adopted(request, &conversation, execution)
            .await
            .map(Some)
    }

    async fn request_cancellation(
        &self,
        request: &WorkflowAgentRequest,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<AgentExecution>> {
        let Some(mut execution) = self.adopt(request).await? else {
            return Ok(None);
        };
        if execution.status.is_terminal() || execution.status == AgentExecutionStatus::Cancelling {
            return Ok(Some(execution));
        }
        let idempotency = request.cancellation_idempotency(execution.id)?;
        if let Some(replayed) = self.agents.replay_execution(&idempotency).await? {
            let conversation = self.adopt_conversation(request).await?.ok_or_else(|| {
                ApplicationError::Conflict("Workflow Agent conversation disappeared".into())
            })?;
            return self
                .validate_adopted(request, &conversation, replayed)
                .await
                .map(Some);
        }
        let expected_version = execution.aggregate_version;
        let requested_at = canonical_timestamp(requested_at.max(execution.updated_at));
        execution
            .request_cancellation(requested_at)
            .map_err(ApplicationError::Conflict)?;
        let event = AgentExecutionCancellationRequested::envelope(
            &execution,
            request.workflow_run_id.as_uuid(),
        )
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .agents
            .request_cancellation(RequestAgentExecutionCancellationWrite {
                execution,
                expected_version,
                event,
                idempotency,
            })
            .await?;
        let conversation = self.adopt_conversation(request).await?.ok_or_else(|| {
            ApplicationError::Conflict("Workflow Agent conversation disappeared".into())
        })?;
        self.validate_adopted(request, &conversation, write.execution)
            .await
            .map(Some)
    }

    async fn terminal_observation(
        &self,
        request: &WorkflowAgentRequest,
        execution: &AgentExecution,
    ) -> ApplicationResult<Option<WorkflowAgentTerminalObservation>> {
        let Some(current) = self.adopt(request).await? else {
            return Ok(None);
        };
        if &current != execution {
            return Err(ApplicationError::Conflict(
                "Workflow Agent terminal observation used a stale execution".into(),
            ));
        }
        if !current.status.is_terminal() {
            return Ok(None);
        }
        let mut after_sequence = None;
        let mut event_count = 0usize;
        let mut output_text = String::new();
        let mut terminal_event = None;
        loop {
            let page = self
                .agents
                .list_events(
                    request.organization_id,
                    current.conversation_id,
                    after_sequence,
                    WORKFLOW_AGENT_EVENT_PAGE_SIZE,
                )
                .await?;
            if page.is_empty() {
                break;
            }
            for event in &page {
                event.validate().map_err(ApplicationError::Internal)?;
                let expected_sequence =
                    after_sequence.unwrap_or(0).checked_add(1).ok_or_else(|| {
                        ApplicationError::Conflict(
                            "Workflow Agent semantic event sequence overflowed".into(),
                        )
                    })?;
                if event.execution_id != current.id || event.sequence != expected_sequence {
                    return Err(ApplicationError::Conflict(
                        "Workflow Agent semantic event authority drifted".into(),
                    ));
                }
                event_count = event_count.checked_add(1).ok_or_else(|| {
                    ApplicationError::Conflict("Workflow Agent event count overflowed".into())
                })?;
                if event_count > WORKFLOW_AGENT_MAX_EVENTS {
                    return Err(ApplicationError::Conflict(
                        "Workflow Agent semantic event stream exceeds its bound".into(),
                    ));
                }
                if event.kind == AgentExecutionEventKind::ModelOutput {
                    let text = event
                        .content
                        .value()
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            ApplicationError::Conflict(
                                "Workflow Agent model output omitted text".into(),
                            )
                        })?;
                    if output_text.len().saturating_add(text.len())
                        > WORKFLOW_AGENT_MAX_OUTPUT_TEXT_BYTES
                    {
                        return Err(ApplicationError::Conflict(
                            "Workflow Agent output exceeds its bound".into(),
                        ));
                    }
                    output_text.push_str(text);
                }
                if matches!(
                    event.kind,
                    AgentExecutionEventKind::ExecutionCompleted
                        | AgentExecutionEventKind::ExecutionFailed
                        | AgentExecutionEventKind::ExecutionCancelled
                ) {
                    terminal_event = Some((event.kind, event.sequence));
                }
                after_sequence = Some(event.sequence);
            }
            if page.len() < WORKFLOW_AGENT_EVENT_PAGE_SIZE {
                break;
            }
        }
        let (terminal_kind, terminal_event_sequence) = terminal_event.ok_or_else(|| {
            ApplicationError::Conflict(
                "terminal Workflow Agent execution has no semantic terminal event".into(),
            )
        })?;
        let expected_kind = match current.status {
            AgentExecutionStatus::Succeeded => AgentExecutionEventKind::ExecutionCompleted,
            AgentExecutionStatus::Failed => AgentExecutionEventKind::ExecutionFailed,
            AgentExecutionStatus::Cancelled => AgentExecutionEventKind::ExecutionCancelled,
            _ => {
                return Err(ApplicationError::Conflict(
                    "non-terminal Workflow Agent execution reached terminal projection".into(),
                ))
            }
        };
        if terminal_kind != expected_kind || after_sequence != Some(terminal_event_sequence) {
            return Err(ApplicationError::Conflict(
                "Workflow Agent semantic terminal event disagrees with its aggregate".into(),
            ));
        }
        Ok(Some(WorkflowAgentTerminalObservation {
            execution: current,
            output_text,
            terminal_event_sequence,
        }))
    }
}
