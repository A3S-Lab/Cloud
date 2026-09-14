use crate::modules::applications::domain::{ApplicationFeedback, IApplicationFeedbackRepository};
use crate::modules::shared_kernel::domain::{
    ApplicationFeedbackId, ApplicationId, ApplicationSessionId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    by_id: HashMap<(OrganizationId, ApplicationId, ApplicationFeedbackId), ApplicationFeedback>,
}

/// In-memory Applications owner for immutable session feedback.
#[derive(Clone, Default)]
pub struct InMemoryApplicationFeedbackRepository {
    state: Arc<RwLock<State>>,
}

#[async_trait]
impl IApplicationFeedbackRepository for InMemoryApplicationFeedbackRepository {
    async fn create_feedback(
        &self,
        feedback: ApplicationFeedback,
    ) -> Result<IdempotentWrite<ApplicationFeedback>, RepositoryError> {
        feedback
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let mut state = self.state.write().await;
        let key = (
            feedback.organization_id,
            feedback.application_id,
            feedback.id,
        );
        if let Some(existing) = state.by_id.get(&key) {
            if existing.project_id != feedback.project_id {
                return Err(RepositoryError::Conflict(
                    "Application feedback identity is already bound to another project".into(),
                ));
            }
            if existing != &feedback {
                return Err(RepositoryError::Conflict(
                    "Application feedback replay changed values".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        state.by_id.insert(key, feedback.clone());
        Ok(IdempotentWrite {
            value: feedback,
            replayed: false,
        })
    }

    async fn find_feedback(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        feedback_id: ApplicationFeedbackId,
    ) -> Result<Option<ApplicationFeedback>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .by_id
            .get(&(organization_id, application_id, feedback_id))
            .filter(|value| value.project_id == project_id)
            .cloned())
    }

    async fn list_feedback_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationFeedback>, RepositoryError> {
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
        ApplicationFeedbackRating, ApplicationInteractionMode, ApplicationRelease,
        ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
        ApplicationSession, ApplicationWorkflowBinding, ConversationVariableRevision,
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

    #[tokio::test]
    async fn creates_replays_and_lists_by_session() {
        let repository = InMemoryApplicationFeedbackRepository::default();
        let session = active_session();
        let feedback = ApplicationFeedback::create(
            &session,
            None,
            ApplicationFeedbackRating::Positive,
            Some("good".into()),
            session.created_at + Duration::seconds(1),
        )
        .expect("feedback");
        let created = repository
            .create_feedback(feedback.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        let replayed = repository
            .create_feedback(feedback.clone())
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, feedback);

        let mut drifted = feedback.clone();
        drifted.comment = Some("changed".into());
        assert!(repository.create_feedback(drifted).await.is_err());

        let listed = repository
            .list_feedback_by_session(
                session.organization_id,
                session.project_id,
                session.application_id,
                session.id,
            )
            .await
            .expect("list");
        assert_eq!(listed, vec![feedback]);
    }
}
