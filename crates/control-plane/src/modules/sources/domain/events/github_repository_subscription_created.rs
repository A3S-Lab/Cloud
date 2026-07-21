use crate::modules::sources::domain::GithubRepositorySubscription;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubRepositorySubscriptionCreated {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub source_connection_id: Uuid,
    pub source_subscription_id: Uuid,
    pub installation_id: u64,
    pub repository_identity: String,
    pub branch: String,
    pub recipe_digest: String,
}

impl GithubRepositorySubscriptionCreated {
    pub fn envelope(
        subscription: &GithubRepositorySubscription,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "source.github-repository-subscription.created".into(),
            schema_version: 1,
            organization_id: subscription.organization_id.as_uuid(),
            aggregate_id: subscription.id.as_uuid(),
            aggregate_version: subscription.aggregate_version,
            occurred_at: subscription.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: subscription.organization_id.as_uuid(),
                project_id: subscription.project_id.as_uuid(),
                environment_id: subscription.environment_id.as_uuid(),
                source_connection_id: subscription.connection_id.as_uuid(),
                source_subscription_id: subscription.id.as_uuid(),
                installation_id: subscription.installation_id.as_u64(),
                repository_identity: subscription.repository.identity().into(),
                branch: subscription.branch_name().into(),
                recipe_digest: subscription.recipe_digest.clone(),
            })?,
        })
    }
}
