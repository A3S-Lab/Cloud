//! Immutable Applications-owned session annotations.
//!
//! `APP0.2-C21` freezes the annotation aggregate named by the platform model.
//! Annotation Reply matching, persistence, CQRS, and public delivery stay later.

use super::{
    APPLICATION_MESSAGE_MAX_BYTES, ApplicationMessage, ApplicationSession,
    ApplicationSessionStatus, digest_json,
};
use crate::modules::shared_kernel::domain::{
    ApplicationAnnotationId, ApplicationEndUserId, ApplicationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId, Sha256Digest,
    canonical_timestamp,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const APPLICATION_ANNOTATION_CONTENT_MAX_BYTES: usize = APPLICATION_MESSAGE_MAX_BYTES;
const ANNOTATION_IDENTITY: &str = "application-annotation:v1";

/// One immutable annotation bound to an exact session release.
///
/// Annotations never rewrite ordered `ApplicationMessage` sequences. Annotation
/// Reply matching remains an Applications policy over this immutable record and
/// is out of scope for `APP0.2-C21`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationAnnotation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub end_user_id: ApplicationEndUserId,
    pub source_message_id: Option<ApplicationMessageId>,
    pub id: ApplicationAnnotationId,
    pub content: Value,
    pub content_digest: Sha256Digest,
    pub created_at: DateTime<Utc>,
}

impl ApplicationAnnotation {
    pub fn create(
        session: &ApplicationSession,
        source_message: Option<&ApplicationMessage>,
        content: Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        if session.status != ApplicationSessionStatus::Active {
            return Err("inactive Application session cannot accept annotations".into());
        }
        let content_digest = digest_json_bounded(&content)?;
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
            return Err("Application annotation cannot predate its session".into());
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
            content,
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
    ) -> Result<ApplicationAnnotationId, String> {
        if session_id.as_uuid().is_nil() {
            return Err("Application annotation session identity cannot be nil".into());
        }
        let material = format!(
            "{ANNOTATION_IDENTITY}\0{}\0{}",
            source_message_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            content_digest.as_str()
        );
        Ok(ApplicationAnnotationId::from_uuid(Uuid::new_v5(
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
            || digest_json_bounded(&self.content)? != self.content_digest
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Application annotation is invalid".into());
        }
        let expected = Self::deterministic_id(
            self.session_id,
            self.source_message_id,
            &self.content_digest,
        )?;
        if self.id != expected {
            return Err("Application annotation identity drifted".into());
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
            return Err("Application annotation is outside the exact session release".into());
        }
        Ok(())
    }
}

fn digest_json_bounded(content: &Value) -> Result<Sha256Digest, String> {
    // Reuse message digest helper after bound check via digest_json's underlying path.
    // digest_json already bounds to APPLICATION_MESSAGE_MAX_BYTES.
    let _ = APPLICATION_ANNOTATION_CONTENT_MAX_BYTES;
    digest_json(content)
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
        return Err("Application annotation source message is outside the exact session".into());
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
        ConversationVariableRevision,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, WorkflowDefinitionId, WorkflowRevisionId,
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
            Utc.with_ymd_and_hms(2026, 9, 14, 15, 0, 0)
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
        let content = json!({"note": "highlight", "offset": 3});
        let first = ApplicationAnnotation::create(
            &session,
            None,
            content.clone(),
            session.created_at + Duration::seconds(1),
        )
        .expect("annotation");
        let again = ApplicationAnnotation::create(
            &session,
            None,
            content,
            session.created_at + Duration::seconds(2),
        )
        .expect("replay");
        assert_eq!(first.id, again.id);
        assert_eq!(first.content_digest, again.content_digest);
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
            ApplicationAnnotation::create(
                &closed,
                None,
                json!({"note": "late"}),
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
            invocation_id: crate::modules::shared_kernel::domain::ApplicationInvocationId::new(),
            id: ApplicationMessageId::new(),
            sequence: 1,
            kind: crate::modules::applications::domain::ApplicationMessageKind::Input,
            content: json!({}),
            content_digest: digest_json(&json!({})).expect("digest"),
            workflow_effect: None,
            created_at: other.created_at,
        };
        assert!(
            ApplicationAnnotation::create(
                &session,
                Some(&foreign),
                json!({"note": "foreign"}),
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );
    }
}
