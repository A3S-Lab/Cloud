use crate::modules::developer_workflows::published::{
    PullRequestPreviewLifecycleCommitted, PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY,
    PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
    PULL_REQUEST_PREVIEW_LIFECYCLE_MAX_BYTES,
};
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, GitCommitSha, RepositoryError, Sha256Digest,
};
use crate::modules::sources::application::{
    IPreviewSourceRevisionProjectionPort, PreviewSourceRevisionDesiredState,
    ProjectPreviewSourceRevision,
};
use crate::modules::sources::domain::{
    GitProvider, GitReference, GitRepository, GithubInstallationId,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Sources-owned anti-corruption adapter from Developer Workflows Published
/// Language into the local version-fenced SourceRevision projection port.
/// Delivery and retry remain on the shared Outbox Relay.
pub struct PullRequestPreviewSourceProjector {
    projections: Arc<dyn IPreviewSourceRevisionProjectionPort>,
}

impl PullRequestPreviewSourceProjector {
    pub fn new(projections: Arc<dyn IPreviewSourceRevisionProjectionPort>) -> Self {
        Self { projections }
    }

    async fn project_lifecycle(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: PullRequestPreviewLifecycleCommitted =
            serde_json::from_value(message.payload.clone())
                .map_err(|error| invalid_fact(format!("payload could not be decoded: {error}")))?;
        fact.validate().map_err(invalid_fact)?;
        let lifecycle_causation_id = message.causation_id.ok_or_else(|| {
            invalid_fact("lifecycle envelope is missing its Sources causation".into())
        })?;
        if message.event_id.is_nil()
            || message.schema_version != PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION
            || message.organization_id() != Some(fact.organization_id.as_uuid())
            || message.aggregate_id != fact.preview_id.as_uuid()
            || message.aggregate_version != fact.preview_aggregate_version
            || message.occurred_at != canonical_timestamp(message.occurred_at)
            || message.correlation_id.is_nil()
            || lifecycle_causation_id.is_nil()
        {
            return Err(invalid_fact(
                "envelope and Preview lifecycle fact identity differ".into(),
            ));
        }
        let canonical_payload = canonical_json_bounded(
            &message.payload,
            PULL_REQUEST_PREVIEW_LIFECYCLE_MAX_BYTES,
            "Preview lifecycle fact",
        )
        .map_err(invalid_fact)?;
        let base_repository = repository(
            &fact.base_repository_provider,
            &fact.base_repository_url,
            &fact.base_repository_identity,
        )?;
        let head_repository = match (
            fact.head_repository_provider.as_deref(),
            fact.head_repository_url.as_deref(),
            fact.head_repository_identity.as_deref(),
        ) {
            (None, None, None) => None,
            (Some(provider), Some(url), Some(identity)) => {
                Some(repository(provider, url, identity)?)
            }
            _ => return Err(invalid_fact("head repository binding is incomplete".into())),
        };
        let desired_state = if fact.is_active() {
            PreviewSourceRevisionDesiredState::Active
        } else {
            PreviewSourceRevisionDesiredState::CleanupRequired
        };
        let input = ProjectPreviewSourceRevision {
            lifecycle_event_id: message.event_id,
            correlation_id: message.correlation_id,
            lifecycle_causation_id,
            source_pull_request_change_id: fact.source_pull_request_change_id,
            organization_id: fact.organization_id,
            project_id: fact.project_id,
            source_environment_id: fact.source_environment_id,
            source_subscription_id: fact.source_subscription_id,
            preview_id: fact.preview_id,
            preview_aggregate_version: fact.preview_aggregate_version,
            preview_environment_id: fact.environment_id,
            installation_id: GithubInstallationId::parse(fact.installation_id)
                .map_err(invalid_fact)?,
            base_repository,
            base_branch: GitReference::parse("branch", fact.base_branch).map_err(invalid_fact)?,
            head_repository,
            head_branch: GitReference::parse("branch", fact.head_branch).map_err(invalid_fact)?,
            head_commit_sha: GitCommitSha::parse(fact.head_commit_sha).map_err(invalid_fact)?,
            pull_request_id: fact.pull_request_id,
            pull_request_number: fact.pull_request_number,
            desired_state,
            fact_digest: Sha256Digest::from_bytes(&canonical_payload),
            fact_occurred_at: message.occurred_at,
        };
        input.validate().map_err(invalid_fact)?;
        self.projections
            .project_preview_source_revision(input)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl IIntegrationEventProjector for PullRequestPreviewSourceProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        if message.event_key == PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY {
            self.project_lifecycle(message).await
        } else {
            Ok(())
        }
    }
}

fn repository(provider: &str, url: &str, identity: &str) -> Result<GitRepository, RepositoryError> {
    let repository = GitRepository::parse(GitProvider::parse(provider).map_err(invalid_fact)?, url)
        .map_err(invalid_fact)?;
    if repository.identity() != identity || repository.canonical_url() != url {
        return Err(invalid_fact(
            "repository URL and identity binding is not canonical".into(),
        ));
    }
    Ok(repository)
}

fn invalid_fact(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "Sources Preview lifecycle fact is invalid: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
        PullRequestPreviewId, PullRequestPreviewPolicyRevisionId, SourcePullRequestChangeId,
        SourceSubscriptionId,
    };
    use crate::modules::sources::application::PreviewSourceRevisionProjectionReceipt;
    use chrono::{Duration, TimeZone, Utc};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct RecordingProjection {
        inputs: Mutex<Vec<ProjectPreviewSourceRevision>>,
    }

    #[async_trait]
    impl IPreviewSourceRevisionProjectionPort for RecordingProjection {
        async fn project_preview_source_revision(
            &self,
            input: ProjectPreviewSourceRevision,
        ) -> Result<IdempotentWrite<PreviewSourceRevisionProjectionReceipt>, RepositoryError>
        {
            self.inputs.lock().await.push(input.clone());
            Ok(IdempotentWrite {
                value: PreviewSourceRevisionProjectionReceipt::from_input(
                    &input,
                    crate::modules::sources::application::PreviewSourceRevisionProjectionOutcome::IgnoredStale,
                    None,
                )
                .map_err(RepositoryError::Storage)?,
                replayed: false,
            })
        }
    }

    #[tokio::test]
    async fn translates_only_valid_versioned_preview_lifecycle_facts() {
        let recording = Arc::new(RecordingProjection::default());
        let projector = PullRequestPreviewSourceProjector::new(recording.clone());
        let active = message("active", 1);
        projector.project(&active).await.expect("active projection");

        let mut unrelated = active.clone();
        unrelated.event_key = "source.revision.accepted".into();
        projector
            .project(&unrelated)
            .await
            .expect("unrelated event");

        let cleanup = message("cleanup_required", 2);
        projector
            .project(&cleanup)
            .await
            .expect("cleanup projection");
        let inputs = recording.inputs.lock().await;
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            inputs[0].desired_state,
            PreviewSourceRevisionDesiredState::Active
        );
        assert_eq!(
            inputs[1].desired_state,
            PreviewSourceRevisionDesiredState::CleanupRequired
        );
        assert_eq!(inputs[0].preview_aggregate_version, 1);
        assert_eq!(inputs[1].preview_aggregate_version, 2);
        drop(inputs);

        let mut drifted = active;
        drifted.aggregate_version = 9;
        assert!(projector.project(&drifted).await.is_err());
        assert_eq!(recording.inputs.lock().await.len(), 2);
    }

    fn message(status: &str, version: u64) -> OutboxMessage {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let source_environment_id = EnvironmentId::new();
        let preview_environment_id = EnvironmentId::new();
        let subscription_id = SourceSubscriptionId::new();
        let preview_id = PullRequestPreviewId::new();
        let source_change_id = SourcePullRequestChangeId::new();
        let occurred_at = Utc
            .with_ymd_and_hms(2026, 8, 26, 4, 0, version as u32)
            .single()
            .expect("timestamp");
        let provider_updated_at = occurred_at - Duration::seconds(1);
        let (last_change_kind, cleanup_reason, cleanup_requested_at, head) = if status == "active" {
            ("opened", None, None, Some("github"))
        } else {
            (
                "closed",
                Some("pull_request_closed"),
                Some(provider_updated_at),
                None,
            )
        };
        let payload = PullRequestPreviewLifecycleCommitted {
            source_pull_request_change_id: source_change_id,
            organization_id,
            project_id,
            source_environment_id,
            source_subscription_id: subscription_id,
            preview_policy_revision_id: PullRequestPreviewPolicyRevisionId::new(),
            preview_policy_revision_number: 1,
            preview_policy_accepted_at: occurred_at - Duration::minutes(2),
            preview_id,
            preview_aggregate_version: version,
            environment_id: preview_environment_id,
            environment_name: format!("pr-7-{}", preview_id.as_uuid().simple()),
            owner_principal_id: PrincipalId::new(),
            installation_id: 42,
            base_repository_provider: "github".into(),
            base_repository_url: "https://github.com/a3s-lab/cloud".into(),
            base_repository_identity: "github:github.com/a3s-lab/cloud".into(),
            base_branch: "main".into(),
            head_repository_provider: head.map(str::to_owned),
            head_repository_url: head.map(|_| "https://github.com/a3s-lab/cloud".into()),
            head_repository_identity: head.map(|_| "github:github.com/a3s-lab/cloud".into()),
            head_branch: "feature/preview".into(),
            head_commit_sha: "a".repeat(40),
            pull_request_id: 42,
            pull_request_number: 7,
            provider_created_at: occurred_at - Duration::minutes(1),
            last_provider_updated_at: provider_updated_at,
            last_change_kind: last_change_kind.into(),
            last_merged: false,
            expires_at: provider_updated_at + Duration::hours(24),
            status: status.into(),
            cleanup_reason: cleanup_reason.map(str::to_owned),
            cleanup_requested_at,
            fork_policy: "isolated".into(),
            is_fork: head.is_none(),
            allow_protected_secrets_for_trusted_sources: true,
            protected_secrets_eligible: status == "active",
            lifetime_seconds: 86_400,
            maximum_active_previews: 8,
            maximum_workloads: 4,
            cpu_millis: 2_000,
            memory_bytes: 1024 * 1024 * 1024,
            ephemeral_storage_bytes: 1024 * 1024 * 1024,
        };
        OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY.into(),
            schema_version: PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_SCHEMA_VERSION,
            scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
                crate::modules::shared_kernel::domain::InstallationId::new(),
                crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                    organization_id.as_uuid(),
                ),
            )
            .expect("scope"),
            aggregate_id: preview_id.as_uuid(),
            aggregate_version: version,
            occurred_at,
            correlation_id: Uuid::now_v7(),
            causation_id: Some(Uuid::now_v7()),
            payload: serde_json::to_value(payload).expect("payload"),
            delivery_attempts: 1,
        }
    }
}
