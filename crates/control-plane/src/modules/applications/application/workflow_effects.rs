use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, ApplicationInvocation, ApplicationInvocationStatus,
    ApplicationMessage, ApplicationMessageKind, ApplicationSession, ApplicationWorkflowEffect,
    ConversationVariableRevision, IApplicationSessionRepository,
    APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES, APPLICATION_MESSAGE_MAX_BYTES,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, ApplicationId, ApplicationInvocationId,
    ApplicationReleaseId, ApplicationSessionId, ConversationVariableRevisionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest, WorkflowRunId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

const WORKFLOW_APPLICATION_EFFECT_REQUEST_MAX_BYTES: usize = 4 * 1024;

/// Minimal reference by which Workflow can address an Applications-owned
/// invocation. Project, Application, release, session, and invocation
/// identities are deliberately resolved from the durable WorkflowRun binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationRunReference {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
}

impl WorkflowApplicationRunReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.workflow_run_id.as_uuid().is_nil() {
            return Err("Workflow Applications run reference is invalid".into());
        }
        Ok(())
    }
}

/// Replay-stable identity and time for one Applications-owned Workflow
/// semantic effect. Flow/Workflow retain execution and attempt history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationEffectRequest {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub step_id: String,
    pub step_attempt: u32,
    pub effect_ordinal: u32,
    pub occurred_at: DateTime<Utc>,
}

impl WorkflowApplicationEffectRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.run_reference().validate()?;
        self.effect()?.validate()?;
        if self.occurred_at != canonical_timestamp(self.occurred_at) {
            return Err("Workflow Applications effect time is not canonical".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_APPLICATION_EFFECT_REQUEST_MAX_BYTES,
            "Workflow Applications effect request",
        )?;
        Ok(())
    }

    pub fn run_reference(&self) -> WorkflowApplicationRunReference {
        WorkflowApplicationRunReference {
            organization_id: self.organization_id,
            workflow_run_id: self.workflow_run_id,
        }
    }

    pub fn effect(&self) -> Result<ApplicationWorkflowEffect, String> {
        ApplicationWorkflowEffect::new(
            self.workflow_run_id,
            self.step_id.clone(),
            self.step_attempt,
            self.effect_ordinal,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationMessageRequest {
    pub effect: WorkflowApplicationEffectRequest,
    pub content: Value,
}

impl WorkflowApplicationMessageRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.effect.validate()?;
        canonical_json_bounded(
            &self.content,
            APPLICATION_MESSAGE_MAX_BYTES,
            "Workflow Application message content",
        )?;
        Ok(())
    }
}

/// Optimistic Applications variable evidence returned to and later presented
/// by Workflow. The effect identity is the idempotency key; this version is
/// the compare-and-swap authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableVersion {
    pub revision_id: ConversationVariableRevisionId,
    pub revision_number: u64,
    pub values_digest: Sha256Digest,
}

impl WorkflowApplicationVariableVersion {
    pub fn validate(&self) -> Result<(), String> {
        if self.revision_id.as_uuid().is_nil()
            || self.revision_number == 0
            || Sha256Digest::parse(self.values_digest.as_str())? != self.values_digest
        {
            return Err("Workflow Application variable version is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableWriteRequest {
    pub effect: WorkflowApplicationEffectRequest,
    pub expected: WorkflowApplicationVariableVersion,
    pub values: Value,
}

impl WorkflowApplicationVariableWriteRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.effect.validate()?;
        self.expected.validate()?;
        variable_values_digest(&self.values)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationTerminalRequest {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub status: ApplicationInvocationStatus,
    pub completed_at: DateTime<Utc>,
}

impl WorkflowApplicationTerminalRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.run_reference().validate()?;
        if !self.status.is_terminal() || self.completed_at != canonical_timestamp(self.completed_at)
        {
            return Err("Workflow Application terminal observation is invalid".into());
        }
        Ok(())
    }

    pub fn run_reference(&self) -> WorkflowApplicationRunReference {
        WorkflowApplicationRunReference {
            organization_id: self.organization_id,
            workflow_run_id: self.workflow_run_id,
        }
    }
}

/// Exact Applications-owned conversation-variable snapshot supplied to
/// Workflow without moving the values into Workflow's durable state owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationVariableSnapshot {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub workflow_run_id: WorkflowRunId,
    pub version: WorkflowApplicationVariableVersion,
    pub values: Value,
}

impl WorkflowApplicationVariableSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        self.version.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || variable_values_digest(&self.values)? != self.version.values_digest
        {
            return Err("Workflow Application variable snapshot is invalid".into());
        }
        Ok(())
    }
}

/// Typed internal Workflow consumer boundary. It exposes Applications-owned
/// semantic effects only; it is not an end-user delivery or execution API.
#[async_trait]
pub trait IWorkflowApplicationEffectsPort: Send + Sync {
    async fn read_conversation_variables(
        &self,
        reference: &WorkflowApplicationRunReference,
    ) -> ApplicationResult<WorkflowApplicationVariableSnapshot>;

    async fn append_answer(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>>;

    async fn append_final_output(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>>;

    async fn advance_conversation_variables(
        &self,
        request: &WorkflowApplicationVariableWriteRequest,
    ) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>>;

    async fn observe_terminal(
        &self,
        request: &WorkflowApplicationTerminalRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationInvocation>>;
}

#[derive(Clone)]
pub struct WorkflowApplicationEffectsService {
    sessions: Arc<dyn IApplicationSessionRepository>,
}

impl WorkflowApplicationEffectsService {
    pub fn new(sessions: Arc<dyn IApplicationSessionRepository>) -> Self {
        Self { sessions }
    }

    async fn resolve_binding(
        &self,
        reference: &WorkflowApplicationRunReference,
    ) -> ApplicationResult<WorkflowApplicationBinding> {
        reference.validate().map_err(ApplicationError::Invalid)?;
        let invocation = self
            .sessions
            .find_invocation_for_workflow_run(reference.organization_id, reference.workflow_run_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound(
                    "Application invocation for WorkflowRun not found".into(),
                )
            })?;
        invocation.validate().map_err(ApplicationError::Internal)?;
        if invocation.organization_id != reference.organization_id
            || invocation.workflow_run_id != Some(reference.workflow_run_id)
        {
            return Err(ApplicationError::Internal(
                "Application WorkflowRun invocation binding drifted".into(),
            ));
        }
        let session = self
            .sessions
            .find_session(
                invocation.organization_id,
                invocation.project_id,
                invocation.application_id,
                invocation.session_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::Internal(
                    "Application WorkflowRun session binding is missing".into(),
                )
            })?;
        session.validate().map_err(ApplicationError::Internal)?;
        if session.organization_id != invocation.organization_id
            || session.project_id != invocation.project_id
            || session.application_id != invocation.application_id
            || session.application_release_id != invocation.application_release_id
            || session.application_release_digest != invocation.application_release_digest
            || session.id != invocation.session_id
        {
            return Err(ApplicationError::Internal(
                "Application WorkflowRun session binding drifted".into(),
            ));
        }
        Ok(WorkflowApplicationBinding {
            invocation,
            session,
        })
    }

    async fn write_message(
        &self,
        request: &WorkflowApplicationMessageRequest,
        kind: ApplicationMessageKind,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let binding = self
            .resolve_binding(&request.effect.run_reference())
            .await?;
        let effect = request.effect.effect().map_err(ApplicationError::Invalid)?;
        let message_id =
            ApplicationMessage::workflow_frame_id(binding.invocation.id, kind, &effect)
                .map_err(ApplicationError::Invalid)?;
        if let Some(current) = self
            .sessions
            .find_message(
                binding.invocation.organization_id,
                binding.invocation.project_id,
                binding.invocation.application_id,
                message_id,
            )
            .await?
        {
            return replay_message(current, &binding, request, kind);
        }

        let message = ApplicationMessage::workflow_frame(
            &binding.session,
            &binding.invocation,
            kind,
            effect,
            request.content.clone(),
            request.effect.occurred_at,
        )
        .map_err(ApplicationError::Conflict)?;
        let write = AppendApplicationMessageWrite {
            message: message.clone(),
            expected_session_version: binding.session.aggregate_version,
        };
        match self.sessions.append_message(write).await {
            Ok(result) if result.value == message => Ok(result),
            Ok(_) => Err(ApplicationError::Internal(
                "Workflow Application message repository returned drifted state".into(),
            )),
            Err(error) => {
                self.recover_message(error, &binding, request, kind, message_id)
                    .await
            }
        }
    }

    async fn recover_message(
        &self,
        write_error: RepositoryError,
        binding: &WorkflowApplicationBinding,
        request: &WorkflowApplicationMessageRequest,
        kind: ApplicationMessageKind,
        message_id: crate::modules::shared_kernel::domain::ApplicationMessageId,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        match self
            .sessions
            .find_message(
                binding.invocation.organization_id,
                binding.invocation.project_id,
                binding.invocation.application_id,
                message_id,
            )
            .await
        {
            Ok(Some(current)) => replay_message(current, binding, request, kind),
            Ok(None) => Err(write_error.into()),
            Err(read_error) => Err(ApplicationError::Unavailable(format!(
                "Workflow Application message write failed ({write_error}); recovery read failed: {read_error}"
            ))),
        }
    }

    async fn recover_variables(
        &self,
        write_error: RepositoryError,
        binding: &WorkflowApplicationBinding,
        request: &WorkflowApplicationVariableWriteRequest,
        revision_id: ConversationVariableRevisionId,
    ) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>> {
        match self
            .sessions
            .find_variable_revision(
                binding.invocation.organization_id,
                binding.invocation.project_id,
                binding.invocation.application_id,
                binding.invocation.session_id,
                revision_id,
            )
            .await
        {
            Ok(Some(current)) => replay_variables(current, binding, request),
            Ok(None) => Err(write_error.into()),
            Err(read_error) => Err(ApplicationError::Unavailable(format!(
                "Workflow Application variable write failed ({write_error}); recovery read failed: {read_error}"
            ))),
        }
    }
}

#[async_trait]
impl IWorkflowApplicationEffectsPort for WorkflowApplicationEffectsService {
    async fn read_conversation_variables(
        &self,
        reference: &WorkflowApplicationRunReference,
    ) -> ApplicationResult<WorkflowApplicationVariableSnapshot> {
        let binding = self.resolve_binding(reference).await?;
        let revision = self
            .sessions
            .find_variable_revision(
                binding.session.organization_id,
                binding.session.project_id,
                binding.session.application_id,
                binding.session.id,
                binding.session.current_variable_revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::Internal("Application session variable head is missing".into())
            })?;
        revision.validate().map_err(ApplicationError::Internal)?;
        if revision.organization_id != binding.session.organization_id
            || revision.project_id != binding.session.project_id
            || revision.application_id != binding.session.application_id
            || revision.application_release_id != binding.session.application_release_id
            || revision.application_release_digest != binding.session.application_release_digest
            || revision.session_id != binding.session.id
            || revision.id != binding.session.current_variable_revision_id
            || revision.revision_number != binding.session.current_variable_revision_number
            || revision.values_digest != binding.session.current_variable_digest
        {
            return Err(ApplicationError::Internal(
                "Application session variable head drifted".into(),
            ));
        }
        let snapshot = WorkflowApplicationVariableSnapshot {
            organization_id: binding.invocation.organization_id,
            project_id: binding.invocation.project_id,
            application_id: binding.invocation.application_id,
            application_release_id: binding.invocation.application_release_id,
            application_release_digest: binding.invocation.application_release_digest.clone(),
            session_id: binding.invocation.session_id,
            invocation_id: binding.invocation.id,
            workflow_run_id: reference.workflow_run_id,
            version: WorkflowApplicationVariableVersion {
                revision_id: revision.id,
                revision_number: revision.revision_number,
                values_digest: revision.values_digest.clone(),
            },
            values: revision.values,
        };
        snapshot.validate().map_err(ApplicationError::Internal)?;
        Ok(snapshot)
    }

    async fn append_answer(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.write_message(request, ApplicationMessageKind::Answer)
            .await
    }

    async fn append_final_output(
        &self,
        request: &WorkflowApplicationMessageRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
        self.write_message(request, ApplicationMessageKind::FinalOutput)
            .await
    }

    async fn advance_conversation_variables(
        &self,
        request: &WorkflowApplicationVariableWriteRequest,
    ) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let binding = self
            .resolve_binding(&request.effect.run_reference())
            .await?;
        let effect = request.effect.effect().map_err(ApplicationError::Invalid)?;
        let revision_id = ConversationVariableRevision::successor_id(binding.session.id, &effect)
            .map_err(ApplicationError::Invalid)?;
        if let Some(current) = self
            .sessions
            .find_variable_revision(
                binding.invocation.organization_id,
                binding.invocation.project_id,
                binding.invocation.application_id,
                binding.invocation.session_id,
                revision_id,
            )
            .await?
        {
            return replay_variables(current, &binding, request);
        }
        if binding.session.current_variable_revision_id != request.expected.revision_id
            || binding.session.current_variable_revision_number != request.expected.revision_number
            || binding.session.current_variable_digest != request.expected.values_digest
        {
            return Err(ApplicationError::Conflict(
                "Workflow Application variable write used a stale revision".into(),
            ));
        }
        let parent = self
            .sessions
            .find_variable_revision(
                binding.session.organization_id,
                binding.session.project_id,
                binding.session.application_id,
                binding.session.id,
                request.expected.revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::Internal("Application session variable parent is missing".into())
            })?;
        if parent.revision_number != request.expected.revision_number
            || parent.values_digest != request.expected.values_digest
        {
            return Err(ApplicationError::Internal(
                "Application session variable parent drifted".into(),
            ));
        }
        if variable_values_digest(&request.values).map_err(ApplicationError::Invalid)?
            == parent.values_digest
        {
            return Err(ApplicationError::Invalid(
                "Workflow Application variable assignment must change values".into(),
            ));
        }
        let revision = ConversationVariableRevision::successor(
            &parent,
            effect,
            request.values.clone(),
            request.effect.occurred_at,
        )
        .map_err(ApplicationError::Conflict)?;
        let write = AdvanceConversationVariablesWrite {
            revision: revision.clone(),
            expected_session_version: binding.session.aggregate_version,
        };
        match self.sessions.advance_variables(write).await {
            Ok(result) if result.value == revision => Ok(result),
            Ok(_) => Err(ApplicationError::Internal(
                "Workflow Application variable repository returned drifted state".into(),
            )),
            Err(error) => {
                self.recover_variables(error, &binding, request, revision_id)
                    .await
            }
        }
    }

    async fn observe_terminal(
        &self,
        request: &WorkflowApplicationTerminalRequest,
    ) -> ApplicationResult<IdempotentWrite<ApplicationInvocation>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let binding = self.resolve_binding(&request.run_reference()).await?;
        if binding.invocation.status.is_terminal() {
            return replay_terminal(binding.invocation, request);
        }
        let terminal = binding
            .invocation
            .observe_terminal(
                binding.invocation.aggregate_version,
                request.status,
                request.completed_at,
            )
            .map_err(ApplicationError::Conflict)?;
        let write = AdvanceApplicationInvocationWrite {
            invocation: terminal.clone(),
            expected_version: binding.invocation.aggregate_version,
        };
        match self.sessions.advance_invocation(write).await {
            Ok(result) if result.value == terminal => Ok(result),
            Ok(_) => Err(ApplicationError::Internal(
                "Workflow Application terminal repository returned drifted state".into(),
            )),
            Err(write_error) => match self
                .sessions
                .find_invocation_for_workflow_run(
                    request.organization_id,
                    request.workflow_run_id,
                )
                .await
            {
                Ok(Some(current)) if current == binding.invocation => Err(write_error.into()),
                Ok(Some(current)) => replay_terminal(current, request),
                Ok(None) => Err(write_error.into()),
                Err(read_error) => Err(ApplicationError::Unavailable(format!(
                    "Workflow Application terminal write failed ({write_error}); recovery read failed: {read_error}"
                ))),
            },
        }
    }
}

struct WorkflowApplicationBinding {
    invocation: ApplicationInvocation,
    session: ApplicationSession,
}

fn replay_message(
    current: ApplicationMessage,
    binding: &WorkflowApplicationBinding,
    request: &WorkflowApplicationMessageRequest,
    kind: ApplicationMessageKind,
) -> ApplicationResult<IdempotentWrite<ApplicationMessage>> {
    current.validate().map_err(ApplicationError::Internal)?;
    let effect = request.effect.effect().map_err(ApplicationError::Invalid)?;
    let expected_id = ApplicationMessage::workflow_frame_id(binding.invocation.id, kind, &effect)
        .map_err(ApplicationError::Invalid)?;
    if current.organization_id != binding.invocation.organization_id
        || current.project_id != binding.invocation.project_id
        || current.application_id != binding.invocation.application_id
        || current.application_release_id != binding.invocation.application_release_id
        || current.application_release_digest != binding.invocation.application_release_digest
        || current.session_id != binding.invocation.session_id
        || current.invocation_id != binding.invocation.id
        || current.id != expected_id
        || current.kind != kind
        || current.workflow_effect.as_ref() != Some(&effect)
        || current.content != request.content
        || current.created_at != request.effect.occurred_at
    {
        return Err(ApplicationError::Conflict(
            "Workflow Application message effect replay changed content or authority".into(),
        ));
    }
    Ok(IdempotentWrite {
        value: current,
        replayed: true,
    })
}

fn replay_variables(
    current: ConversationVariableRevision,
    binding: &WorkflowApplicationBinding,
    request: &WorkflowApplicationVariableWriteRequest,
) -> ApplicationResult<IdempotentWrite<ConversationVariableRevision>> {
    current.validate().map_err(ApplicationError::Internal)?;
    let effect = request.effect.effect().map_err(ApplicationError::Invalid)?;
    let expected_revision_number =
        request
            .expected
            .revision_number
            .checked_add(1)
            .ok_or_else(|| {
                ApplicationError::Invalid(
                    "Workflow Application variable revision is exhausted".into(),
                )
            })?;
    let expected_id = ConversationVariableRevision::successor_id(binding.session.id, &effect)
        .map_err(ApplicationError::Invalid)?;
    if current.organization_id != binding.invocation.organization_id
        || current.project_id != binding.invocation.project_id
        || current.application_id != binding.invocation.application_id
        || current.application_release_id != binding.invocation.application_release_id
        || current.application_release_digest != binding.invocation.application_release_digest
        || current.session_id != binding.session.id
        || current.id != expected_id
        || current.revision_number != expected_revision_number
        || current.parent_revision_id != Some(request.expected.revision_id)
        || current.parent_digest.as_ref() != Some(&request.expected.values_digest)
        || current.source_effect.as_ref() != Some(&effect)
        || current.values != request.values
        || current.created_at != request.effect.occurred_at
    {
        return Err(ApplicationError::Conflict(
            "Workflow Application variable effect replay changed values or authority".into(),
        ));
    }
    Ok(IdempotentWrite {
        value: current,
        replayed: true,
    })
}

fn replay_terminal(
    current: ApplicationInvocation,
    request: &WorkflowApplicationTerminalRequest,
) -> ApplicationResult<IdempotentWrite<ApplicationInvocation>> {
    current.validate().map_err(ApplicationError::Internal)?;
    if current.organization_id != request.organization_id
        || current.workflow_run_id != Some(request.workflow_run_id)
        || current.status != request.status
        || current.completed_at != Some(request.completed_at)
    {
        return Err(ApplicationError::Conflict(
            "Workflow Application terminal observation replay drifted".into(),
        ));
    }
    Ok(IdempotentWrite {
        value: current,
        replayed: true,
    })
}

fn variable_values_digest(values: &Value) -> Result<Sha256Digest, String> {
    if !values.is_object() {
        return Err("Workflow Application variables must be a JSON object".into());
    }
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        values,
        APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES,
        "Workflow Application variables",
    )?))
}
