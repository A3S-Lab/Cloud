//! Adapts Identity inference credential storage into Edge ACL projections.

use crate::modules::identity::application::{
    IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
};
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceCredentialAclProjection;
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone)]
pub struct InferenceCredentialAclProjectionAdapter {
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl InferenceCredentialAclProjectionAdapter {
    pub fn new(credentials: Arc<dyn IInferenceCredentialRepository>) -> Self {
        Self { credentials }
    }
}

#[async_trait]
impl IInferenceCredentialAclProjectionPort for InferenceCredentialAclProjectionAdapter {
    async fn list_inference_credential_acl_projections(
        &self,
        scopes: &[InferenceCredentialEnvironmentScope],
    ) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError> {
        let unique = scopes.iter().copied().collect::<BTreeSet<_>>();
        let mut projections = Vec::new();
        for scope in unique {
            let credentials = self
                .credentials
                .list_inference_credentials_by_environment(
                    scope.organization_id(),
                    scope.project_id(),
                    scope.environment_id(),
                )
                .await?;
            for credential in credentials {
                projections.push(
                    credential
                        .gateway_projection()
                        .map_err(RepositoryError::Storage)?,
                );
            }
        }
        projections.sort_by_key(|projection| projection.credential_id);
        Ok(projections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::InferenceCredential;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId,
    };
    use chrono::{Duration, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[tokio::test]
    async fn projects_unique_environment_credentials_sorted_by_id() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let now = Utc::now();
        let first = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_aaaaaaaaaaaaaaaa",
            VERIFIER,
            now + Duration::hours(2),
            now,
        )
        .unwrap();
        let second = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_bbbbbbbbbbbbbbbb",
            VERIFIER,
            now + Duration::hours(2),
            now,
        )
        .unwrap();
        repo.create_inference_credential(second.clone())
            .await
            .unwrap();
        repo.create_inference_credential(first.clone())
            .await
            .unwrap();
        let adapter = InferenceCredentialAclProjectionAdapter::new(repo);
        let scope =
            InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
                .unwrap();
        let projections = adapter
            .list_inference_credential_acl_projections(&[scope, scope])
            .await
            .unwrap();
        assert_eq!(projections.len(), 2);
        assert!(projections[0].credential_id < projections[1].credential_id);
        assert_eq!(projections[0].audience, "cloud-inference");
    }

    #[tokio::test]
    async fn projects_revoked_credentials_with_revoked_flag() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let now = Utc::now();
        let mut credential = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_cccccccccccccccc",
            VERIFIER,
            now + Duration::hours(2),
            now,
        )
        .unwrap();
        repo.create_inference_credential(credential.clone())
            .await
            .unwrap();
        let revoked_at = credential.updated_at() + Duration::seconds(1);
        assert!(credential.revoke(revoked_at).unwrap());
        repo.update_inference_credential(credential.clone(), 1)
            .await
            .unwrap();

        let adapter = InferenceCredentialAclProjectionAdapter::new(repo);
        let scope =
            InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
                .unwrap();
        let projections = adapter
            .list_inference_credential_acl_projections(&[scope])
            .await
            .unwrap();
        assert_eq!(projections.len(), 1);
        assert!(
            projections[0].revoked,
            "revoked credentials must still project so Gateway can fail closed"
        );
    }

    #[tokio::test]
    async fn projects_expired_unrevoked_credentials_with_expires_at_for_gateway_fail_closed() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let now = Utc::now();
        // Domain allows expires_at in the past relative to wall clock so long as
        // expires_at > updated_at and the credential is not revoked.
        let credential = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_dddddddddddddddd",
            VERIFIER,
            now - Duration::hours(1),
            now - Duration::hours(2),
        )
        .unwrap();
        assert!(!credential.is_active_at(now));
        assert!(credential.revoked_at().is_none());
        repo.create_inference_credential(credential.clone())
            .await
            .unwrap();

        let adapter = InferenceCredentialAclProjectionAdapter::new(repo);
        let scope =
            InferenceCredentialEnvironmentScope::new(organization_id, project_id, environment_id)
                .unwrap();
        let projections = adapter
            .list_inference_credential_acl_projections(&[scope])
            .await
            .unwrap();
        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].credential_id, credential.id.as_uuid());
        assert_eq!(projections[0].expires_at, credential.expires_at());
        assert!(
            !projections[0].revoked,
            "expiry alone must not set revoked; Gateway fail-closes on expires_at"
        );
        assert!(
            projections[0].expires_at < now,
            "projection must retain the past expires_at for Gateway fail-closed"
        );
    }
}
