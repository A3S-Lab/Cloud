//! Immutable Applications-owned message variants ("More Like This").
//!
//! `APP0.2-C25` freezes the `ApplicationMessageVariant` aggregate named by the
//! platform model. Persistence, CQRS, regeneration invocation, and public
//! delivery stay later.

use super::{
    ApplicationMessage, ApplicationMessageKind, ApplicationSession, ApplicationSessionStatus,
    digest_json,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationMessageVariantId, ApplicationReleaseId, ApplicationSessionId, OrganizationId,
    ProjectId, Sha256Digest, canonical_timestamp,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// Maximum UTF-8 byte length of optional instruction JSON.
pub const APPLICATION_MESSAGE_VARIANT_INSTRUCTION_MAX_BYTES: usize = 4 * 1024;
const VARIANT_IDENTITY: &str = "application-message-variant:v1";

/// One immutable regeneration request bound to an exact session release.
///
/// Variants never rewrite ordered `ApplicationMessage` sequences. Identity is
/// deterministic from the session, required source Answer/FinalOutput message,
/// linked invocation (exact input), and instruction digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessageVariant {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub end_user_id: ApplicationEndUserId,
    pub invocation_id: ApplicationInvocationId,
    pub source_message_id: ApplicationMessageId,
    pub source_message_kind: ApplicationMessageKind,
    pub id: ApplicationMessageVariantId,
    pub instruction: Option<Value>,
    pub instruction_digest: Sha256Digest,
    pub created_at: DateTime<Utc>,
}

impl ApplicationMessageVariant {
    pub fn create(
        session: &ApplicationSession,
        source_message: &ApplicationMessage,
        instruction: Option<Value>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        if session.status != ApplicationSessionStatus::Active {
            return Err("inactive Application session cannot accept message variants".into());
        }
        validate_source_message(session, source_message)?;
        source_message.validate()?;
        if !matches!(
            source_message.kind,
            ApplicationMessageKind::Answer | ApplicationMessageKind::FinalOutput
        ) {
            return Err(format!(
                "Application message variant source must be Answer or FinalOutput, got {:?}",
                source_message.kind
            ));
        }
        let instruction = normalize_instruction(instruction)?;
        let instruction_digest = instruction_digest(instruction.as_ref())?;
        let created_at = canonical_timestamp(created_at);
        if created_at < session.created_at {
            return Err("Application message variant cannot predate its session".into());
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
            source_message_id: source_message.id,
            source_message_kind: source_message.kind,
            id: Self::deterministic_id(
                session.id,
                source_message.id,
                source_message.invocation_id,
                &instruction_digest,
            )?,
            instruction,
            instruction_digest,
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
        source_message_id: ApplicationMessageId,
        invocation_id: ApplicationInvocationId,
        instruction_digest: &Sha256Digest,
    ) -> Result<ApplicationMessageVariantId, String> {
        if session_id.as_uuid().is_nil()
            || source_message_id.as_uuid().is_nil()
            || invocation_id.as_uuid().is_nil()
        {
            return Err("Application message variant identity inputs cannot be nil".into());
        }
        let material = format!(
            "{VARIANT_IDENTITY}\0{source_message_id}\0{invocation_id}\0{}",
            instruction_digest.as_str()
        );
        Ok(ApplicationMessageVariantId::from_uuid(Uuid::new_v5(
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
            || self.source_message_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || instruction_digest(self.instruction.as_ref())? != self.instruction_digest
            || self.created_at != canonical_timestamp(self.created_at)
            || !matches!(
                self.source_message_kind,
                ApplicationMessageKind::Answer | ApplicationMessageKind::FinalOutput
            )
        {
            return Err("stored Application message variant is invalid".into());
        }
        if let Some(instruction) = &self.instruction {
            normalize_instruction(Some(instruction.clone()))?;
        }
        let expected = Self::deterministic_id(
            self.session_id,
            self.source_message_id,
            self.invocation_id,
            &self.instruction_digest,
        )?;
        if self.id != expected {
            return Err("Application message variant identity drifted".into());
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
            return Err("Application message variant is outside the exact session release".into());
        }
        Ok(())
    }
}

fn normalize_instruction(instruction: Option<Value>) -> Result<Option<Value>, String> {
    let Some(instruction) = instruction else {
        return Ok(None);
    };
    if !instruction.is_object() {
        return Err("Application message variant instruction must be a JSON object".into());
    }
    let encoded = serde_json::to_vec(&instruction).map_err(|error| {
        format!("Application message variant instruction encode failed: {error}")
    })?;
    if encoded.len() > APPLICATION_MESSAGE_VARIANT_INSTRUCTION_MAX_BYTES {
        return Err("Application message variant instruction exceeds the maximum length".into());
    }
    Ok(Some(instruction))
}

fn instruction_digest(instruction: Option<&Value>) -> Result<Sha256Digest, String> {
    digest_json(&json!({
        "instruction": instruction,
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
        return Err("Application message variant source is outside the exact session".into());
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
        ApplicationWorkflowEffect, ConversationVariableRevision,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
    };
    use chrono::{Duration, TimeZone};

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
    fn create_is_idempotent_for_exact_source_and_instruction() {
        let session = active_session();
        let answer = answer_message(&session);
        answer.validate().expect("answer");
        let first = ApplicationMessageVariant::create(
            &session,
            &answer,
            Some(json!({"tone": "shorter"})),
            session.created_at + Duration::seconds(2),
        )
        .expect("variant");
        let again = ApplicationMessageVariant::create(
            &session,
            &answer,
            Some(json!({"tone": "shorter"})),
            session.created_at + Duration::seconds(9),
        )
        .expect("replay");
        assert_eq!(first.id, again.id);
        assert_eq!(first.instruction_digest, again.instruction_digest);
        assert_eq!(first.invocation_id, answer.invocation_id);
        assert_eq!(first.source_message_kind, ApplicationMessageKind::Answer);
        first.validate_against(&session).expect("validate");
    }

    #[test]
    fn closed_session_input_source_and_foreign_message_fail_closed() {
        let session = active_session();
        let closed = session
            .close(
                session.aggregate_version,
                session.created_at + Duration::seconds(1),
            )
            .expect("close");
        let answer = answer_message(&session);
        assert!(
            ApplicationMessageVariant::create(
                &closed,
                &answer,
                None,
                closed.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let session = active_session();
        let invocation_id = ApplicationInvocationId::new();
        let input_content = json!({"query": "hello"});
        let input = ApplicationMessage {
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
            content: input_content.clone(),
            content_digest: digest_json(&input_content).expect("digest"),
            workflow_effect: None,
            created_at: canonical_timestamp(session.created_at),
        };
        input.validate().expect("input");
        assert!(
            ApplicationMessageVariant::create(
                &session,
                &input,
                None,
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let session = active_session();
        let other = active_session();
        let foreign = answer_message(&other);
        assert!(
            ApplicationMessageVariant::create(
                &session,
                &foreign,
                None,
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );
    }

    #[test]
    fn oversized_instruction_fails_closed() {
        let session = active_session();
        let answer = answer_message(&session);
        let huge = "x".repeat(APPLICATION_MESSAGE_VARIANT_INSTRUCTION_MAX_BYTES + 1);
        assert!(
            ApplicationMessageVariant::create(
                &session,
                &answer,
                Some(json!({ "note": huge })),
                session.created_at + Duration::seconds(1),
            )
            .is_err()
        );
    }
}
