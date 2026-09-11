use crate::modules::identity::application::{
    IIdentityEnvironmentAccess, IdentityAccess, IdentityEnvironmentScope,
};
use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{InferenceCredentialId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetInferenceKey {
    pub organization_id: OrganizationId,
    pub credential_id: InferenceCredentialId,
    pub access: IdentityAccess,
}

impl Query for GetInferenceKey {
    type Output = ApplicationResult<InferenceCredential>;
}

pub struct GetInferenceKeyHandler {
    environments: Arc<dyn IIdentityEnvironmentAccess>,
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl GetInferenceKeyHandler {
    pub fn new(
        environments: Arc<dyn IIdentityEnvironmentAccess>,
        credentials: Arc<dyn IInferenceCredentialRepository>,
    ) -> Self {
        Self {
            environments,
            credentials,
        }
    }
}

impl QueryHandler<GetInferenceKey> for GetInferenceKeyHandler {
    fn execute(
        &self,
        query: GetInferenceKey,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceCredential>>>
    {
        let environments = Arc::clone(&self.environments);
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            match credentials
                .find_inference_credential(query.organization_id, query.credential_id)
                .await
            {
                Ok(Some(credential)) => {
                    if !query
                        .access
                        .environment_is_visible(credential.project_id, credential.environment_id)
                    {
                        return Ok(Err(ApplicationError::NotFound(
                            "inference key not found".into(),
                        )));
                    }
                    let environment_scope = match IdentityEnvironmentScope::new(
                        query.organization_id,
                        credential.project_id,
                        credential.environment_id,
                    ) {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                    };
                    match environments.environment_exists(environment_scope).await {
                        Ok(true) => Ok(Ok(credential)),
                        Ok(false) => Ok(Err(ApplicationError::NotFound(
                            "inference key not found".into(),
                        ))),
                        Err(error) => Ok(Err(error.into())),
                    }
                }
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "inference key not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::application::IdentityAccessScope;
    use crate::modules::identity::domain::entities::InferenceCredential;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId, RepositoryError};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use chrono::{Duration, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    struct AlwaysPresentEnvironmentAccess;

    #[async_trait]
    impl IIdentityEnvironmentAccess for AlwaysPresentEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: IdentityEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct MissingEnvironmentAccess;

    #[async_trait]
    impl IIdentityEnvironmentAccess for MissingEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: IdentityEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    fn org_wide() -> IdentityAccess {
        IdentityAccess::organization_wide()
    }

    async fn seed(
        repo: &InMemoryInferenceCredentialRepository,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> InferenceCredential {
        let now = Utc::now();
        let credential = InferenceCredential::issue(
            InferenceCredentialId::new(),
            organization_id,
            project_id,
            environment_id,
            "a3s_inf_bbbbbbbbbbbbbbbb",
            VERIFIER,
            now + Duration::hours(1),
            now,
        )
        .unwrap();
        repo.create_inference_credential(credential.clone())
            .await
            .unwrap();
        credential
    }

    #[tokio::test]
    async fn returns_key_when_owning_environment_is_visible() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let seeded = seed(repo.as_ref(), organization_id, project_id, environment_id).await;
        let handler = GetInferenceKeyHandler::new(Arc::new(AlwaysPresentEnvironmentAccess), repo);

        let fetched = handler
            .execute(
                GetInferenceKey {
                    organization_id,
                    credential_id: seeded.id,
                    access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, seeded.id);
    }

    #[tokio::test]
    async fn hides_key_when_owning_environment_is_ungranted() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let seeded = seed(repo.as_ref(), organization_id, project_id, environment_id).await;
        let handler = GetInferenceKeyHandler::new(Arc::new(AlwaysPresentEnvironmentAccess), repo);

        let denied = handler
            .execute(
                GetInferenceKey {
                    organization_id,
                    credential_id: seeded.id,
                    access: IdentityAccess::restricted([IdentityAccessScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn hides_missing_key_as_not_found() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let handler = GetInferenceKeyHandler::new(Arc::new(AlwaysPresentEnvironmentAccess), repo);
        let denied = handler
            .execute(
                GetInferenceKey {
                    organization_id: OrganizationId::new(),
                    credential_id: InferenceCredentialId::new(),
                    access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn missing_owning_environment_fails_closed_as_not_found() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let seeded = seed(repo.as_ref(), organization_id, project_id, environment_id).await;
        let handler = GetInferenceKeyHandler::new(Arc::new(MissingEnvironmentAccess), repo);

        let denied = handler
            .execute(
                GetInferenceKey {
                    organization_id,
                    credential_id: seeded.id,
                    access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }
}
