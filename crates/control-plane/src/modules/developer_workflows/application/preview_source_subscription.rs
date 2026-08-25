use crate::modules::developer_workflows::domain::{GitBranch, GithubInstallationRef};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use async_trait::async_trait;

/// Consumer-owned view of the exact Sources subscription to which a Preview
/// policy is attached. It deliberately excludes connection, credential,
/// recipe, webhook-inbox, and source-revision internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSourceSubscriptionBinding {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub installation_id: GithubInstallationRef,
    pub repository: GitRepository,
    pub branch: GitBranch,
    pub active: bool,
}

impl PreviewSourceSubscriptionBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.repository.provider() != GitProvider::Github
        {
            return Err("Preview source subscription binding is invalid".into());
        }
        if GitRepository::parse(self.repository.provider(), self.repository.canonical_url())?
            != self.repository
            || GitBranch::parse(self.branch.as_str())? != self.branch
        {
            return Err("Preview source subscription binding is not canonical".into());
        }
        GithubInstallationRef::parse(self.installation_id.as_u64())?;
        Ok(())
    }
}

#[async_trait]
pub trait IPreviewSourceSubscriptionQueryPort: Send + Sync {
    async fn resolve(
        &self,
        organization_id: OrganizationId,
        source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<PreviewSourceSubscriptionBinding>, RepositoryError>;
}
