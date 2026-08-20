use super::{
    ApplicationInvocation, ApplicationInvocationStatus, ApplicationSession,
    ApplicationWorkflowEffect,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, ApplicationId, ApplicationInvocationId,
    ApplicationMessageId, ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId,
    Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const APPLICATION_MESSAGE_MAX_BYTES: usize = 256 * 1024;
const INPUT_MESSAGE_IDENTITY: &[u8] = b"application-message:input:v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationMessageKind {
    Input,
    Answer,
    FinalOutput,
}

impl ApplicationMessageKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Answer => "answer",
            Self::FinalOutput => "final_output",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "input" => Ok(Self::Input),
            "answer" => Ok(Self::Answer),
            "final_output" => Ok(Self::FinalOutput),
            _ => Err(format!("unsupported Application message kind {value:?}")),
        }
    }

    const fn effect_purpose(self) -> Option<&'static str> {
        match self {
            Self::Input => None,
            Self::Answer => Some("message-answer"),
            Self::FinalOutput => Some("message-final-output"),
        }
    }
}

/// One immutable channel-visible message. It references an invocation and an
/// exact Workflow effect but never copies Flow history or provider state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessage {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub invocation_id: ApplicationInvocationId,
    pub id: ApplicationMessageId,
    pub sequence: u64,
    pub kind: ApplicationMessageKind,
    pub content: Value,
    pub content_digest: Sha256Digest,
    pub workflow_effect: Option<ApplicationWorkflowEffect>,
    pub created_at: DateTime<Utc>,
}

impl ApplicationMessage {
    pub fn input(
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_session_invocation(session, invocation)?;
        if invocation.status != ApplicationInvocationStatus::Requested
            || invocation.workflow_run_id.is_some()
        {
            return Err("Application input message must precede WorkflowRun binding".into());
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < invocation.requested_at {
            return Err("Application input message cannot predate its invocation".into());
        }
        let content = invocation.input.clone();
        let value = Self {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            invocation_id: invocation.id,
            id: ApplicationMessageId::from_uuid(Uuid::new_v5(
                &invocation.id.as_uuid(),
                INPUT_MESSAGE_IDENTITY,
            )),
            sequence: next_sequence(session)?,
            kind: ApplicationMessageKind::Input,
            content_digest: digest_json(&content)?,
            content,
            workflow_effect: None,
            created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn workflow_frame(
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
        kind: ApplicationMessageKind,
        workflow_effect: ApplicationWorkflowEffect,
        content: Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_session_invocation(session, invocation)?;
        workflow_effect.validate()?;
        let Some(purpose) = kind.effect_purpose() else {
            return Err("Workflow effect cannot append an Application input message".into());
        };
        if invocation.status == ApplicationInvocationStatus::Requested
            || invocation.workflow_run_id != Some(workflow_effect.workflow_run_id)
            || (kind == ApplicationMessageKind::FinalOutput
                && matches!(
                    invocation.status,
                    ApplicationInvocationStatus::Failed | ApplicationInvocationStatus::Cancelled
                ))
        {
            return Err(
                "Application message effect does not match the invocation WorkflowRun".into(),
            );
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < invocation.requested_at {
            return Err("Application Workflow frame cannot predate its invocation".into());
        }
        let id = ApplicationMessageId::from_uuid(
            workflow_effect.deterministic_uuid(invocation.id.as_uuid(), purpose)?,
        );
        let value = Self {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            invocation_id: invocation.id,
            id,
            sequence: next_sequence(session)?,
            kind,
            content_digest: digest_json(&content)?,
            content,
            workflow_effect: Some(workflow_effect),
            created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.sequence == 0
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || digest_json(&self.content)? != self.content_digest
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Application message is invalid".into());
        }
        match (self.kind, self.workflow_effect.as_ref()) {
            (ApplicationMessageKind::Input, None) => {
                let expected = Uuid::new_v5(&self.invocation_id.as_uuid(), INPUT_MESSAGE_IDENTITY);
                if self.id.as_uuid() != expected {
                    return Err("Application input message identity drifted".into());
                }
            }
            (kind, Some(effect)) if kind != ApplicationMessageKind::Input => {
                effect.validate()?;
                let purpose = kind
                    .effect_purpose()
                    .ok_or_else(|| "Application message effect kind is invalid".to_owned())?;
                let expected = effect.deterministic_uuid(self.invocation_id.as_uuid(), purpose)?;
                if self.id.as_uuid() != expected {
                    return Err("Application Workflow message identity drifted".into());
                }
            }
            _ => return Err("Application message origin is invalid".into()),
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
    ) -> Result<(), String> {
        self.validate()?;
        validate_session_invocation(session, invocation)?;
        let expected = match (self.kind, self.workflow_effect.clone()) {
            (ApplicationMessageKind::Input, None) => {
                Self::input(session, invocation, self.created_at)?
            }
            (kind, Some(effect)) if kind != ApplicationMessageKind::Input => Self::workflow_frame(
                session,
                invocation,
                kind,
                effect,
                self.content.clone(),
                self.created_at,
            )?,
            _ => return Err("Application message origin is invalid".into()),
        };
        if expected != *self {
            return Err("Application message changed its session or invocation binding".into());
        }
        Ok(())
    }
}

pub fn digest_json(value: &Value) -> Result<Sha256Digest, String> {
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        value,
        APPLICATION_MESSAGE_MAX_BYTES,
        "Application message content",
    )?))
}

fn next_sequence(session: &ApplicationSession) -> Result<u64, String> {
    session
        .last_message_sequence
        .checked_add(1)
        .ok_or_else(|| "Application session message sequence is exhausted".to_owned())
}

fn validate_session_invocation(
    session: &ApplicationSession,
    invocation: &ApplicationInvocation,
) -> Result<(), String> {
    session.validate()?;
    invocation.validate()?;
    if session.organization_id != invocation.organization_id
        || session.project_id != invocation.project_id
        || session.application_id != invocation.application_id
        || session.application_release_id != invocation.application_release_id
        || session.application_release_digest != invocation.application_release_digest
        || session.id != invocation.session_id
    {
        return Err("Application invocation does not belong to the exact session release".into());
    }
    Ok(())
}
