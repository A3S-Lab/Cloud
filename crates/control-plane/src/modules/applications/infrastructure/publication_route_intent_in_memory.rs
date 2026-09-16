use crate::modules::applications::domain::{
    ApplicationPublicationRouteIntent, IApplicationPublicationRouteIntentRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest,
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
            ApplicationPublicationRouteIntentId,
        ),
        ApplicationPublicationRouteIntent,
    >,
}

/// In-memory Applications owner for immutable publication route intents.
#[derive(Clone, Default)]
pub struct InMemoryApplicationPublicationRouteIntentRepository {
    state: Arc<RwLock<State>>,
}

impl InMemoryApplicationPublicationRouteIntentRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IApplicationPublicationRouteIntentRepository
    for InMemoryApplicationPublicationRouteIntentRepository
{
    async fn create_intent(
        &self,
        intent: ApplicationPublicationRouteIntent,
    ) -> Result<IdempotentWrite<ApplicationPublicationRouteIntent>, RepositoryError> {
        intent
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let mut state = self.state.write().await;
        let key = (
            intent.organization_id,
            intent.application_id,
            intent.id,
        );
        if let Some(existing) = state.by_id.get(&key) {
            if existing.project_id != intent.project_id {
                return Err(RepositoryError::Conflict(
                    "Application publication route intent identity is already bound to another project"
                        .into(),
                ));
            }
            if existing != &intent {
                return Err(RepositoryError::Conflict(
                    "Application publication route intent replay changed values".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        state.by_id.insert(key, intent.clone());
        Ok(IdempotentWrite {
            value: intent,
            replayed: false,
        })
    }

    async fn find_intent(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        intent_id: ApplicationPublicationRouteIntentId,
    ) -> Result<Option<ApplicationPublicationRouteIntent>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .by_id
            .get(&(organization_id, application_id, intent_id))
            .filter(|value| value.project_id == project_id)
            .cloned())
    }

    async fn list_intents_by_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        application_release_id: ApplicationReleaseId,
        application_release_digest: &Sha256Digest,
    ) -> Result<Vec<ApplicationPublicationRouteIntent>, RepositoryError> {
        let state = self.state.read().await;
        let mut values: Vec<_> = state
            .by_id
            .values()
            .filter(|value| {
                value.organization_id == organization_id
                    && value.project_id == project_id
                    && value.application_id == application_id
                    && value.application_release_id == application_release_id
                    && value.application_release_digest == *application_release_digest
            })
            .cloned()
            .collect();
        values.sort_by(|left, right| left.id.as_uuid().cmp(&right.id.as_uuid()));
        Ok(values)
    }

    async fn list_intents_by_project(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<ApplicationPublicationRouteIntent>, RepositoryError> {
        let state = self.state.read().await;
        let mut values: Vec<_> = state
            .by_id
            .values()
            .filter(|value| {
                value.organization_id == organization_id && value.project_id == project_id
            })
            .cloned()
            .collect();
        values.sort_by(|left, right| left.id.as_uuid().cmp(&right.id.as_uuid()));
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
        ApplicationInteractionMode, ApplicationPublicationChannel,
        ApplicationPublicationRateShapingPolicyRef, ApplicationRelease,
        ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
        ApplicationWorkflowBinding,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, WorkflowDefinitionId, WorkflowRevisionId,
    };
    use chrono::{TimeZone, Utc};

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }

    fn rate_policy() -> ApplicationPublicationRateShapingPolicyRef {
        ApplicationPublicationRateShapingPolicyRef::create("public-api-default", digest('a'))
            .expect("rate policy")
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
            Utc.with_ymd_and_hms(2026, 9, 15, 13, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    #[tokio::test]
    async fn creates_replays_finds_and_lists_by_release() {
        let repository = InMemoryApplicationPublicationRouteIntentRepository::default();
        let release = release();
        let intent = ApplicationPublicationRouteIntent::create(
            &release,
            vec![
                ApplicationPublicationChannel::ApiBlocking,
                ApplicationPublicationChannel::Web,
            ],
            Vec::new(),
            rate_policy(),
        )
        .expect("intent");

        let created = repository
            .create_intent(intent.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        let replayed = repository
            .create_intent(intent.clone())
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, intent);

        let mut drifted = intent.clone();
        drifted.rate_shaping_policy.profile_id = "mutated".into();
        assert!(repository.create_intent(drifted).await.is_err());

        let found = repository
            .find_intent(
                intent.organization_id,
                intent.project_id,
                intent.application_id,
                intent.id,
            )
            .await
            .expect("find");
        assert_eq!(found, Some(intent.clone()));

        let listed = repository
            .list_intents_by_release(
                intent.organization_id,
                intent.project_id,
                intent.application_id,
                intent.application_release_id,
                &intent.application_release_digest,
            )
            .await
            .expect("list");
        assert_eq!(listed, vec![intent]);
    }

    #[tokio::test]
    async fn lists_multiple_intents_for_same_release_sorted_by_id() {
        let repository = InMemoryApplicationPublicationRouteIntentRepository::default();
        let release = release();
        let first = ApplicationPublicationRouteIntent::create(
            &release,
            vec![ApplicationPublicationChannel::Internal],
            Vec::new(),
            rate_policy(),
        )
        .expect("first");
        let second = ApplicationPublicationRouteIntent::create(
            &release,
            vec![ApplicationPublicationChannel::Mcp],
            Vec::new(),
            rate_policy(),
        )
        .expect("second");
        repository
            .create_intent(first.clone())
            .await
            .expect("create first");
        repository
            .create_intent(second.clone())
            .await
            .expect("create second");

        let listed = repository
            .list_intents_by_release(
                release.organization_id,
                release.project_id,
                release.application_id,
                release.id,
                release.contract.digest(),
            )
            .await
            .expect("list");
        let mut expected = vec![first, second];
        expected.sort_by(|left, right| left.id.as_uuid().cmp(&right.id.as_uuid()));
        assert_eq!(listed, expected);
    }
}