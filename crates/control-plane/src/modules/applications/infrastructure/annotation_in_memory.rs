use crate::modules::applications::domain::{
    ApplicationAnnotation, IApplicationAnnotationRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationAnnotationId, ApplicationId, ApplicationSessionId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    by_id: HashMap<(OrganizationId, ApplicationId, ApplicationAnnotationId), ApplicationAnnotation>,
}

/// In-memory Applications owner for immutable session annotations.
#[derive(Clone, Default)]
pub struct InMemoryApplicationAnnotationRepository {
    state: Arc<RwLock<State>>,
}

impl InMemoryApplicationAnnotationRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IApplicationAnnotationRepository for InMemoryApplicationAnnotationRepository {
    async fn create_annotation(
        &self,
        annotation: ApplicationAnnotation,
    ) -> Result<IdempotentWrite<ApplicationAnnotation>, RepositoryError> {
        annotation
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let mut state = self.state.write().await;
        let key = (
            annotation.organization_id,
            annotation.application_id,
            annotation.id,
        );
        if let Some(existing) = state.by_id.get(&key) {
            if existing.project_id != annotation.project_id {
                return Err(RepositoryError::Conflict(
                    "Application annotation identity is already bound to another project".into(),
                ));
            }
            if existing != &annotation {
                return Err(RepositoryError::Conflict(
                    "Application annotation replay changed values".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        state.by_id.insert(key, annotation.clone());
        Ok(IdempotentWrite {
            value: annotation,
            replayed: false,
        })
    }

    async fn find_annotation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        annotation_id: ApplicationAnnotationId,
    ) -> Result<Option<ApplicationAnnotation>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .by_id
            .get(&(organization_id, application_id, annotation_id))
            .filter(|value| value.project_id == project_id)
            .cloned())
    }

    async fn list_annotations_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationAnnotation>, RepositoryError> {
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
        ApplicationInteractionMode, ApplicationRelease, ApplicationReleaseContract,
        ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationSession,
        ApplicationWorkflowBinding, ConversationVariableRevision,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationEndUserId, ApplicationReleaseId, PrincipalId, Sha256Digest,
        WorkflowDefinitionId, WorkflowRevisionId,
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
            Utc.with_ymd_and_hms(2026, 9, 14, 15, 30, 0)
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

    #[tokio::test]
    async fn creates_replays_and_lists_by_session() {
        let repository = InMemoryApplicationAnnotationRepository::default();
        let session = active_session();
        let annotation = ApplicationAnnotation::create(
            &session,
            None,
            json!({"note": "pin"}),
            session.created_at + Duration::seconds(1),
        )
        .expect("annotation");
        let created = repository
            .create_annotation(annotation.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        let replayed = repository
            .create_annotation(annotation.clone())
            .await
            .expect("replay");
        assert!(replayed.replayed);

        let mut drifted = annotation.clone();
        drifted.content = json!({"note": "changed"});
        assert!(repository.create_annotation(drifted).await.is_err());

        let listed = repository
            .list_annotations_by_session(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
            )
            .await
            .expect("list");
        assert_eq!(listed, vec![annotation]);
    }
}
