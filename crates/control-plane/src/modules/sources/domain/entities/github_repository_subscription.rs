use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, ProjectId, SourceConnectionId,
    SourceSubscriptionId,
};
use crate::modules::sources::domain::{
    BuildRecipe, GitProvider, GitReference, GitRepository, GithubInstallationId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubRepositorySubscriptionStatus {
    Active,
    Inactive,
}

impl GithubRepositorySubscriptionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            _ => Err("GitHub repository subscription status is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubRepositorySubscription {
    pub id: SourceSubscriptionId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub connection_id: SourceConnectionId,
    pub installation_id: GithubInstallationId,
    pub repository: GitRepository,
    pub branch: GitReference,
    pub recipe: BuildRecipe,
    pub recipe_digest: String,
    pub status: GithubRepositorySubscriptionStatus,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewGithubRepositorySubscription {
    pub id: SourceSubscriptionId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub connection_id: SourceConnectionId,
    pub installation_id: GithubInstallationId,
    pub repository: GitRepository,
    pub branch: GitReference,
    pub recipe: BuildRecipe,
    pub created_at: DateTime<Utc>,
}

impl GithubRepositorySubscription {
    pub fn subscribe(input: NewGithubRepositorySubscription) -> Result<Self, String> {
        let recipe_digest = input.recipe.digest()?;
        Self::restore(Self {
            id: input.id,
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id: input.environment_id,
            connection_id: input.connection_id,
            installation_id: input.installation_id,
            repository: input.repository,
            branch: input.branch,
            recipe: input.recipe,
            recipe_digest,
            status: GithubRepositorySubscriptionStatus::Active,
            aggregate_version: 1,
            created_at: input.created_at,
            deactivated_at: None,
        })
    }

    pub fn restore(mut subscription: Self) -> Result<Self, String> {
        if subscription.repository.provider() != GitProvider::Github {
            return Err("GitHub repository subscription requires a GitHub repository".into());
        }
        let repository = GitRepository::parse(
            subscription.repository.provider(),
            subscription.repository.canonical_url(),
        )?;
        if repository.identity() != subscription.repository.identity() {
            return Err("subscription repository identity does not match its canonical URL".into());
        }
        let branch = GitReference::parse(subscription.branch.kind(), subscription.branch.value())?;
        if !matches!(branch, GitReference::Branch(_)) {
            return Err("GitHub repository subscription requires an exact branch".into());
        }
        let recipe = subscription.recipe.validate()?;
        if recipe.digest()? != subscription.recipe_digest {
            return Err("subscription recipe digest does not match its recipe".into());
        }
        subscription.created_at = canonical_timestamp(subscription.created_at);
        subscription.deactivated_at = subscription.deactivated_at.map(canonical_timestamp);
        match subscription.status {
            GithubRepositorySubscriptionStatus::Active
                if subscription.aggregate_version == 1 && subscription.deactivated_at.is_none() => {
            }
            GithubRepositorySubscriptionStatus::Inactive
                if subscription.aggregate_version == 2
                    && subscription
                        .deactivated_at
                        .is_some_and(|value| value >= subscription.created_at) => {}
            _ => {
                return Err("GitHub repository subscription lifecycle state is inconsistent".into())
            }
        }
        subscription.repository = repository;
        subscription.branch = branch;
        subscription.recipe = recipe;
        Ok(subscription)
    }

    pub fn deactivate(&mut self, deactivated_at: DateTime<Utc>) -> Result<bool, String> {
        if self.status == GithubRepositorySubscriptionStatus::Inactive {
            return Ok(false);
        }
        let deactivated_at = canonical_timestamp(deactivated_at);
        if deactivated_at < self.created_at {
            return Err("subscription deactivation cannot precede creation".into());
        }
        self.status = GithubRepositorySubscriptionStatus::Inactive;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "subscription aggregate version overflowed".to_owned())?;
        self.deactivated_at = Some(deactivated_at);
        Self::restore(self.clone())?;
        Ok(true)
    }

    pub fn branch_name(&self) -> &str {
        self.branch.value()
    }

    pub const fn is_active(&self) -> bool {
        matches!(self.status, GithubRepositorySubscriptionStatus::Active)
    }
}
