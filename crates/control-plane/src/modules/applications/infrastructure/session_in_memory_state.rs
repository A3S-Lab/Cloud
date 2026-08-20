use crate::modules::applications::domain::{
    ApplicationEndUser, ApplicationInvocation, ApplicationMessage, ApplicationSession,
    ApplicationWorkflowEffect, ConversationVariableRevision, OpenApplicationSessionWrite,
    RequestApplicationInvocationWrite,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationSessionId, ConversationVariableRevisionId, OrganizationId, ProjectId,
    RepositoryError, WorkflowRunId,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type EndUserKey = (
    OrganizationId,
    ProjectId,
    ApplicationId,
    ApplicationEndUserId,
);
pub(super) type SessionKey = (
    OrganizationId,
    ProjectId,
    ApplicationId,
    ApplicationSessionId,
);
pub(super) type InvocationKey = (
    OrganizationId,
    ProjectId,
    ApplicationId,
    ApplicationInvocationId,
);
pub(super) type MessageKey = (
    OrganizationId,
    ProjectId,
    ApplicationId,
    ApplicationMessageId,
);
pub(super) type MessageSequenceKey = (
    OrganizationId,
    ProjectId,
    ApplicationId,
    ApplicationSessionId,
    u64,
);
pub(super) type VariableKey = (
    OrganizationId,
    ProjectId,
    ApplicationId,
    ApplicationSessionId,
    ConversationVariableRevisionId,
);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EffectKey {
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
    workflow_run_id: WorkflowRunId,
    step_id: String,
    attempt: u32,
    ordinal: u32,
}

impl EffectKey {
    pub(super) fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        effect: &ApplicationWorkflowEffect,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            application_id,
            session_id,
            workflow_run_id: effect.workflow_run_id,
            step_id: effect.step_id.clone(),
            attempt: effect.attempt,
            ordinal: effect.ordinal,
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub(super) end_users: BTreeMap<EndUserKey, ApplicationEndUser>,
    pub(super) sessions: BTreeMap<SessionKey, ApplicationSession>,
    pub(super) invocations: BTreeMap<InvocationKey, ApplicationInvocation>,
    pub(super) messages: BTreeMap<MessageKey, ApplicationMessage>,
    pub(super) message_sequences: BTreeMap<MessageSequenceKey, ApplicationMessageId>,
    pub(super) variables: BTreeMap<VariableKey, ConversationVariableRevision>,
    pub(super) workflow_runs: BTreeMap<(OrganizationId, WorkflowRunId), InvocationKey>,
    pub(super) effects: BTreeSet<EffectKey>,
    pub(super) final_outputs: BTreeMap<InvocationKey, ApplicationMessageId>,
}

pub(super) fn ensure_message_identity_available(
    state: &State,
    message: &ApplicationMessage,
) -> Result<(), RepositoryError> {
    if state.messages.contains_key(&message_key(message))
        || state
            .message_sequences
            .contains_key(&message_sequence_key(message))
    {
        return Err(RepositoryError::Conflict(
            "Application message identity or sequence is already in use".into(),
        ));
    }
    Ok(())
}

pub(super) fn matches_invocation_request(
    write: &RequestApplicationInvocationWrite,
    current: &ApplicationInvocation,
) -> Result<bool, RepositoryError> {
    current.validate().map_err(RepositoryError::Storage)?;
    Ok(current.organization_id == write.invocation.organization_id
        && current.project_id == write.invocation.project_id
        && current.application_id == write.invocation.application_id
        && current.application_release_id == write.invocation.application_release_id
        && current.application_release_digest == write.invocation.application_release_digest
        && current.session_id == write.invocation.session_id
        && current.id == write.invocation.id
        && current.response_mode == write.invocation.response_mode
        && current.input == write.invocation.input
        && current.input_digest == write.invocation.input_digest
        && current.requested_at == write.invocation.requested_at)
}

pub(super) fn matches_session_open(
    write: &OpenApplicationSessionWrite,
    current: &ApplicationSession,
) -> Result<bool, RepositoryError> {
    current.validate().map_err(RepositoryError::Storage)?;
    Ok(current.organization_id == write.session.organization_id
        && current.project_id == write.session.project_id
        && current.application_id == write.session.application_id
        && current.application_release_id == write.session.application_release_id
        && current.application_release_number == write.session.application_release_number
        && current.application_release_digest == write.session.application_release_digest
        && current.end_user_id == write.session.end_user_id
        && current.id == write.session.id
        && current.interaction_mode == write.session.interaction_mode
        && current.created_at == write.session.created_at)
}

pub(super) fn ensure_run_owner(
    state: &State,
    workflow_run_id: WorkflowRunId,
    expected: &InvocationKey,
) -> Result<(), RepositoryError> {
    match state.workflow_runs.get(&(expected.0, workflow_run_id)) {
        Some(owner) if owner == expected => Ok(()),
        Some(_) => Err(RepositoryError::Conflict(
            "WorkflowRun belongs to another Application invocation".into(),
        )),
        None => Err(RepositoryError::Conflict(
            "WorkflowRun is not bound to an Application invocation".into(),
        )),
    }
}

pub(super) fn store_message(state: &mut State, message: ApplicationMessage) {
    state
        .message_sequences
        .insert(message_sequence_key(&message), message.id);
    state.messages.insert(message_key(&message), message);
}

pub(super) fn end_user_key(value: &ApplicationEndUser) -> EndUserKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.id,
    )
}

pub(super) fn session_key(value: &ApplicationSession) -> SessionKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.id,
    )
}

pub(super) fn session_key_for_invocation(value: &ApplicationInvocation) -> SessionKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.session_id,
    )
}

pub(super) fn session_key_for_message(value: &ApplicationMessage) -> SessionKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.session_id,
    )
}

pub(super) fn session_key_for_revision(value: &ConversationVariableRevision) -> SessionKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.session_id,
    )
}

pub(super) fn invocation_key(value: &ApplicationInvocation) -> InvocationKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.id,
    )
}

pub(super) fn invocation_key_for_message(value: &ApplicationMessage) -> InvocationKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.invocation_id,
    )
}

pub(super) fn message_key(value: &ApplicationMessage) -> MessageKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.id,
    )
}

fn message_sequence_key(value: &ApplicationMessage) -> MessageSequenceKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.session_id,
        value.sequence,
    )
}

pub(super) fn variable_key(value: &ConversationVariableRevision) -> VariableKey {
    (
        value.organization_id,
        value.project_id,
        value.application_id,
        value.session_id,
        value.id,
    )
}
