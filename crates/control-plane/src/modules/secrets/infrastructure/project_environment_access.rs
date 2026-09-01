use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::application::{ISecretEnvironmentAccess, SecretEnvironmentScope};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Projects environment authority.
#[derive(Clone)]
pub struct ProjectsSecretEnvironmentAccessAdapter {
    environments: Arc<dyn IEnvironmentRepository>,
}

impl ProjectsSecretEnvironmentAccessAdapter {
    pub fn new(environments: Arc<dyn IEnvironmentRepository>) -> Self {
        Self { environments }
    }
}

#[async_trait]
impl ISecretEnvironmentAccess for ProjectsSecretEnvironmentAccessAdapter {
    async fn environment_exists(
        &self,
        scope: SecretEnvironmentScope,
    ) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        match self
            .environments
            .find(
                scope.organization_id(),
                scope.project_id(),
                scope.environment_id(),
            )
            .await?
        {
            Some(environment)
                if environment.organization_id == scope.organization_id()
                    && environment.project_id == scope.project_id()
                    && environment.id == scope.environment_id()
                    && environment.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Some(_) => Err(RepositoryError::Storage(
                "Projects returned inconsistent Secrets environment evidence".into(),
            )),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::projects::domain::entities::Environment;
    use crate::modules::projects::domain::value_objects::EnvironmentName;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    };
    use a3s_cloud_contracts::DomainEventEnvelope;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubEnvironmentRepository {
        environment: Option<Environment>,
        find_calls: AtomicUsize,
    }

    #[async_trait]
    impl IEnvironmentRepository for StubEnvironmentRepository {
        async fn create(
            &self,
            _environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            unreachable!("environment access adapter never creates environments")
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.environment.clone())
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            unreachable!("environment access adapter never lists environments")
        }
    }

    #[tokio::test]
    async fn adapter_projects_only_exact_existing_environment_evidence() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let scope = SecretEnvironmentScope::new(organization_id, project_id, environment_id)
            .expect("scope");
        let environment = Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("production").expect("environment name"),
            Utc::now(),
        );
        let repository = Arc::new(StubEnvironmentRepository {
            environment: Some(environment),
            find_calls: AtomicUsize::new(0),
        });
        let adapter = ProjectsSecretEnvironmentAccessAdapter::new(repository.clone());

        assert!(adapter
            .environment_exists(scope)
            .await
            .expect("environment evidence"));
        assert_eq!(repository.find_calls.load(Ordering::SeqCst), 1);

        let inconsistent =
            ProjectsSecretEnvironmentAccessAdapter::new(Arc::new(StubEnvironmentRepository {
                environment: Some(Environment::create(
                    organization_id,
                    ProjectId::new(),
                    environment_id,
                    EnvironmentName::parse("foreign").expect("environment name"),
                    Utc::now(),
                )),
                find_calls: AtomicUsize::new(0),
            }));
        assert!(matches!(
            inconsistent.environment_exists(scope).await,
            Err(RepositoryError::Storage(_))
        ));
    }
}
