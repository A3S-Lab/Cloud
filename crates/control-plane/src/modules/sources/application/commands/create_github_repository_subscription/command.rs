use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::application::commands::resolve_external_source_revision::DockerfileBuildRecipeInput;
use crate::modules::sources::domain::GithubRepositorySubscription;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateGithubRepositorySubscription {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub repository_provider: String,
    pub repository_url: String,
    pub branch: String,
    pub recipe: DockerfileBuildRecipeInput,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub created_at: DateTime<Utc>,
}

impl Command for CreateGithubRepositorySubscription {
    type Output = ApplicationResult<CreateGithubRepositorySubscriptionResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateGithubRepositorySubscriptionResult {
    pub subscription: GithubRepositorySubscription,
    pub replayed: bool,
}
