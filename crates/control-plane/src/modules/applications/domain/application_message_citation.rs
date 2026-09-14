//! Immutable Applications-owned message citations.
//!
//! `APP0.2-C35` freezes the Applications-side attachment of an exact Knowledge
//! chunk lineage to an Answer or FinalOutput message without owning Knowledge
//! retrieval, indexes, or corpus lifecycle. Persistence, CQRS, Knowledge
//! admission ports, blocking/streaming wait, and public delivery stay later.

use super::{
    ApplicationMessage, ApplicationMessageKind, ApplicationSession, ApplicationSessionStatus,
    digest_json,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageCitationId,
    ApplicationMessageId, ApplicationReleaseId, ApplicationSessionId, KnowledgeBaseId,
    KnowledgeBaseRevisionId, KnowledgeChunkId, KnowledgeDocumentId, OrganizationId, ProjectId,
    Sha256Digest, canonical_timestamp,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// Maximum UTF-8 byte length of optional citation excerpt text.
pub const APPLICATION_MESSAGE_CITATION_EXCERPT_MAX_BYTES: usize = 8 * 1024;
const CITATION_IDENTITY: &str = "application-message-citation:v1";

/// One immutable citation bound to an exact session Answer/FinalOutput message.
///
/// Citations never rewrite ordered `ApplicationMessage` sequences. Identity is
/// deterministic from the session, Answer/FinalOutput message, exact Knowledge
/// base/revision/document/chunk lineage, and excerpt digest so create can replay
/// without duplicating channel history. Applications does not own Knowledge
/// bytes, indexes, or retrieval policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessageCitation {
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
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub knowledge_document_id: KnowledgeDocumentId,
    pub knowledge_chunk_id: KnowledgeChunkId,
    pub excerpt: Option<String>,
    pub excerpt_digest: Sha256Digest,
    pub id: ApplicationMessageCitationId,
    pub created_at: DateTime<Utc>,
}

impl ApplicationMessageCitation {
    pub fn create(
        session: &ApplicationSession,
        source_message: &ApplicationMessage,
        knowledge_base_id: KnowledgeBaseId,
        knowledge_base_revision_id: KnowledgeBaseRevisionId,
        knowledge_document_id: KnowledgeDocumentId,
        knowledge_chunk_id: KnowledgeChunkId,
        excerpt: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        if session.status != ApplicationSessionStatus::Active {
            return Err("inactive Application session cannot accept message citations".into());
        }
        validate_source_message(session, source_message)?;
        source_message.validate()?;
        if !matches!(
            source_message.kind,
            ApplicationMessageKind::Answer | ApplicationMessageKind::FinalOutput
        ) {
            return Err(format!(
                "Application message citation source must be Answer or FinalOutput, got {:?}",
                source_message.kind
            ));
        }
        if knowledge_base_id.as_uuid().is_nil()
            || knowledge_base_revision_id.as_uuid().is_nil()
            || knowledge_document_id.as_uuid().is_nil()
            || knowledge_chunk_id.as_uuid().is_nil()
        {
            return Err("Application message citation Knowledge lineage cannot be nil".into());
        }
        let excerpt = normalize_excerpt(excerpt)?;
        let excerpt_digest = excerpt_digest(excerpt.as_ref())?;
        let created_at = canonical_timestamp(created_at);
        if created_at < session.created_at {
            return Err("Application message citation cannot predate its session".into());
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
            knowledge_base_id,
            knowledge_base_revision_id,
            knowledge_document_id,
            knowledge_chunk_id,
            excerpt,
            excerpt_digest: excerpt_digest.clone(),
            id: Self::deterministic_id(
                session.id,
                source_message.id,
                knowledge_base_revision_id,
                knowledge_chunk_id,
                &excerpt_digest,
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
        knowledge_base_revision_id: KnowledgeBaseRevisionId,
        knowledge_chunk_id: KnowledgeChunkId,
        excerpt_digest: &Sha256Digest,
    ) -> Result<ApplicationMessageCitationId, String> {
        if session_id.as_uuid().is_nil()
            || message_id.as_uuid().is_nil()
            || knowledge_base_revision_id.as_uuid().is_nil()
            || knowledge_chunk_id.as_uuid().is_nil()
        {
            return Err("Application message citation identity inputs cannot be nil".into());
        }
        let material = format!(
            "{CITATION_IDENTITY}\0{message_id}\0{knowledge_base_revision_id}\0{knowledge_chunk_id}\0{}",
            excerpt_digest.as_str()
        );
        Ok(ApplicationMessageCitationId::from_uuid(Uuid::new_v5(
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
            || self.knowledge_base_id.as_uuid().is_nil()
            || self.knowledge_base_revision_id.as_uuid().is_nil()
            || self.knowledge_document_id.as_uuid().is_nil()
            || self.knowledge_chunk_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || Sha256Digest::parse(self.excerpt_digest.as_str())? != self.excerpt_digest
            || self.created_at != canonical_timestamp(self.created_at)
            || !matches!(
                self.message_kind,
                ApplicationMessageKind::Answer | ApplicationMessageKind::FinalOutput
            )
        {
            return Err("stored Application message citation is invalid".into());
        }
        let expected_excerpt = normalize_excerpt(self.excerpt.clone())?;
        if self.excerpt != expected_excerpt {
            return Err("Application message citation excerpt is not canonical".into());
        }
        if excerpt_digest(self.excerpt.as_ref())? != self.excerpt_digest {
            return Err("Application message citation excerpt digest drifted".into());
        }
        let expected = Self::deterministic_id(
            self.session_id,
            self.message_id,
            self.knowledge_base_revision_id,
            self.knowledge_chunk_id,
            &self.excerpt_digest,
        )?;
        if self.id != expected {
            return Err("Application message citation identity drifted".into());
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
            return Err("Application message citation is outside the exact session release".into());
        }
        Ok(())
    }
}

fn normalize_excerpt(excerpt: Option<String>) -> Result<Option<String>, String> {
    match excerpt {
        None => Ok(None),
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.len() > APPLICATION_MESSAGE_CITATION_EXCERPT_MAX_BYTES {
                return Err(format!(
                    "Application message citation excerpt exceeds {APPLICATION_MESSAGE_CITATION_EXCERPT_MAX_BYTES} bytes"
                ));
            }
            if trimmed.contains('\0') {
                return Err("Application message citation excerpt cannot contain NUL".into());
            }
            Ok(Some(trimmed.to_string()))
        }
    }
}

fn excerpt_digest(excerpt: Option<&String>) -> Result<Sha256Digest, String> {
    digest_json(&json!({
        "excerpt": excerpt,
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
        return Err("Application message citation source is outside the exact session".into());
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
        let content = json!({"query": "what does the policy say?"});
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
        let content = json!({"text": "Per section 3.2, retention is 30 days."});
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

    fn lineage() -> (
        KnowledgeBaseId,
        KnowledgeBaseRevisionId,
        KnowledgeDocumentId,
        KnowledgeChunkId,
    ) {
        (
            KnowledgeBaseId::new(),
            KnowledgeBaseRevisionId::new(),
            KnowledgeDocumentId::new(),
            KnowledgeChunkId::new(),
        )
    }

    #[test]
    fn create_is_idempotent_for_exact_answer_chunk_and_excerpt() {
        let session = active_session();
        let answer = answer_message(&session);
        answer.validate().expect("answer");
        let (base, revision, document, chunk) = lineage();
        let first = ApplicationMessageCitation::create(
            &session,
            &answer,
            base,
            revision,
            document,
            chunk,
            Some(" retention is 30 days ".into()),
            session.created_at + Duration::seconds(2),
        )
        .expect("citation");
        let again = ApplicationMessageCitation::create(
            &session,
            &answer,
            base,
            revision,
            document,
            chunk,
            Some("retention is 30 days".into()),
            session.created_at + Duration::seconds(9),
        )
        .expect("replay");
        assert_eq!(first.id, again.id);
        assert_eq!(first.invocation_id, answer.invocation_id);
        assert_eq!(first.message_kind, ApplicationMessageKind::Answer);
        assert_eq!(first.excerpt.as_deref(), Some("retention is 30 days"));
        first.validate_against(&session).expect("validate");
    }

    #[test]
    fn distinct_chunks_on_same_answer_get_distinct_identities() {
        let session = active_session();
        let answer = answer_message(&session);
        let (base, revision, document, _) = lineage();
        let first = ApplicationMessageCitation::create(
            &session,
            &answer,
            base,
            revision,
            document,
            KnowledgeChunkId::new(),
            None,
            session.created_at + Duration::seconds(1),
        )
        .expect("first");
        let second = ApplicationMessageCitation::create(
            &session,
            &answer,
            base,
            revision,
            document,
            KnowledgeChunkId::new(),
            None,
            session.created_at + Duration::seconds(1),
        )
        .expect("second");
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn closed_session_input_source_foreign_message_and_huge_excerpt_fail_closed() {
        let session = active_session();
        let closed = session
            .close(
                session.aggregate_version,
                session.created_at + Duration::seconds(1),
            )
            .expect("close");
        let answer = answer_message(&session);
        let (base, revision, document, chunk) = lineage();
        assert!(ApplicationMessageCitation::create(
            &closed,
            &answer,
            base,
            revision,
            document,
            chunk,
            None,
            closed.created_at + Duration::seconds(1),
        )
        .is_err());

        let session = active_session();
        let input = input_message(&session);
        input.validate().expect("input");
        let (base, revision, document, chunk) = lineage();
        assert!(ApplicationMessageCitation::create(
            &session,
            &input,
            base,
            revision,
            document,
            chunk,
            None,
            session.created_at + Duration::seconds(1),
        )
        .is_err());

        let session = active_session();
        let other = active_session();
        let foreign = answer_message(&other);
        let (base, revision, document, chunk) = lineage();
        assert!(ApplicationMessageCitation::create(
            &session,
            &foreign,
            base,
            revision,
            document,
            chunk,
            None,
            session.created_at + Duration::seconds(1),
        )
        .is_err());

        let session = active_session();
        let answer = answer_message(&session);
        let (base, revision, document, chunk) = lineage();
        let huge = "x".repeat(APPLICATION_MESSAGE_CITATION_EXCERPT_MAX_BYTES + 1);
        assert!(ApplicationMessageCitation::create(
            &session,
            &answer,
            base,
            revision,
            document,
            chunk,
            Some(huge),
            session.created_at + Duration::seconds(1),
        )
        .is_err());
    }
}
