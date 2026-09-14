//! Immutable Applications-owned message file references.
//!
//! `APP0.2-C31` freezes the Applications-side attachment of an admitted UserFile
//! to an exact Input message without owning Files bytes or lifecycle.
//! Persistence, CQRS, Files admission ports, citations, and public delivery
//! stay later.

use super::{
    ApplicationMessage, ApplicationMessageKind, ApplicationSession, ApplicationSessionStatus,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageFileReferenceId,
    ApplicationMessageId, ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId,
    Sha256Digest, UserFileId, canonical_timestamp,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const FILE_REFERENCE_IDENTITY: &str = "application-message-file-reference:v1";

/// One immutable file attachment bound to an exact session Input message.
///
/// References never rewrite ordered `ApplicationMessage` sequences. Identity is
/// deterministic from the session, required Input message, UserFile, and exact
/// content digest so create can replay without duplicating channel history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessageFileReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub end_user_id: ApplicationEndUserId,
    pub invocation_id: ApplicationInvocationId,
    pub message_id: ApplicationMessageId,
    pub message_kind: ApplicationMessageKind,
    pub user_file_id: UserFileId,
    pub content_digest: Sha256Digest,
    pub id: ApplicationMessageFileReferenceId,
    pub created_at: DateTime<Utc>,
}

impl ApplicationMessageFileReference {
    pub fn create(
        session: &ApplicationSession,
        source_message: &ApplicationMessage,
        user_file_id: UserFileId,
        content_digest: Sha256Digest,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        if session.status != ApplicationSessionStatus::Active {
            return Err(
                "inactive Application session cannot accept message file references".into(),
            );
        }
        validate_source_message(session, source_message)?;
        source_message.validate()?;
        if source_message.kind != ApplicationMessageKind::Input {
            return Err(format!(
                "Application message file reference source must be Input, got {:?}",
                source_message.kind
            ));
        }
        if user_file_id.as_uuid().is_nil() {
            return Err("Application message file reference UserFile identity cannot be nil".into());
        }
        if Sha256Digest::parse(content_digest.as_str())? != content_digest {
            return Err("Application message file reference content digest is not canonical".into());
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < session.created_at {
            return Err("Application message file reference cannot predate its session".into());
        }
        let value = Self {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            end_user_id: session.end_user_id,
            invocation_id: source_message.invocation_id,
            message_id: source_message.id,
            message_kind: source_message.kind,
            user_file_id,
            content_digest: content_digest.clone(),
            id: Self::deterministic_id(
                session.id,
                source_message.id,
                user_file_id,
                &content_digest,
            )?,
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

    pub fn deterministic_id(
        session_id: ApplicationSessionId,
        message_id: ApplicationMessageId,
        user_file_id: UserFileId,
        content_digest: &Sha256Digest,
    ) -> Result<ApplicationMessageFileReferenceId, String> {
        if session_id.as_uuid().is_nil()
            || message_id.as_uuid().is_nil()
            || user_file_id.as_uuid().is_nil()
        {
            return Err("Application message file reference identity inputs cannot be nil".into());
        }
        let material = format!(
            "{FILE_REFERENCE_IDENTITY}\0{message_id}\0{user_file_id}\0{}",
            content_digest.as_str()
        );
        Ok(ApplicationMessageFileReferenceId::from_uuid(Uuid::new_v5(
            &session_id.as_uuid(),
            material.as_bytes(),
        )))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.end_user_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.message_id.as_uuid().is_nil()
            || self.user_file_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || Sha256Digest::parse(self.content_digest.as_str())? != self.content_digest
            || self.created_at != canonical_timestamp(self.created_at)
            || self.message_kind != ApplicationMessageKind::Input
        {
            return Err("stored Application message file reference is invalid".into());
        }
        let expected = Self::deterministic_id(
            self.session_id,
            self.message_id,
            self.user_file_id,
            &self.content_digest,
        )?;
        if self.id != expected {
            return Err("Application message file reference identity drifted".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, session: &ApplicationSession) -> Result<(), String> {
        self.validate()?;
        session.validate()?;
        if self.organization_id != session.organization_id
            || self.project_id != session.project_id
            || self.application_id != session.application_id
            || self.application_release_id != session.application_release_id
            || self.application_release_digest != session.application_release_digest
            || self.session_id != session.id
            || self.end_user_id != session.end_user_id
        {
            return Err(
                "Application message file reference is outside the exact session release".into(),
            );
        }
        Ok(())
    }
}

fn validate_source_message(
    session: &ApplicationSession,
    message: &ApplicationMessage,
) -> Result<(), String> {
    if message.organization_id != session.organization_id
        || message.project_id != session.project_id
        || message.application_id != session.application_id
        || message.application_release_id != session.application_release_id
        || message.application_release_digest != session.application_release_digest
        || message.session_id != session.id
    {
        return Err("Application message file reference source is outside the exact session".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
        ApplicationInteractionMode, ApplicationRelease, ApplicationReleaseContract,
        ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationWorkflowBinding,
        ApplicationWorkflowEffect, ConversationVariableRevision, digest_json,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
    };
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", format!("{:02x}", marker as u8).repeat(32)))
            .expect("digest")
    }

    fn release() -> ApplicationRelease {
        let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatflow,
            audience: ApplicationAudience::ProjectMembers,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationInteractionMode::Conversation,
                response_modes: vec![ApplicationResponseMode::Blocking],
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::new(),
                workflow_revision_id: WorkflowRevisionId::new(),
                workflow_contract_digest: digest('a'),
                workflow_payload_set_digest: digest('b'),
                workflow_semantic_contract_set_digest: digest('c'),
                input_schema_digest: digest('d'),
                output_schema_digest: digest('e'),
            },
            presentation_digest: digest('f'),
        })
        .expect("contract");
        ApplicationRelease::initial(
            OrganizationId::new(),
            ProjectId::new(),
            ApplicationId::new(),
            ApplicationReleaseId::new(),
            contract,
            PrincipalId::new(),
            Utc.with_ymd_and_hms(2026, 9, 14, 16, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    fn active_session() -> ApplicationSession {
        let release = release();
        let end_user = ApplicationEndUser::create(
            ApplicationEndUserId::new(),
            &release,
            Some(PrincipalId::new()),
            release.created_by,
            release.created_at,
        )
        .expect("end user");
        let session_id = ApplicationSessionId::new();
        let variables = ConversationVariableRevision::initial(
            session_id,
            &release,
            json!({"locale": "en-US"}),
            release.created_at,
        )
        .expect("variables");
        ApplicationSession::create(
            session_id,
            &release,
            &end_user,
            &variables,
            release.created_at,
        )
        .expect("session")
    }

    fn input_message(session: &ApplicationSession) -> ApplicationMessage {
        let invocation_id = ApplicationInvocationId::new();
        let content = json!({"query": "summarize", "files": ["doc.pdf"]});
        ApplicationMessage {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            invocation_id,
            id: ApplicationMessageId::from_uuid(Uuid::new_v5(
                &invocation_id.as_uuid(),
                b"application-message:input:v1",
            )),
            sequence: 1,
            kind: ApplicationMessageKind::Input,
            content: content.clone(),
            content_digest: digest_json(&content).expect("digest"),
            workflow_effect: None,
            created_at: canonical_timestamp(session.created_at),
        }
    }

    fn answer_message(session: &ApplicationSession) -> ApplicationMessage {
        let invocation_id = ApplicationInvocationId::new();
        let effect =
            ApplicationWorkflowEffect::new(WorkflowRunId::new(), "answer", 1, 0).expect("effect");
        let content = json!({"text": "Hello"});
        ApplicationMessage {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            invocation_id,
            id: ApplicationMessage::workflow_frame_id(
                invocation_id,
                ApplicationMessageKind::Answer,
                &effect,
            )
            .expect("id"),
            sequence: 2,
            kind: ApplicationMessageKind::Answer,
            content: content.clone(),
            content_digest: digest_json(&content).expect("digest"),
            workflow_effect: Some(effect),
            created_at: canonical_timestamp(session.created_at + Duration::seconds(1)),
        }
    }

    #[test]
    fn create_is_idempotent_for_exact_input_file_and_digest() {
        let session = active_session();
        let input = input_message(&session);
        input.validate().expect("input");
        let user_file_id = UserFileId::new();
        let content_digest = digest('z');
        let first = ApplicationMessageFileReference::create(
            &session,
            &input,
            user_file_id,
            content_digest.clone(),
            session.created_at + Duration::seconds(2),
        )
        .expect("file reference");
        let again = ApplicationMessageFileReference::create(
            &session,
            &input,
            user_file_id,
            content_digest,
            session.created_at + Duration::seconds(9),
        )
        .expect("replay");
        assert_eq!(first.id, again.id);
        assert_eq!(first.invocation_id, input.invocation_id);
        assert_eq!(first.message_kind, ApplicationMessageKind::Input);
        first.validate_against(&session).expect("validate");
    }

    #[test]
    fn distinct_user_files_on_same_input_get_distinct_identities() {
        let session = active_session();
        let input = input_message(&session);
        let first = ApplicationMessageFileReference::create(
            &session,
            &input,
            UserFileId::new(),
            digest('1'),
            session.created_at + Duration::seconds(1),
        )
        .expect("first");
        let second = ApplicationMessageFileReference::create(
            &session,
            &input,
            UserFileId::new(),
            digest('2'),
            session.created_at + Duration::seconds(1),
        )
        .expect("second");
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn closed_session_answer_source_and_foreign_message_fail_closed() {
        let session = active_session();
        let closed = session
            .close(
                session.aggregate_version,
                session.created_at + Duration::seconds(1),
            )
            .expect("close");
        let input = input_message(&session);
        assert!(
            ApplicationMessageFileReference::create(
                &closed,
                &input,
                UserFileId::new(),
                digest('z'),
                closed.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let session = active_session();
        let answer = answer_message(&session);
        answer.validate().expect("answer");
        assert!(
            ApplicationMessageFileReference::create(
                &session,
                &answer,
                UserFileId::new(),
                digest('z'),
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let session = active_session();
        let other = active_session();
        let foreign = input_message(&other);
        assert!(
            ApplicationMessageFileReference::create(
                &session,
                &foreign,
                UserFileId::new(),
                digest('z'),
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );
    }
}
