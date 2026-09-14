use crate::modules::applications::domain::{
    ApplicationMessageVariant, IApplicationMessageVariantRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageVariantId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    by_id: HashMap<
        (OrganizationId, ApplicationId, ApplicationMessageVariantId),
        ApplicationMessageVariant,
    >,
}

/// In-memory Applications owner for immutable session message variants.
#[derive(Clone, Default)]
pub struct InMemoryApplicationMessageVariantRepository {
    state: Arc<RwLock<State>>,
}

impl InMemoryApplicationMessageVariantRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IApplicationMessageVariantRepository for InMemoryApplicationMessageVariantRepository {
    async fn create_message_variant(
        &self,
        variant: ApplicationMessageVariant,
    ) -> Result<IdempotentWrite<ApplicationMessageVariant>, RepositoryError> {
        variant
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let mut state = self.state.write().await;
        let key = (variant.organization_id, variant.application_id, variant.id);
        if let Some(existing) = state.by_id.get(&key) {
            if existing.project_id != variant.project_id {
                return Err(RepositoryError::Conflict(
                    "Application message variant identity is already bound to another project"
                        .into(),
                ));
            }
            if existing != &variant {
                return Err(RepositoryError::Conflict(
                    "Application message variant replay changed values".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        state.by_id.insert(key, variant.clone());
        Ok(IdempotentWrite {
            value: variant,
            replayed: false,
        })
    }

    async fn find_message_variant(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        variant_id: ApplicationMessageVariantId,
    ) -> Result<Option<ApplicationMessageVariant>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .by_id
            .get(&(organization_id, application_id, variant_id))
            .filter(|value| value.project_id == project_id)
            .cloned())
    }

    async fn list_message_variants_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageVariant>, RepositoryError> {
        let state = self.state.read().await;
        let mut values: Vec<_> = state
            .by_id
            .values()
            .filter(|value| {
                value.organization_id == organization_id
                    && value.project_id == project_id
                    && value.application_id == application_id
                    && value.session_id == session_id
            })
            .cloned()
            .collect();
        values.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
        ApplicationInteractionMode, ApplicationMessage, ApplicationMessageKind, ApplicationRelease,
        ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
        ApplicationSession, ApplicationWorkflowBinding, ApplicationWorkflowEffect,
        ConversationVariableRevision, digest_json,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
        PrincipalId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
        canonical_timestamp,
    };
    use chrono::{Duration, TimeZone, Utc};
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

    #[tokio::test]
    async fn creates_replays_and_lists_by_session() {
        let repository = InMemoryApplicationMessageVariantRepository::default();
        let session = active_session();
        let answer = answer_message(&session);
        let variant = ApplicationMessageVariant::create(
            &session,
            &answer,
            Some(json!({"tone": "shorter"})),
            session.created_at + Duration::seconds(2),
        )
        .expect("variant");
        let created = repository
            .create_message_variant(variant.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        let replayed = repository
            .create_message_variant(variant.clone())
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, variant);

        let mut drifted = variant.clone();
        drifted.instruction = Some(json!({"tone": "longer"}));
        assert!(repository.create_message_variant(drifted).await.is_err());

        let listed = repository
            .list_message_variants_by_session(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
            )
            .await
            .expect("list");
        assert_eq!(listed, vec![variant.clone()]);
        let found = repository
            .find_message_variant(
                session.organization_id,
                session.project_id,
                session.application_id,
                variant.id,
            )
            .await
            .expect("find");
        assert_eq!(found, Some(variant));
    }
}
