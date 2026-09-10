use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{InferenceCredentialId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetInferenceKey {
    pub organization_id: OrganizationId,
    pub credential_id: InferenceCredentialId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetInferenceKey {
    type Output = ApplicationResult<InferenceCredential>;
}

pub struct GetInferenceKeyHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl GetInferenceKeyHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
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
                        .resource_access
                        .environment_is_visible(credential.project_id, credential.environment_id)
                    {
                        return Ok(Err(ApplicationError::NotFound(
                            "inference key not found".into(),
                        )));
                    }
                    match environments
                        .find(
                            query.organization_id,
                            credential.project_id,
                            credential.environment_id,
                        )
                        .await
                    {
                        Ok(Some(_)) => Ok(Ok(credential)),
                        Ok(None) => Ok(Err(ApplicationError::NotFound(
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
    use crate::modules::identity::domain::entities::InferenceCredential;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::projects::domain::entities::Environment;
    use crate::modules::projects::domain::value_objects::EnvironmentName;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotencyRequest, IdempotentWrite, ProjectId, RepositoryError,
    };
    use a3s_boot::ModuleRef;
    use a3s_cloud_contracts::DomainEventEnvelope;
    use async_trait::async_trait;
    use chrono::{Duration, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    struct AlwaysPresentEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for AlwaysPresentEnvironmentRepository {
        async fn create(
            &self,
            environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Ok(IdempotentWrite {
                value: environment,
                replayed: false,
            })
        }

        async fn find(
            &self,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(Some(Environment::create(
                organization_id,
                project_id,
                environment_id,
                EnvironmentName::parse("default").expect("environment name"),
                Utc::now(),
            )))
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IEnvironmentRepository for MissingEnvironmentRepository {
        async fn create(
            &self,
            environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Ok(IdempotentWrite {
                value: environment,
                replayed: false,
            })
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            Ok(None)
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    fn org_wide() -> ResourceAccessEvaluator {
        ResourceAccessEvaluator::organization_wide()
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
        let handler = GetInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            repo,
        );

        let fetched = handler
            .execute(
                GetInferenceKey {
                    organization_id,
                    credential_id: seeded.id,
                    resource_access: org_wide(),
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
        let handler = GetInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            repo,
        );

        let denied = handler
            .execute(
                GetInferenceKey {
                    organization_id,
                    credential_id: seeded.id,
                    resource_access: ResourceAccessEvaluator::restricted(vec![
                        ResourceGrantScope::Environment {
                            project_id,
                            environment_id: EnvironmentId::new(),
                        },
                    ]),
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
        let handler = GetInferenceKeyHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            repo,
        );
        let denied = handler
            .execute(
                GetInferenceKey {
                    organization_id: OrganizationId::new(),
                    credential_id: InferenceCredentialId::new(),
                    resource_access: org_wide(),
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
        let handler =
            GetInferenceKeyHandler::new(Arc::new(MissingEnvironmentRepository), repo);

        let denied = handler
            .execute(
                GetInferenceKey {
                    organization_id,
                    credential_id: seeded.id,
                    resource_access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }
}
