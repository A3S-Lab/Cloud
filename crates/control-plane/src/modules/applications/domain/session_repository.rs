use super::{
    ApplicationEndUser, ApplicationInvocation, ApplicationInvocationStatus,
    ApplicationInvocationWorkflowAuthority, ApplicationMessage, ApplicationMessageKind,
    ApplicationRelease, ApplicationSession, ApplicationSessionStatus, ConversationVariableRevision,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationSessionId,
    ConversationVariableRevisionId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

/// Atomic opening of one release-pinned Applications session and its initial
/// variable snapshot. The release is admission evidence, not duplicated state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApplicationSessionWrite {
    pub release: ApplicationRelease,
    pub end_user: ApplicationEndUser,
    pub session: ApplicationSession,
    pub initial_variables: ConversationVariableRevision,
}

impl OpenApplicationSessionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.release.validate()?;
        self.end_user.validate_release(&self.release)?;
        self.initial_variables.validate()?;
        self.session.validate_release(&self.release)?;
        let expected = ApplicationSession::create(
            self.session.id,
            &self.release,
            &self.end_user,
            &self.initial_variables,
            self.session.created_at,
        )?;
        if expected != self.session {
            return Err("Application session opening changed initial state".into());
        }
        Ok(())
    }

    pub fn matches_replay(
        &self,
        current: &ApplicationSession,
        end_user: &ApplicationEndUser,
        initial_variables: &ConversationVariableRevision,
    ) -> Result<bool, String> {
        current.validate()?;
        end_user.validate()?;
        initial_variables.validate()?;
        Ok(current.organization_id == self.session.organization_id
            && current.project_id == self.session.project_id
            && current.application_id == self.session.application_id
            && current.application_release_id == self.session.application_release_id
            && current.application_release_number == self.session.application_release_number
            && current.application_release_digest == self.session.application_release_digest
            && current.end_user_id == self.session.end_user_id
            && current.id == self.session.id
            && current.interaction_mode == self.session.interaction_mode
            && current.created_at == self.session.created_at
            && end_user == &self.end_user
            && initial_variables == &self.initial_variables)
    }
}

/// Atomic creation of one invocation request and its first channel message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestApplicationInvocationWrite {
    pub invocation: ApplicationInvocation,
    pub workflow_authority: ApplicationInvocationWorkflowAuthority,
    pub input_message: ApplicationMessage,
    pub expected_session_version: u64,
}

impl RequestApplicationInvocationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.invocation.validate()?;
        self.workflow_authority.validate_against(&self.invocation)?;
        self.input_message.validate()?;
        if self.expected_session_version == 0
            || self.invocation.status != ApplicationInvocationStatus::Requested
            || self.invocation.aggregate_version != 1
            || self.invocation.workflow_run_id.is_some()
            || self.input_message.kind != ApplicationMessageKind::Input
            || self.input_message.created_at != self.invocation.requested_at
        {
            return Err("initial Application invocation write is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, session: &ApplicationSession) -> Result<(), String> {
        self.validate()?;
        if self.expected_session_version != session.aggregate_version {
            return Err("Application invocation request used a stale session version".into());
        }
        self.input_message
            .validate_against(session, &self.invocation)?;
        session.append_message(self.expected_session_version, &self.input_message)?;
        Ok(())
    }

    pub fn matches_replay(
        &self,
        current: &ApplicationInvocation,
        workflow_authority: &ApplicationInvocationWorkflowAuthority,
        input_message: &ApplicationMessage,
    ) -> Result<bool, String> {
        current.validate()?;
        input_message.validate()?;
        Ok(current.organization_id == self.invocation.organization_id
            && current.project_id == self.invocation.project_id
            && current.application_id == self.invocation.application_id
            && current.application_release_id == self.invocation.application_release_id
            && current.application_release_digest == self.invocation.application_release_digest
            && current.session_id == self.invocation.session_id
            && current.id == self.invocation.id
            && current.response_mode == self.invocation.response_mode
            && current.input == self.invocation.input
            && current.input_digest == self.invocation.input_digest
            && current.requested_at == self.invocation.requested_at
            && workflow_authority == &self.workflow_authority
            && input_message == &self.input_message)
    }
}

/// One optimistic invocation transition. Exact immediate retries replay the
/// already-stored successor; later stale transitions conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvanceApplicationInvocationWrite {
    pub invocation: ApplicationInvocation,
    pub expected_version: u64,
}

impl AdvanceApplicationInvocationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.invocation.validate()?;
        if self.expected_version == 0
            || self.invocation.aggregate_version != self.expected_version.saturating_add(1)
            || self.invocation.status == ApplicationInvocationStatus::Requested
        {
            return Err("Application invocation transition write is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, current: &ApplicationInvocation) -> Result<(), String> {
        self.validate()?;
        if current.aggregate_version != self.expected_version {
            return Err("Application invocation transition used a stale version".into());
        }
        let expected = match self.invocation.status {
            ApplicationInvocationStatus::Requested => {
                return Err("Application invocation cannot transition back to requested".into())
            }
            ApplicationInvocationStatus::Running => current.bind_workflow_run(
                self.expected_version,
                self.invocation
                    .workflow_run_id
                    .ok_or_else(|| "running invocation requires a WorkflowRun".to_owned())?,
                self.invocation.updated_at,
            )?,
            ApplicationInvocationStatus::Cancelling => {
                current.request_cancellation(self.expected_version, self.invocation.updated_at)?
            }
            status => current.observe_terminal(
                self.expected_version,
                status,
                self.invocation.updated_at,
            )?,
        };
        if expected != self.invocation {
            return Err("Application invocation transition changed immutable state".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendApplicationMessageWrite {
    pub message: ApplicationMessage,
    pub expected_session_version: u64,
}

impl AppendApplicationMessageWrite {
    pub fn validate_against(
        &self,
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
    ) -> Result<(), String> {
        if self.expected_session_version == 0
            || self.expected_session_version != session.aggregate_version
            || self.message.kind == ApplicationMessageKind::Input
            || self.message.workflow_effect.is_none()
        {
            return Err("Application Workflow message write is invalid".into());
        }
        self.message.validate_against(session, invocation)?;
        session.append_message(self.expected_session_version, &self.message)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvanceConversationVariablesWrite {
    pub revision: ConversationVariableRevision,
    pub expected_session_version: u64,
}

impl AdvanceConversationVariablesWrite {
    pub fn validate_against(
        &self,
        session: &ApplicationSession,
        parent: &ConversationVariableRevision,
    ) -> Result<(), String> {
        if self.expected_session_version == 0
            || self.expected_session_version != session.aggregate_version
            || self.revision.source_effect.is_none()
        {
            return Err("Conversation variable successor write is invalid".into());
        }
        self.revision.validate_successor_of(parent)?;
        session.advance_variables(self.expected_session_version, &self.revision)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseApplicationSessionWrite {
    pub session: ApplicationSession,
    pub expected_version: u64,
}

impl CloseApplicationSessionWrite {
    pub fn validate_against(&self, current: &ApplicationSession) -> Result<(), String> {
        self.session.validate()?;
        if self.expected_version == 0
            || current.aggregate_version != self.expected_version
            || self.session.status != ApplicationSessionStatus::Closed
            || self.session.aggregate_version != self.expected_version.saturating_add(1)
        {
            return Err("Application session close write is invalid".into());
        }
        let expected = current.close(
            self.expected_version,
            self.session
                .closed_at
                .ok_or_else(|| "closed Application session requires a close time".to_owned())?,
        )?;
        if expected != self.session {
            return Err("Application session close changed immutable state".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IApplicationSessionRepository: Send + Sync {
    async fn open_session(
        &self,
        write: OpenApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError>;

    async fn request_invocation(
        &self,
        write: RequestApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError>;

    async fn advance_invocation(
        &self,
        write: AdvanceApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError>;

    async fn append_message(
        &self,
        write: AppendApplicationMessageWrite,
    ) -> Result<IdempotentWrite<ApplicationMessage>, RepositoryError>;

    async fn advance_variables(
        &self,
        write: AdvanceConversationVariablesWrite,
    ) -> Result<IdempotentWrite<ConversationVariableRevision>, RepositoryError>;

    async fn close_session(
        &self,
        write: CloseApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError>;

    async fn find_end_user(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        end_user_id: ApplicationEndUserId,
    ) -> Result<Option<ApplicationEndUser>, RepositoryError>;

    async fn find_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Option<ApplicationSession>, RepositoryError>;

    async fn find_invocation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocation>, RepositoryError>;

    async fn find_invocation_workflow_authority(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocationWorkflowAuthority>, RepositoryError>;

    async fn list_messages(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<ApplicationMessage>, RepositoryError>;

    async fn find_variable_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        revision_id: ConversationVariableRevisionId,
    ) -> Result<Option<ConversationVariableRevision>, RepositoryError>;
}
