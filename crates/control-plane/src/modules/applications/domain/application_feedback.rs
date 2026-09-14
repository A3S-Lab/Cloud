//! Immutable Applications-owned session feedback.
//!
//! `APP0.2-C21` freezes the feedback aggregate named by the platform model.
//! Persistence, CQRS, Annotation Reply matching, and public delivery stay later.

use super::{ApplicationMessage, ApplicationSession, ApplicationSessionStatus, digest_json};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationFeedbackId, ApplicationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId, Sha256Digest,
    canonical_timestamp,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub const APPLICATION_FEEDBACK_COMMENT_MAX_CHARS: usize = 4 * 1024;
const FEEDBACK_IDENTITY: &str = "application-feedback:v1";

/// Discrete rating admitted on one Applications feedback record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationFeedbackRating {
    Positive,
    Negative,
}

impl ApplicationFeedbackRating {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "positive" => Ok(Self::Positive),
            "negative" => Ok(Self::Negative),
            _ => Err(format!("unsupported Application feedback rating {value:?}")),
        }
    }
}

/// One immutable feedback record bound to an exact session release.
///
/// Feedback never rewrites ordered `ApplicationMessage` sequences. Identity is
/// deterministic from the session, optional source message, and content digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationFeedback {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub end_user_id: ApplicationEndUserId,
    pub source_message_id: Option<ApplicationMessageId>,
    pub id: ApplicationFeedbackId,
    pub rating: ApplicationFeedbackRating,
    pub comment: Option<String>,
    pub content_digest: Sha256Digest,
    pub created_at: DateTime<Utc>,
}

impl ApplicationFeedback {
    pub fn create(
        session: &ApplicationSession,
        source_message: Option<&ApplicationMessage>,
        rating: ApplicationFeedbackRating,
        comment: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        if session.status != ApplicationSessionStatus::Active {
            return Err("inactive Application session cannot accept feedback".into());
        }
        let comment = normalize_comment(comment)?;
        let content_digest = content_digest(rating, comment.as_deref())?;
        let source_message_id = match source_message {
            Some(message) => {
                validate_source_message(session, message)?;
                message.validate()?;
                Some(message.id)
            }
            None => None,
        };
        let created_at = canonical_timestamp(created_at);
        if created_at < session.created_at {
            return Err("Application feedback cannot predate its session".into());
        }
        let value = Self {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            end_user_id: session.end_user_id,
            source_message_id,
            id: Self::deterministic_id(session.id, source_message_id, &content_digest)?,
            rating,
            comment,
            content_digest,
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
        source_message_id: Option<ApplicationMessageId>,
        content_digest: &Sha256Digest,
    ) -> Result<ApplicationFeedbackId, String> {
        if session_id.as_uuid().is_nil() {
            return Err("Application feedback session identity cannot be nil".into());
        }
        let material = format!(
            "{FEEDBACK_IDENTITY}\0{}\0{}",
            source_message_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            content_digest.as_str()
        );
        Ok(ApplicationFeedbackId::from_uuid(Uuid::new_v5(
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
            || self.id.as_uuid().is_nil()
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || content_digest(self.rating, self.comment.as_deref())? != self.content_digest
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Application feedback is invalid".into());
        }
        if let Some(comment) = &self.comment {
            normalize_comment(Some(comment.clone()))?;
        }
        let expected = Self::deterministic_id(
            self.session_id,
            self.source_message_id,
            &self.content_digest,
        )?;
        if self.id != expected {
            return Err("Application feedback identity drifted".into());
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
            return Err("Application feedback is outside the exact session release".into());
        }
        Ok(())
    }
}

fn normalize_comment(comment: Option<String>) -> Result<Option<String>, String> {
    let Some(comment) = comment else {
        return Ok(None);
    };
    let trimmed = comment.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > APPLICATION_FEEDBACK_COMMENT_MAX_CHARS {
        return Err("Application feedback comment exceeds the maximum length".into());
    }
    Ok(Some(trimmed.to_owned()))
}

fn content_digest(
    rating: ApplicationFeedbackRating,
    comment: Option<&str>,
) -> Result<Sha256Digest, String> {
    digest_json(&json!({
        "rating": rating.as_str(),
        "comment": comment,
    }))
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
        return Err("Application feedback source message is outside the exact session".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
        ApplicationInteractionMode, ApplicationMessageKind, ApplicationRelease,
        ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
        ApplicationWorkflowBinding, ConversationVariableRevision,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationInvocationId, PrincipalId, WorkflowDefinitionId, WorkflowRevisionId,
    };
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
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
            Utc.with_ymd_and_hms(2026, 9, 14, 14, 0, 0)
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

    #[test]
    fn create_is_idempotent_for_exact_content() {
        let session = active_session();
        let first = ApplicationFeedback::create(
            &session,
            None,
            ApplicationFeedbackRating::Positive,
            Some(" helpful ".into()),
            session.created_at + Duration::seconds(1),
        )
        .expect("feedback");
        let again = ApplicationFeedback::create(
            &session,
            None,
            ApplicationFeedbackRating::Positive,
            Some("helpful".into()),
            session.created_at + Duration::seconds(2),
        )
        .expect("replay");
        assert_eq!(first.id, again.id);
        assert_eq!(first.content_digest, again.content_digest);
        assert_eq!(first.comment.as_deref(), Some("helpful"));
        first.validate_against(&session).expect("validate");
    }

    #[test]
    fn closed_session_and_foreign_message_fail_closed() {
        let session = active_session();
        let closed = session
            .close(
                session.aggregate_version,
                session.created_at + Duration::seconds(1),
            )
            .expect("close");
        assert!(
            ApplicationFeedback::create(
                &closed,
                None,
                ApplicationFeedbackRating::Negative,
                None,
                closed.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let session = active_session();
        let other = active_session();
        let foreign = ApplicationMessage {
            organization_id: other.organization_id,
            project_id: other.project_id,
            application_id: other.application_id,
            application_release_id: other.application_release_id,
            application_release_digest: other.application_release_digest.clone(),
            session_id: other.id,
            invocation_id: ApplicationInvocationId::new(),
            id: ApplicationMessageId::new(),
            sequence: 1,
            kind: ApplicationMessageKind::Input,
            content: json!({}),
            content_digest: digest_json(&json!({})).expect("digest"),
            workflow_effect: None,
            created_at: other.created_at,
        };
        assert!(
            ApplicationFeedback::create(
                &session,
                Some(&foreign),
                ApplicationFeedbackRating::Positive,
                None,
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );
    }

    #[test]
    fn oversized_comment_fails_closed() {
        let session = active_session();
        let comment = "x".repeat(APPLICATION_FEEDBACK_COMMENT_MAX_CHARS + 1);
        assert!(
            ApplicationFeedback::create(
                &session,
                None,
                ApplicationFeedbackRating::Positive,
                Some(comment),
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );
    }
}
