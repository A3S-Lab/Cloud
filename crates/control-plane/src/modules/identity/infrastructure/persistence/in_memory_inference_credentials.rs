use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    credentials: HashMap<InferenceCredentialId, InferenceCredential>,
    prefixes: HashMap<String, InferenceCredentialId>,
}

/// Standalone in-memory authority for Identity inference credentials.
#[derive(Clone, Default)]
pub struct InMemoryInferenceCredentialRepository {
    state: Arc<RwLock<State>>,
}

#[async_trait]
impl IInferenceCredentialRepository for InMemoryInferenceCredentialRepository {
    async fn create_inference_credential(
        &self,
        credential: InferenceCredential,
    ) -> Result<InferenceCredential, RepositoryError> {
        if credential.generation() != 1
            || credential.aggregate_version() != 1
            || credential.created_at() != credential.updated_at()
            || credential.revoked_at().is_some()
        {
            return Err(RepositoryError::Conflict(
                "new inference credential is not at its initial generation".into(),
            ));
        }
        let mut state = self.state.write().await;
        if state.credentials.contains_key(&credential.id)
            || state.prefixes.contains_key(credential.prefix())
        {
            return Err(RepositoryError::Conflict(
                "inference credential identity or lookup prefix is already in use".into(),
            ));
        }
        state
            .prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state.credentials.insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn update_inference_credential(
        &self,
        credential: InferenceCredential,
        expected_aggregate_version: u64,
    ) -> Result<InferenceCredential, RepositoryError> {
        let mut state = self.state.write().await;
        let existing = state
            .credentials
            .get(&credential.id)
            .filter(|existing| existing.organization_id == credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        credential
            .validate_transition_from(&existing, expected_aggregate_version)
            .map_err(RepositoryError::Conflict)?;
        if state
            .prefixes
            .get(credential.prefix())
            .is_some_and(|id| *id != credential.id)
        {
            return Err(RepositoryError::Conflict(
                "inference credential lookup prefix is already in use".into(),
            ));
        }
        state.prefixes.remove(existing.prefix());
        state
            .prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state.credentials.insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn find_inference_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: InferenceCredentialId,
    ) -> Result<Option<InferenceCredential>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .credentials
            .get(&credential_id)
            .filter(|credential| credential.organization_id == organization_id)
            .cloned())
    }

    async fn list_inference_credentials_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceCredential>, RepositoryError> {
        let state = self.state.read().await;
        let mut credentials = state
            .credentials
            .values()
            .filter(|credential| {
                credential.organization_id == organization_id
                    && credential.project_id == project_id
                    && credential.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        credentials.sort_by_key(|credential| credential.id);
        Ok(credentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        render_inference_policy_acl, require_inference_tokenizer_revision,
    };
    use chrono::{Duration, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[tokio::test]
    async fn issue_store_project_and_revoke_round_trip() {
        let repo = InMemoryInferenceCredentialRepository::default();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let created_at = Utc::now();
        let credential = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(2),
            created_at,
        )
        .unwrap();
        let stored = repo
            .create_inference_credential(credential.clone())
            .await
            .unwrap();
        let listed = repo
            .list_inference_credentials_by_environment(
                organization_id,
                project_id,
                environment_id,
            )
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        let projection = listed[0].gateway_projection().unwrap();
        let acl = render_inference_policy_acl(stored.expires_at(), &[projection]).unwrap();
        require_inference_tokenizer_revision(&acl).unwrap();
        assert!(acl.contains("a3s_inf_0123456789abcdef"));
        assert!(acl.contains("revoked = false"));

        let mut revoked = listed[0].clone();
        let version = revoked.aggregate_version();
        revoked
            .revoke(revoked.updated_at() + Duration::seconds(1))
            .unwrap();
        let revoked = repo
            .update_inference_credential(revoked, version)
            .await
            .unwrap();
        assert!(revoked.gateway_projection().unwrap().revoked);
    }

    #[tokio::test]
    async fn rejects_duplicate_prefix() {
        let repo = InMemoryInferenceCredentialRepository::default();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let created_at = Utc::now();
        let first = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap();
        repo.create_inference_credential(first).await.unwrap();
        let second = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_0123456789abcdef",
            VERIFIER,
            created_at + Duration::hours(1),
            created_at,
        )
        .unwrap();
        let err = repo.create_inference_credential(second).await.unwrap_err();
        assert!(matches!(err, RepositoryError::Conflict(_)));
    }
}
