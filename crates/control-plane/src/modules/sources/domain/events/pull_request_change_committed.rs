use crate::modules::shared_kernel::domain::SourcePullRequestChangeId;
use crate::modules::sources::domain::{
    GithubRepositorySubscription, PullRequestChangeKind, SourceWebhookDelivery,
    SourceWebhookPayload,
};
use crate::modules::sources::published::{
    PullRequestChangeCommittedFact, SourcePullRequestChangeKind,
    PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY, PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use uuid::Uuid;

const SOURCE_PULL_REQUEST_CHANGE_NAMESPACE: Uuid = Uuid::from_bytes([
    0x53, 0x5f, 0x3b, 0x88, 0x5d, 0x69, 0x46, 0x7a, 0x92, 0x5f, 0x8b, 0x7e, 0xab, 0x93, 0x26, 0x0f,
]);

pub struct PullRequestChangeCommitted;

impl PullRequestChangeCommitted {
    pub fn fact(
        subscription: &GithubRepositorySubscription,
        delivery: &SourceWebhookDelivery,
    ) -> Result<PullRequestChangeCommittedFact, String> {
        if GithubRepositorySubscription::restore(subscription.clone())? != *subscription {
            return Err("pull-request Source subscription is not canonical".into());
        }
        let SourceWebhookPayload::PullRequest(change) = &delivery.payload else {
            return Err("a push delivery cannot publish a pull-request change fact".into());
        };
        if !subscription.is_active()
            || subscription.installation_id != delivery.installation_id
            || subscription.repository != delivery.repository
            || subscription.branch_name() != change.base_reference.value()
        {
            return Err(
                "pull-request delivery does not match its exact active Source subscription".into(),
            );
        }
        let fact = PullRequestChangeCommittedFact::new(
            change_id(subscription, delivery),
            subscription.organization_id,
            subscription.project_id,
            subscription.environment_id,
            subscription.id,
            delivery.installation_id.as_u64(),
            delivery.repository.clone(),
            change.base_reference.value().to_owned(),
            change.head_repository.clone(),
            change.head_reference.value().to_owned(),
            change.head_commit_sha.as_str().to_owned(),
            change.pull_request_id,
            change.pull_request_number,
            published_kind(change.kind),
            change.merged,
            change.provider_created_at,
            change.provider_updated_at,
        );
        fact.validate()?;
        Ok(fact)
    }

    pub fn envelope(
        fact: &PullRequestChangeCommittedFact,
        occurred_at: chrono::DateTime<chrono::Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY.into(),
            schema_version: PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: fact.organization_id().as_uuid(),
            },
            aggregate_id: fact.source_pull_request_change_id().as_uuid(),
            aggregate_version: 1,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(fact)?,
        })
    }
}

fn change_id(
    subscription: &GithubRepositorySubscription,
    delivery: &SourceWebhookDelivery,
) -> SourcePullRequestChangeId {
    let mut identity = Vec::with_capacity(16 + 1 + 128);
    identity.extend_from_slice(subscription.id.as_uuid().as_bytes());
    identity.push(0);
    identity.extend_from_slice(delivery.provider.as_str().as_bytes());
    identity.push(0);
    identity.extend_from_slice(delivery.delivery_id.as_str().as_bytes());
    SourcePullRequestChangeId::from_uuid(Uuid::new_v5(
        &SOURCE_PULL_REQUEST_CHANGE_NAMESPACE,
        &identity,
    ))
}

const fn published_kind(kind: PullRequestChangeKind) -> SourcePullRequestChangeKind {
    match kind {
        PullRequestChangeKind::Opened => SourcePullRequestChangeKind::Opened,
        PullRequestChangeKind::Synchronized => SourcePullRequestChangeKind::Synchronized,
        PullRequestChangeKind::Reopened => SourcePullRequestChangeKind::Reopened,
        PullRequestChangeKind::Closed => SourcePullRequestChangeKind::Closed,
    }
}
