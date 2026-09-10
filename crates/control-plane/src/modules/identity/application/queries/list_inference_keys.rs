use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListInferenceKeys {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListInferenceKeys {
    type Output = ApplicationResult<Vec<InferenceCredential>>;
}

pub struct ListInferenceKeysHandler {
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl ListInferenceKeysHandler {
    pub fn new(credentials: Arc<dyn IInferenceCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl QueryHandler<ListInferenceKeys> for ListInferenceKeysHandler {
    fn execute(
        &self,
        query: ListInferenceKeys,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<InferenceCredential>>>>
    {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if !query
                .resource_access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization".into(),
                )));
            }
            Ok(credentials
                .list_inference_credentials_by_environment(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::InferenceCredential;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;
    use crate::modules::shared_kernel::domain::InferenceCredentialId;
    use a3s_boot::ModuleRef;
    use chrono::{Duration, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

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
            "a3s_inf_aaaaaaaaaaaaaaaa",
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
    async fn lists_keys_when_environment_is_visible() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let seeded = seed(repo.as_ref(), organization_id, project_id, environment_id).await;
        let handler = ListInferenceKeysHandler::new(repo);

        let listed = handler
            .execute(
                ListInferenceKeys {
                    organization_id,
                    project_id,
                    environment_id,
                    resource_access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, seeded.id);
    }

    #[tokio::test]
    async fn ungranted_environment_fails_closed_as_not_found() {
        let repo = Arc::new(InMemoryInferenceCredentialRepository::default());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        seed(repo.as_ref(), organization_id, project_id, environment_id).await;
        let handler = ListInferenceKeysHandler::new(repo);

        let denied = handler
            .execute(
                ListInferenceKeys {
                    organization_id,
                    project_id,
                    environment_id,
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
}
