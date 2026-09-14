use crate::modules::applications::domain::{
    ApplicationMessageFileReference, IApplicationMessageFileReferenceRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageFileReferenceId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    by_id: HashMap<
        (
            OrganizationId,
            ApplicationId,
            ApplicationMessageFileReferenceId,
        ),
        ApplicationMessageFileReference,
    >,
}

/// In-memory Applications owner for immutable message file references.
#[derive(Clone, Default)]
pub struct InMemoryApplicationMessageFileReferenceRepository {
    state: Arc<RwLock<State>>,
}

impl InMemoryApplicationMessageFileReferenceRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IApplicationMessageFileReferenceRepository
    for InMemoryApplicationMessageFileReferenceRepository
{
    async fn create_message_file_reference(
        &self,
        reference: ApplicationMessageFileReference,
    ) -> Result<IdempotentWrite<ApplicationMessageFileReference>, RepositoryError> {
        reference
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let mut state = self.state.write().await;
        let key = (
            reference.organization_id,
            reference.application_id,
            reference.id,
        );
        if let Some(existing) = state.by_id.get(&key) {
            if existing.project_id != reference.project_id {
                return Err(RepositoryError::Conflict(
                    "Application message file reference identity is already bound to another project"
                        .into(),
                ));
            }
            if existing != &reference {
                return Err(RepositoryError::Conflict(
                    "Application message file reference replay changed values".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        state.by_id.insert(key, reference.clone());
        Ok(IdempotentWrite {
            value: reference,
            replayed: false,
        })
    }

    async fn find_message_file_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        reference_id: ApplicationMessageFileReferenceId,
    ) -> Result<Option<ApplicationMessageFileReference>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .by_id
            .get(&(organization_id, application_id, reference_id))
            .filter(|value| value.project_id == project_id)
            .cloned())
    }

    async fn list_message_file_references_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageFileReference>, RepositoryError> {
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
        ApplicationSession, ApplicationWorkflowBinding, ConversationVariableRevision, digest_json,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
        ApplicationReleaseId, PrincipalId, Sha256Digest, UserFileId, WorkflowDefinitionId,
        WorkflowRevisionId, canonical_timestamp,
    };
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!(
            "sha256:{}",
            format!("{:02x}", marker as u8).repeat(32)
        ))
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

    #[tokio::test]
    async fn creates_replays_and_lists_by_session() {
        let repository = InMemoryApplicationMessageFileReferenceRepository::default();
        let session = active_session();
        let input = input_message(&session);
        let reference = ApplicationMessageFileReference::create(
            &session,
            &input,
            UserFileId::new(),
            digest('z'),
            session.created_at + Duration::seconds(2),
        )
        .expect("reference");
        let created = repository
            .create_message_file_reference(reference.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        let replayed = repository
            .create_message_file_reference(reference.clone())
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, reference);

        let mut drifted = reference.clone();
        drifted.content_digest = digest('y');
        assert!(
            repository
                .create_message_file_reference(drifted)
                .await
                .is_err()
        );

        let listed = repository
            .list_message_file_references_by_session(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
            )
            .await
            .expect("list");
        assert_eq!(listed, vec![reference.clone()]);
        let found = repository
            .find_message_file_reference(
                session.organization_id,
                session.project_id,
                session.application_id,
                reference.id,
            )
            .await
            .expect("find");
        assert_eq!(found, Some(reference));
    }
}
