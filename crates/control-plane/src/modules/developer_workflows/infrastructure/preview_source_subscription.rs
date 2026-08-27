use crate::modules::developer_workflows::application::{
    IPreviewSourceSubscriptionQueryPort, PreviewSourceSubscriptionBinding,
};
use crate::modules::developer_workflows::domain::{GitBranch, GithubInstallationRef};
use crate::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, SourceSubscriptionId,
};
use crate::modules::sources::domain::{
    GithubRepositorySubscription, ISourceSubscriptionRepository,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from the Sources-owned subscription aggregate to
/// Developer Workflows' minimal Preview policy binding.
#[derive(Clone)]
pub struct RepositoryPreviewSourceSubscriptionQueryPort {
    sources: Arc<dyn ISourceSubscriptionRepository>,
}

impl RepositoryPreviewSourceSubscriptionQueryPort {
    pub fn new(sources: Arc<dyn ISourceSubscriptionRepository>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl IPreviewSourceSubscriptionQueryPort for RepositoryPreviewSourceSubscriptionQueryPort {
    async fn resolve(
        &self,
        organization_id: OrganizationId,
        source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<PreviewSourceSubscriptionBinding>, RepositoryError> {
        let subscription = match self
            .sources
            .find(organization_id, source_subscription_id)
            .await
        {
            Ok(Some(subscription)) => subscription,
            Ok(None) | Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let subscription =
            GithubRepositorySubscription::restore(subscription).map_err(|error| {
                RepositoryError::Storage(format!(
                    "Sources returned an invalid Preview source subscription: {error}"
                ))
            })?;
        if subscription.organization_id != organization_id
            || subscription.id != source_subscription_id
        {
            return Err(RepositoryError::Storage(
                "Sources returned a Preview source subscription outside the requested scope".into(),
            ));
        }
        let binding = PreviewSourceSubscriptionBinding {
            organization_id: subscription.organization_id,
            project_id: subscription.project_id,
            environment_id: subscription.environment_id,
            source_subscription_id: subscription.id,
            installation_id: GithubInstallationRef::parse(subscription.installation_id.as_u64())
                .map_err(RepositoryError::Storage)?,
            repository: subscription.repository.clone(),
            branch: GitBranch::parse(subscription.branch_name())
                .map_err(RepositoryError::Storage)?,
            active: subscription.is_active(),
        };
        binding.validate().map_err(RepositoryError::Storage)?;
        Ok(Some(binding))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotentWrite, ProjectId, SourceConnectionId,
    };
    use crate::modules::sources::domain::{
        BuildRecipe, CreateGithubRepositorySubscription, DeactivateGithubRepositorySubscription,
        GitProvider, GitReference, GitRepository, GithubInstallationId,
        NewGithubRepositorySubscription,
    };
    use chrono::{Duration, TimeZone, Utc};
    use std::sync::Mutex;

    #[tokio::test]
    async fn resolves_the_exact_active_subscription_into_the_consumer_binding() {
        let subscription = subscription();
        let repository = Arc::new(StubSourceSubscriptionRepository::new(Some(
            subscription.clone(),
        )));
        let adapter = adapter(repository.clone());

        let binding = adapter
            .resolve(subscription.organization_id, subscription.id)
            .await
            .expect("subscription resolution")
            .expect("subscription binding");

        assert_eq!(binding.organization_id, subscription.organization_id);
        assert_eq!(binding.project_id, subscription.project_id);
        assert_eq!(binding.environment_id, subscription.environment_id);
        assert_eq!(binding.source_subscription_id, subscription.id);
        assert_eq!(
            binding.installation_id.as_u64(),
            subscription.installation_id.as_u64()
        );
        assert_eq!(binding.repository, subscription.repository);
        assert_eq!(binding.branch.as_str(), subscription.branch_name());
        assert!(binding.active);
        assert_eq!(
            repository.calls(),
            vec![(subscription.organization_id, subscription.id)]
        );
    }

    #[tokio::test]
    async fn preserves_inactive_state_without_creating_another_lifecycle() {
        let mut subscription = subscription();
        subscription
            .deactivate(subscription.created_at + Duration::seconds(1))
            .expect("deactivate subscription");
        let adapter = adapter(Arc::new(StubSourceSubscriptionRepository::new(Some(
            subscription.clone(),
        ))));

        let binding = adapter
            .resolve(subscription.organization_id, subscription.id)
            .await
            .expect("subscription resolution")
            .expect("subscription binding");

        assert!(!binding.active);
        assert_eq!(binding.source_subscription_id, subscription.id);
    }

    #[tokio::test]
    async fn preserves_a_missing_subscription_as_missing() {
        let missing_adapter = adapter(Arc::new(StubSourceSubscriptionRepository::new(None)));

        assert!(missing_adapter
            .resolve(OrganizationId::new(), SourceSubscriptionId::new())
            .await
            .expect("missing subscription lookup")
            .is_none());

        let not_found = adapter(Arc::new(StubSourceSubscriptionRepository::failing(
            RepositoryError::NotFound,
        )));
        assert!(not_found
            .resolve(OrganizationId::new(), SourceSubscriptionId::new())
            .await
            .expect("not-found subscription lookup")
            .is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_or_out_of_scope_owner_evidence() {
        let mut corrupt = subscription();
        corrupt.recipe_digest = "sha256:corrupt".into();
        let corrupt_adapter = adapter(Arc::new(StubSourceSubscriptionRepository::new(Some(
            corrupt.clone(),
        ))));
        assert!(matches!(
            corrupt_adapter
                .resolve(corrupt.organization_id, corrupt.id)
                .await,
            Err(RepositoryError::Storage(message)) if message.contains("invalid Preview source subscription")
        ));

        let out_of_scope = subscription();
        let requested_organization_id = OrganizationId::new();
        let requested_subscription_id = SourceSubscriptionId::new();
        let out_of_scope_adapter = adapter(Arc::new(StubSourceSubscriptionRepository::new(Some(
            out_of_scope,
        ))));
        assert!(matches!(
            out_of_scope_adapter
                .resolve(requested_organization_id, requested_subscription_id)
                .await,
            Err(RepositoryError::Storage(message)) if message.contains("outside the requested scope")
        ));
    }

    #[test]
    fn adapter_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RepositoryPreviewSourceSubscriptionQueryPort>();
    }

    fn adapter(
        sources: Arc<StubSourceSubscriptionRepository>,
    ) -> RepositoryPreviewSourceSubscriptionQueryPort {
        let sources: Arc<dyn ISourceSubscriptionRepository> = sources;
        RepositoryPreviewSourceSubscriptionQueryPort::new(sources)
    }

    fn subscription() -> GithubRepositorySubscription {
        GithubRepositorySubscription::subscribe(NewGithubRepositorySubscription {
            id: SourceSubscriptionId::new(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            connection_id: SourceConnectionId::new(),
            installation_id: GithubInstallationId::parse(42).expect("installation ID"),
            repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/A3S-Lab/Cloud.git",
            )
            .expect("repository"),
            branch: GitReference::parse("branch", "main").expect("branch"),
            recipe: BuildRecipe::dockerfile(
                BuildRecipe::SCHEMA,
                BuildRecipe::DOCKERFILE_KIND,
                ".",
                "Dockerfile",
                None,
                vec!["linux/amd64".into()],
            )
            .expect("recipe"),
            created_at: Utc
                .with_ymd_and_hms(2026, 8, 27, 1, 0, 0)
                .single()
                .expect("timestamp"),
        })
        .expect("subscription")
    }

    struct StubSourceSubscriptionRepository {
        result: Result<Option<GithubRepositorySubscription>, RepositoryError>,
        calls: Mutex<Vec<(OrganizationId, SourceSubscriptionId)>>,
    }

    impl StubSourceSubscriptionRepository {
        fn new(subscription: Option<GithubRepositorySubscription>) -> Self {
            Self {
                result: Ok(subscription),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn failing(error: RepositoryError) -> Self {
            Self {
                result: Err(error),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(OrganizationId, SourceSubscriptionId)> {
            self.calls.lock().expect("subscription calls").clone()
        }
    }

    #[async_trait]
    impl ISourceSubscriptionRepository for StubSourceSubscriptionRepository {
        async fn create(
            &self,
            _request: CreateGithubRepositorySubscription,
        ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError> {
            unreachable!("the query adapter never creates subscriptions")
        }

        async fn find(
            &self,
            organization_id: OrganizationId,
            subscription_id: SourceSubscriptionId,
        ) -> Result<Option<GithubRepositorySubscription>, RepositoryError> {
            self.calls
                .lock()
                .expect("subscription calls")
                .push((organization_id, subscription_id));
            self.result.clone()
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Vec<GithubRepositorySubscription>, RepositoryError> {
            unreachable!("the query adapter never lists subscriptions")
        }

        async fn deactivate(
            &self,
            _request: DeactivateGithubRepositorySubscription,
        ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError> {
            unreachable!("the query adapter never deactivates subscriptions")
        }
    }
}
