use crate::modules::sources::domain::GithubRepositorySubscription;
use crate::modules::sources::presentation::dto::response::source_revision_response::{
    BuildRecipeResponse, GitRepositoryResponse,
};
use crate::modules::sources::{
    CreateGithubRepositorySubscriptionResult, DeactivateGithubRepositorySubscriptionResult,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepositorySubscriptionResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub connection_id: Uuid,
    pub installation_id: u64,
    pub repository: GitRepositoryResponse,
    pub branch: String,
    pub recipe: BuildRecipeResponse,
    pub recipe_digest: String,
    pub status: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
}

impl GithubRepositorySubscriptionResponse {
    pub fn from_create(result: CreateGithubRepositorySubscriptionResult) -> Self {
        Self::new(result.subscription, Some(result.replayed))
    }

    pub fn from_deactivation(result: DeactivateGithubRepositorySubscriptionResult) -> Self {
        Self::new(result.subscription, Some(result.replayed))
    }

    pub fn from_subscription(subscription: GithubRepositorySubscription) -> Self {
        Self::new(subscription, None)
    }

    fn new(subscription: GithubRepositorySubscription, replayed: Option<bool>) -> Self {
        Self {
            id: subscription.id.as_uuid(),
            organization_id: subscription.organization_id.as_uuid(),
            project_id: subscription.project_id.as_uuid(),
            environment_id: subscription.environment_id.as_uuid(),
            connection_id: subscription.connection_id.as_uuid(),
            installation_id: subscription.installation_id.as_u64(),
            repository: GitRepositoryResponse::from(&subscription.repository),
            branch: subscription.branch_name().into(),
            recipe: BuildRecipeResponse::from(&subscription.recipe),
            recipe_digest: subscription.recipe_digest,
            status: subscription.status.as_str().into(),
            aggregate_version: subscription.aggregate_version,
            created_at: subscription.created_at,
            deactivated_at: subscription.deactivated_at,
            replayed,
        }
    }
}
