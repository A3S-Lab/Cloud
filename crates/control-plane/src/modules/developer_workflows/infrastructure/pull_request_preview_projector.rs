use crate::modules::developer_workflows::application::{
    IPullRequestPreviewProjectionPort, ProjectCommittedPullRequestChange,
};
use crate::modules::developer_workflows::domain::{
    GitBranch, GithubInstallationRef, PullRequestChange, PullRequestChangeKind,
};
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, GitCommitSha, RepositoryError, Sha256Digest,
};
use crate::modules::sources::published::{
    PullRequestChangeCommittedFact, SourcePullRequestChangeKind,
    PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY, PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
};
use async_trait::async_trait;
use std::sync::Arc;

const MAX_COMMITTED_PULL_REQUEST_FACT_BYTES: usize = 16 * 1024;

/// Anti-corruption adapter from the Sources Published Language to the local
/// Developer Workflows projection port. Delivery, retry, and publication stay
/// on the existing Outbox Relay.
pub struct PullRequestPreviewProjector {
    previews: Arc<dyn IPullRequestPreviewProjectionPort>,
}

impl PullRequestPreviewProjector {
    pub fn new(previews: Arc<dyn IPullRequestPreviewProjectionPort>) -> Self {
        Self { previews }
    }

    async fn project_pull_request(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: PullRequestChangeCommittedFact = serde_json::from_value(message.payload.clone())
            .map_err(|error| invalid_fact(format!("payload could not be decoded: {error}")))?;
        fact.validate().map_err(invalid_fact)?;
        if message.schema_version != PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION
            || message.organization_id != fact.organization_id().as_uuid()
            || message.aggregate_id != fact.source_pull_request_change_id().as_uuid()
            || message.aggregate_version != 1
            || message.occurred_at != canonical_timestamp(message.occurred_at)
            || message.correlation_id.is_nil()
            || message
                .causation_id
                .is_some_and(|causation_id| causation_id.is_nil())
        {
            return Err(invalid_fact(
                "envelope and committed fact identity differ".into(),
            ));
        }
        let canonical_payload = canonical_json_bounded(
            &message.payload,
            MAX_COMMITTED_PULL_REQUEST_FACT_BYTES,
            "committed pull-request fact",
        )
        .map_err(invalid_fact)?;
        let input = ProjectCommittedPullRequestChange {
            source_pull_request_change_id: fact.source_pull_request_change_id(),
            organization_id: fact.organization_id(),
            project_id: fact.project_id(),
            source_environment_id: fact.environment_id(),
            source_subscription_id: fact.source_subscription_id(),
            change: PullRequestChange {
                installation_id: GithubInstallationRef::parse(fact.installation_id())
                    .map_err(invalid_fact)?,
                base_repository: fact.base_repository().clone(),
                base_branch: GitBranch::parse(fact.base_branch()).map_err(invalid_fact)?,
                head_repository: fact.head_repository().cloned(),
                head_branch: GitBranch::parse(fact.head_branch()).map_err(invalid_fact)?,
                head_commit_sha: GitCommitSha::parse(fact.head_commit_sha())
                    .map_err(invalid_fact)?,
                pull_request_id: fact.pull_request_id(),
                pull_request_number: fact.pull_request_number(),
                kind: local_kind(fact.kind()),
                merged: fact.merged(),
                provider_created_at: fact.provider_created_at(),
                provider_updated_at: fact.provider_updated_at(),
            },
            fact_digest: Sha256Digest::from_bytes(&canonical_payload),
            fact_occurred_at: message.occurred_at,
        };
        self.previews.project_committed_change(input).await?;
        Ok(())
    }
}

#[async_trait]
impl IIntegrationEventProjector for PullRequestPreviewProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        if message.event_key == PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY {
            self.project_pull_request(message).await
        } else {
            Ok(())
        }
    }
}

const fn local_kind(kind: SourcePullRequestChangeKind) -> PullRequestChangeKind {
    match kind {
        SourcePullRequestChangeKind::Opened => PullRequestChangeKind::Opened,
        SourcePullRequestChangeKind::Synchronized => PullRequestChangeKind::Synchronized,
        SourcePullRequestChangeKind::Reopened => PullRequestChangeKind::Reopened,
        SourcePullRequestChangeKind::Closed => PullRequestChangeKind::Closed,
    }
}

fn invalid_fact(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "Developer Workflows committed pull-request fact is invalid: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::domain::{
        PullRequestPreviewProjectionOutcome, PullRequestPreviewProjectionReceipt,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotentWrite, OrganizationId, ProjectId, SourcePullRequestChangeId,
        SourceSubscriptionId,
    };
    use crate::modules::sources::published::{GitProvider, GitRepository};
    use chrono::{TimeZone, Utc};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct RecordingProjection {
        inputs: Mutex<Vec<ProjectCommittedPullRequestChange>>,
    }

    #[async_trait]
    impl IPullRequestPreviewProjectionPort for RecordingProjection {
        async fn project_committed_change(
            &self,
            input: ProjectCommittedPullRequestChange,
        ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
            self.inputs.lock().await.push(input.clone());
            Ok(IdempotentWrite {
                value: PullRequestPreviewProjectionReceipt {
                    source_pull_request_change_id: input.source_pull_request_change_id,
                    organization_id: input.organization_id,
                    project_id: input.project_id,
                    source_environment_id: input.source_environment_id,
                    source_subscription_id: input.source_subscription_id,
                    pull_request_id: input.change.pull_request_id,
                    pull_request_number: input.change.pull_request_number,
                    fact_digest: input.fact_digest,
                    fact_occurred_at: input.fact_occurred_at,
                    policy_revision_id: None,
                    preview_id: None,
                    preview_aggregate_version: None,
                    outcome: PullRequestPreviewProjectionOutcome::NoApplicablePolicy,
                },
                replayed: false,
            })
        }
    }

    #[tokio::test]
    async fn maps_only_the_closed_sources_published_language_to_the_local_port() {
        let port = Arc::new(RecordingProjection::default());
        let projector = PullRequestPreviewProjector::new(port.clone());
        let message = message();
        let expected_digest = Sha256Digest::from_bytes(
            &canonical_json_bounded(
                &message.payload,
                MAX_COMMITTED_PULL_REQUEST_FACT_BYTES,
                "test fact",
            )
            .expect("canonical payload"),
        );

        projector.project(&message).await.expect("projection");
        let inputs = port.inputs.lock().await;
        assert_eq!(inputs.len(), 1);
        let input = &inputs[0];
        assert_eq!(input.organization_id.as_uuid(), message.organization_id);
        assert_eq!(
            input.source_pull_request_change_id.as_uuid(),
            message.aggregate_id
        );
        assert_eq!(input.fact_digest, expected_digest);
        assert_eq!(input.change.kind, PullRequestChangeKind::Opened);
        assert_eq!(input.change.base_branch.as_str(), "main");
        assert_eq!(input.change.head_commit_sha.as_str(), "a".repeat(40));
        assert_eq!(input.fact_occurred_at, message.occurred_at);
    }

    #[tokio::test]
    async fn ignores_other_events_and_rejects_envelope_identity_drift() {
        let port = Arc::new(RecordingProjection::default());
        let projector = PullRequestPreviewProjector::new(port.clone());
        let mut unrelated = message();
        unrelated.event_key = "source.revision.accepted".into();
        projector
            .project(&unrelated)
            .await
            .expect("irrelevant event is a no-op");
        assert!(port.inputs.lock().await.is_empty());

        let mut drifted = message();
        drifted.aggregate_id = Uuid::now_v7();
        assert!(matches!(
            projector.project(&drifted).await,
            Err(RepositoryError::Storage(message))
                if message.contains("envelope and committed fact identity differ")
        ));
        assert!(port.inputs.lock().await.is_empty());
    }

    fn message() -> OutboxMessage {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let subscription_id = SourceSubscriptionId::new();
        let change_id = SourcePullRequestChangeId::new();
        let base_repository =
            GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
                .expect("repository");
        let occurred_at = Utc
            .with_ymd_and_hms(2026, 8, 26, 2, 30, 0)
            .single()
            .expect("timestamp");
        OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY.into(),
            schema_version: PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
            organization_id: organization_id.as_uuid(),
            aggregate_id: change_id.as_uuid(),
            aggregate_version: 1,
            occurred_at,
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({
                "source_pull_request_change_id": change_id,
                "organization_id": organization_id,
                "project_id": project_id,
                "environment_id": environment_id,
                "source_subscription_id": subscription_id,
                "installation_id": 42,
                "base_repository": base_repository,
                "base_branch": "main",
                "head_repository": base_repository,
                "head_branch": "feature/preview",
                "head_commit_sha": "a".repeat(40),
                "pull_request_id": 1_000_042,
                "pull_request_number": 42,
                "kind": "opened",
                "merged": false,
                "provider_created_at": occurred_at - chrono::Duration::minutes(1),
                "provider_updated_at": occurred_at - chrono::Duration::seconds(1),
            }),
            delivery_attempts: 1,
        }
    }
}
