use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceSubscriptionId,
};
use crate::modules::sources::domain::GithubRepositorySubscription;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DeactivateGithubRepositorySubscription {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub subscription_id: SourceSubscriptionId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub deactivated_at: DateTime<Utc>,
}

impl Command for DeactivateGithubRepositorySubscription {
    type Output = ApplicationResult<DeactivateGithubRepositorySubscriptionResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct DeactivateGithubRepositorySubscriptionResult {
    pub subscription: GithubRepositorySubscription,
    pub replayed: bool,
}
