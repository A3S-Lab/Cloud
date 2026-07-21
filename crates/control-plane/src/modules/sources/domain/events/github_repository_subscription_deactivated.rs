use crate::modules::sources::domain::GithubRepositorySubscription;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubRepositorySubscriptionDeactivated {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub source_subscription_id: Uuid,
}

impl GithubRepositorySubscriptionDeactivated {
    pub fn envelope(
        subscription: &GithubRepositorySubscription,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "source.github-repository-subscription.deactivated".into(),
            schema_version: 1,
            organization_id: subscription.organization_id.as_uuid(),
            aggregate_id: subscription.id.as_uuid(),
            aggregate_version: subscription.aggregate_version,
            occurred_at: subscription
                .deactivated_at
                .unwrap_or(subscription.created_at),
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: subscription.organization_id.as_uuid(),
                project_id: subscription.project_id.as_uuid(),
                environment_id: subscription.environment_id.as_uuid(),
                source_subscription_id: subscription.id.as_uuid(),
            })?,
        })
    }
}
