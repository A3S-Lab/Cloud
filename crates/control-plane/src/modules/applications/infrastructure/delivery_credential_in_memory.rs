use crate::modules::applications::domain::{
    ApplicationDeliveryCredential, IApplicationDeliveryCredentialRepository,
};
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, OrganizationId, ProjectId, RepositoryError,
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
            ApplicationDeliveryCredentialId,
        ),
        ApplicationDeliveryCredential,
    >,
    by_lookup: HashMap<
        (OrganizationId, ProjectId, ApplicationId, String),
        ApplicationDeliveryCredentialId,
    >,
}

/// In-memory Applications owner for anonymous delivery credential bindings.
#[derive(Clone, Default)]
pub struct InMemoryApplicationDeliveryCredentialRepository {
    state: Arc<RwLock<State>>,
}

#[async_trait]
impl IApplicationDeliveryCredentialRepository for InMemoryApplicationDeliveryCredentialRepository {
    async fn create_delivery_credential(
        &self,
        credential: ApplicationDeliveryCredential,
    ) -> Result<ApplicationDeliveryCredential, RepositoryError> {
        credential
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        if credential.generation != 1
            || credential.created_at != credential.updated_at
            || credential.revoked_at.is_some()
        {
            return Err(RepositoryError::Conflict(
                "new Application delivery credential is not at its initial generation".into(),
            ));
        }
        let mut state = self.state.write().await;
        let id_key = (
            credential.organization_id,
            credential.application_id,
            credential.id,
        );
        let lookup_key = (
            credential.organization_id,
            credential.project_id,
            credential.application_id,
            credential.lookup_key.clone(),
        );
        if state.by_id.contains_key(&id_key) || state.by_lookup.contains_key(&lookup_key) {
            return Err(RepositoryError::Conflict(
                "Application delivery credential identity or lookup key is already in use".into(),
            ));
        }
        state.by_lookup.insert(lookup_key, credential.id);
        state.by_id.insert(id_key, credential.clone());
        Ok(credential)
    }

    async fn update_delivery_credential(
        &self,
        credential: ApplicationDeliveryCredential,
        expected_generation: u64,
    ) -> Result<ApplicationDeliveryCredential, RepositoryError> {
        let mut state = self.state.write().await;
        let id_key = (
            credential.organization_id,
            credential.application_id,
            credential.id,
        );
        let existing = state
            .by_id
            .get(&id_key)
            .filter(|existing| {
                existing.project_id == credential.project_id
                    && existing.organization_id == credential.organization_id
            })
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        credential
            .validate_transition_from(&existing, expected_generation)
            .map_err(RepositoryError::Conflict)?;
        state.by_id.insert(id_key, credential.clone());
        Ok(credential)
    }

    async fn find_delivery_credential(
        &self,
        organization_id: OrganizationId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
    ) -> Result<Option<ApplicationDeliveryCredential>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .by_id
            .get(&(organization_id, application_id, credential_id))
            .cloned())
    }

    async fn find_delivery_credential_by_lookup_key(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        lookup_key: &str,
    ) -> Result<Option<ApplicationDeliveryCredential>, RepositoryError> {
        let state = self.state.read().await;
        let Some(credential_id) = state
            .by_lookup
            .get(&(
                organization_id,
                project_id,
                application_id,
                lookup_key.to_owned(),
            ))
            .copied()
        else {
            return Ok(None);
        };
        Ok(state
            .by_id
            .get(&(organization_id, application_id, credential_id))
            .cloned())
    }

    async fn list_delivery_credentials_by_application(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> Result<Vec<ApplicationDeliveryCredential>, RepositoryError> {
        let state = self.state.read().await;
        let mut credentials: Vec<_> = state
            .by_id
            .values()
            .filter(|credential| {
                credential.organization_id == organization_id
                    && credential.project_id == project_id
                    && credential.application_id == application_id
            })
            .cloned()
            .collect();
        credentials.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        Ok(credentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience, ApplicationRelease,
        ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationResponseMode,
        ApplicationWorkflowBinding,
    };
    use crate::modules::shared_kernel::domain::{
        ApplicationReleaseId, PrincipalId, SecretId, SecretVersionReference, Sha256Digest,
        WorkflowDefinitionId, WorkflowRevisionId,
    };
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }

    fn release() -> ApplicationRelease {
        let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatbot,
            audience: ApplicationAudience::Anonymous,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationExperience::Chatbot.interaction_mode(),
                response_modes: vec![ApplicationResponseMode::Blocking],
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::from_uuid(
                    Uuid::parse_str("018f0000-0000-7000-8000-000000000101").expect("UUID"),
                ),
                workflow_revision_id: WorkflowRevisionId::from_uuid(
                    Uuid::parse_str("018f0000-0000-7000-8000-000000000102").expect("UUID"),
                ),
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
            OrganizationId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000001").expect("UUID"),
            ),
            ProjectId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000002").expect("UUID"),
            ),
            ApplicationId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000003").expect("UUID"),
            ),
            ApplicationReleaseId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000004").expect("UUID"),
            ),
            contract,
            PrincipalId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000005").expect("UUID"),
            ),
            Utc.with_ymd_and_hms(2026, 9, 14, 8, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    fn issued(lookup_key: &str) -> ApplicationDeliveryCredential {
        let release = release();
        ApplicationDeliveryCredential::issue(
            ApplicationDeliveryCredentialId::from_uuid(
                Uuid::parse_str("018f0000-0000-7000-8000-000000000020").expect("UUID"),
            ),
            &release,
            lookup_key,
            SecretVersionReference::new(
                SecretId::from_uuid(
                    Uuid::parse_str("018f0000-0000-7000-8000-000000000010").expect("UUID"),
                ),
                1,
            )
            .expect("secret"),
            release.created_by,
            Utc.with_ymd_and_hms(2026, 9, 14, 9, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("credential")
    }

    #[tokio::test]
    async fn persists_lookup_uniqueness_and_generation_cas() {
        let repository = InMemoryApplicationDeliveryCredentialRepository::default();
        let credential = issued("public-embed-key");
        repository
            .create_delivery_credential(credential.clone())
            .await
            .expect("create");
        assert!(
            repository
                .create_delivery_credential(credential.clone())
                .await
                .is_err()
        );

        let loaded = repository
            .find_delivery_credential_by_lookup_key(
                credential.organization_id,
                credential.project_id,
                credential.application_id,
                "public-embed-key",
            )
            .await
            .expect("find")
            .expect("present");
        assert_eq!(loaded.id, credential.id);

        let mut disabled = loaded.clone();
        disabled
            .disable(
                1,
                Utc.with_ymd_and_hms(2026, 9, 14, 9, 5, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("disable");
        repository
            .update_delivery_credential(disabled.clone(), 1)
            .await
            .expect("update");
        assert!(
            repository
                .update_delivery_credential(disabled.clone(), 1)
                .await
                .is_err()
        );

        let listed = repository
            .list_delivery_credentials_by_application(
                credential.organization_id,
                credential.project_id,
                credential.application_id,
            )
            .await
            .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].generation, 2);
    }
}
