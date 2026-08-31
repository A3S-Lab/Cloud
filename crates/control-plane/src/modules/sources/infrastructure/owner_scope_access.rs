use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::application::commands::begin_github_connection::BeginGithubConnectionHandler;
use crate::modules::sources::application::commands::create_github_repository_subscription::CreateGithubRepositorySubscriptionHandler;
use crate::modules::sources::application::commands::deactivate_github_repository_subscription::DeactivateGithubRepositorySubscriptionHandler;
use crate::modules::sources::application::commands::resolve_external_source_revision::ResolveExternalSourceRevisionHandler;
use crate::modules::sources::application::queries::list_github_repository_subscriptions::ListGithubRepositorySubscriptionsHandler;
use crate::modules::sources::application::queries::list_source_revisions::ListSourceRevisionsHandler;
use crate::modules::sources::application::{
    ISourceEnvironmentAccess, ISourceOrganizationAccess, ISourceRepositoryCredentialProvider,
};
use crate::modules::sources::domain::{
    IGithubAppAuthorizationService, IGithubConnectionRepository, ISourceResolver,
    ISourceRevisionRepository, ISourceSubscriptionRepository, SourceRepositoryPolicy,
};
use async_trait::async_trait;
use chrono::Duration;
use std::sync::Arc;

/// Sources' sole Identity Organization adapter. It projects trusted owner
/// evidence to existence only and never exposes the Organization aggregate to
/// Application.
struct IdentitySourceOrganizationAccessAdapter {
    organizations: Arc<dyn IOrganizationRepository>,
}

#[async_trait]
impl ISourceOrganizationAccess for IdentitySourceOrganizationAccessAdapter {
    async fn require_organization(&self, organization_id: OrganizationId) -> ApplicationResult<()> {
        match self.organizations.find(organization_id).await? {
            Some(organization)
                if organization.id == organization_id && organization.aggregate_version > 0 =>
            {
                Ok(())
            }
            Some(_) => Err(ApplicationError::Internal(
                "Identity returned inconsistent Sources organization evidence".into(),
            )),
            None => Err(ApplicationError::NotFound("organization not found".into())),
        }
    }
}

/// Sources' sole Projects Environment adapter. It validates the complete owner
/// scope before discarding the aggregate at the anti-corruption boundary.
struct ProjectsSourceEnvironmentAccessAdapter {
    environments: Arc<dyn IEnvironmentRepository>,
}

#[async_trait]
impl ISourceEnvironmentAccess for ProjectsSourceEnvironmentAccessAdapter {
    async fn require_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> ApplicationResult<()> {
        match self
            .environments
            .find(organization_id, project_id, environment_id)
            .await?
        {
            Some(environment)
                if environment.organization_id == organization_id
                    && environment.project_id == project_id
                    && environment.id == environment_id
                    && environment.aggregate_version > 0 =>
            {
                Ok(())
            }
            Some(_) => Err(ApplicationError::Internal(
                "Projects returned inconsistent Sources environment evidence".into(),
            )),
            None => Err(ApplicationError::NotFound(
                "environment not found in organization and project".into(),
            )),
        }
    }
}

fn organization_access(
    organizations: Arc<dyn IOrganizationRepository>,
) -> Arc<dyn ISourceOrganizationAccess> {
    Arc::new(IdentitySourceOrganizationAccessAdapter { organizations })
}

fn environment_access(
    environments: Arc<dyn IEnvironmentRepository>,
) -> Arc<dyn ISourceEnvironmentAccess> {
    Arc::new(ProjectsSourceEnvironmentAccessAdapter { environments })
}

// Preserve the composition root's repository-oriented constructors while the
// Application handlers remain dependent only on Sources-owned ports.
impl BeginGithubConnectionHandler {
    pub fn new(
        organizations: Arc<dyn IOrganizationRepository>,
        connections: Arc<dyn IGithubConnectionRepository>,
        authorization: Arc<dyn IGithubAppAuthorizationService>,
        state_ttl: Duration,
    ) -> Result<Self, String> {
        Self::from_organization_access(
            organization_access(organizations),
            connections,
            authorization,
            state_ttl,
        )
    }
}

impl CreateGithubRepositorySubscriptionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        connections: Arc<dyn IGithubConnectionRepository>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
        policy: Arc<SourceRepositoryPolicy>,
    ) -> Self {
        Self::from_environment_access(
            environment_access(environments),
            connections,
            subscriptions,
            policy,
        )
    }
}

impl DeactivateGithubRepositorySubscriptionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    ) -> Self {
        Self::from_environment_access(environment_access(environments), subscriptions)
    }
}

impl ResolveExternalSourceRevisionHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        sources: Arc<dyn ISourceRevisionRepository>,
        credentials: Arc<dyn ISourceRepositoryCredentialProvider>,
        resolver: Arc<dyn ISourceResolver>,
        policy: Arc<SourceRepositoryPolicy>,
    ) -> Self {
        Self::from_environment_access(
            environment_access(environments),
            sources,
            credentials,
            resolver,
            policy,
        )
    }
}

impl ListSourceRevisionsHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        sources: Arc<dyn ISourceRevisionRepository>,
    ) -> Self {
        Self::from_environment_access(environment_access(environments), sources)
    }
}

impl ListGithubRepositorySubscriptionsHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        subscriptions: Arc<dyn ISourceSubscriptionRepository>,
    ) -> Self {
        Self::from_environment_access(environment_access(environments), subscriptions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::Organization;
    use crate::modules::identity::domain::repositories::{
        CreateOrganizationWrite, ReadOrganizationCatalog,
    };
    use crate::modules::identity::domain::value_objects::OrganizationName;
    use crate::modules::projects::domain::entities::Environment;
    use crate::modules::projects::domain::value_objects::EnvironmentName;
    use crate::modules::shared_kernel::domain::{
        IdempotencyRequest, IdempotentWrite, RepositoryError,
    };
    use a3s_cloud_contracts::DomainEventEnvelope;
    use chrono::Utc;

    struct StubOrganizations {
        result: Result<Option<Organization>, RepositoryError>,
    }

    #[async_trait]
    impl IOrganizationRepository for StubOrganizations {
        async fn create(
            &self,
            _write: CreateOrganizationWrite,
        ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
            Err(RepositoryError::Storage(
                "unexpected Organization write".into(),
            ))
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
        ) -> Result<Option<Organization>, RepositoryError> {
            self.result.clone()
        }

        async fn list_visible(
            &self,
            _read: ReadOrganizationCatalog,
        ) -> Result<Vec<Organization>, RepositoryError> {
            Err(RepositoryError::Storage(
                "unexpected Organization catalog read".into(),
            ))
        }
    }

    struct StubEnvironments {
        result: Result<Option<Environment>, RepositoryError>,
    }

    #[async_trait]
    impl IEnvironmentRepository for StubEnvironments {
        async fn create(
            &self,
            _environment: Environment,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
            Err(RepositoryError::Storage(
                "unexpected Environment write".into(),
            ))
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Option<Environment>, RepositoryError> {
            self.result.clone()
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Vec<Environment>, RepositoryError> {
            Err(RepositoryError::Storage(
                "unexpected Environment catalog read".into(),
            ))
        }
    }

    fn organizations(
        result: Result<Option<Organization>, RepositoryError>,
    ) -> Arc<dyn IOrganizationRepository> {
        Arc::new(StubOrganizations { result })
    }

    fn environments(
        result: Result<Option<Environment>, RepositoryError>,
    ) -> Arc<dyn IEnvironmentRepository> {
        Arc::new(StubEnvironments { result })
    }

    #[tokio::test]
    async fn organization_access_projects_only_consistent_owner_evidence() {
        let organization_id = OrganizationId::new();
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("sources-owner").expect("Organization name"),
            Utc::now(),
        );
        organization_access(organizations(Ok(Some(organization))))
            .require_organization(organization_id)
            .await
            .expect("existing Organization");

        let missing = organization_access(organizations(Ok(None)))
            .require_organization(organization_id)
            .await;
        assert_eq!(
            missing,
            Err(ApplicationError::NotFound("organization not found".into()))
        );

        let inconsistent = Organization::create(
            OrganizationId::new(),
            OrganizationName::parse("foreign-owner").expect("Organization name"),
            Utc::now(),
        );
        assert!(matches!(
            organization_access(organizations(Ok(Some(inconsistent))))
                .require_organization(organization_id)
                .await,
            Err(ApplicationError::Internal(_))
        ));
        assert_eq!(
            organization_access(organizations(Err(RepositoryError::Storage(
                "Identity unavailable".into(),
            ))))
            .require_organization(organization_id)
            .await,
            Err(ApplicationError::Internal("Identity unavailable".into()))
        );
    }

    #[tokio::test]
    async fn environment_access_projects_only_consistent_owner_evidence() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let environment = Environment::create(
            organization_id,
            project_id,
            environment_id,
            EnvironmentName::parse("production").expect("Environment name"),
            Utc::now(),
        );
        environment_access(environments(Ok(Some(environment))))
            .require_environment(organization_id, project_id, environment_id)
            .await
            .expect("existing Environment");

        let missing = environment_access(environments(Ok(None)))
            .require_environment(organization_id, project_id, environment_id)
            .await;
        assert_eq!(
            missing,
            Err(ApplicationError::NotFound(
                "environment not found in organization and project".into()
            ))
        );

        let inconsistent = Environment::create(
            organization_id,
            ProjectId::new(),
            environment_id,
            EnvironmentName::parse("foreign-project").expect("Environment name"),
            Utc::now(),
        );
        assert!(matches!(
            environment_access(environments(Ok(Some(inconsistent))))
                .require_environment(organization_id, project_id, environment_id)
                .await,
            Err(ApplicationError::Internal(_))
        ));
        assert_eq!(
            environment_access(environments(Err(RepositoryError::Storage(
                "Projects unavailable".into(),
            ))))
            .require_environment(organization_id, project_id, environment_id)
            .await,
            Err(ApplicationError::Internal("Projects unavailable".into()))
        );
    }
}
