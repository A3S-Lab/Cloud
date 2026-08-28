use crate::modules::developer_workflows::application::{
    EnsurePreviewEnvironment, IPreviewEnvironmentPort, IPullRequestPreviewProjectionPort,
    PreviewEnvironmentBinding, ProjectCommittedPullRequestChange,
};
use crate::modules::developer_workflows::domain::{
    GitBranch, GithubInstallationRef, PullRequestChange, PullRequestChangeKind,
    PullRequestPreviewLifecycleEvent,
};
use crate::modules::developer_workflows::published::PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY;
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, GitCommitSha, RepositoryError, Sha256Digest,
};
use crate::modules::sources::published::{
    PullRequestChangeCommittedFact, SourcePullRequestChangeKind,
    PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY, PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::sync::Arc;

const MAX_COMMITTED_PULL_REQUEST_FACT_BYTES: usize = 16 * 1024;

/// One Developer Workflows event router: it translates Sources Published
/// Language into the local Preview projection port and committed local Preview
/// language into owner-facing ports. Delivery, retry, and publication stay on
/// the existing Outbox Relay.
pub struct PullRequestPreviewProjector {
    previews: Arc<dyn IPullRequestPreviewProjectionPort>,
    environments: Arc<dyn IPreviewEnvironmentPort>,
}

impl PullRequestPreviewProjector {
    pub fn new(
        previews: Arc<dyn IPullRequestPreviewProjectionPort>,
        environments: Arc<dyn IPreviewEnvironmentPort>,
    ) -> Self {
        Self {
            previews,
            environments,
        }
    }

    async fn project_pull_request(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        let fact: PullRequestChangeCommittedFact = serde_json::from_value(message.payload.clone())
            .map_err(|error| invalid_fact(format!("payload could not be decoded: {error}")))?;
        fact.validate().map_err(invalid_fact)?;
        if message.schema_version != PULL_REQUEST_CHANGE_COMMITTED_SCHEMA_VERSION
            || message.organization_id() != Some(fact.organization_id().as_uuid())
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
            source_event_id: message.event_id,
            correlation_id: message.correlation_id,
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

    async fn project_preview_environment(
        &self,
        message: &OutboxMessage,
    ) -> Result<(), RepositoryError> {
        let fact = PullRequestPreviewLifecycleEvent::from_envelope(&envelope(message))
            .map_err(invalid_lifecycle)?;
        if !fact.is_active() {
            return Ok(());
        }
        let binding = PreviewEnvironmentBinding {
            organization_id: fact.organization_id,
            project_id: fact.project_id,
            preview_id: fact.preview_id,
            environment_id: fact.environment_id,
            pull_request_number: fact.pull_request_number,
            name: fact.environment_name,
            created_at: fact.provider_created_at,
        };
        binding.validate().map_err(invalid_lifecycle)?;
        let write = self
            .environments
            .ensure_preview_environment(EnsurePreviewEnvironment {
                binding: binding.clone(),
                correlation_id: message.correlation_id,
                causation_id: message.event_id,
            })
            .await?;
        write
            .value
            .validate_for(&binding)
            .map_err(invalid_lifecycle)
    }
}

#[async_trait]
impl IIntegrationEventProjector for PullRequestPreviewProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        if message.event_key == PULL_REQUEST_CHANGE_COMMITTED_EVENT_KEY {
            self.project_pull_request(message).await
        } else if message.event_key == PULL_REQUEST_PREVIEW_LIFECYCLE_COMMITTED_EVENT_KEY {
            self.project_preview_environment(message).await
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

fn invalid_lifecycle(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "Developer Workflows Preview lifecycle fact is invalid: {error}"
    ))
}

fn envelope(message: &OutboxMessage) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: message.event_id,
        event_key: message.event_key.clone(),
        schema_version: message.schema_version,
        scope: message.scope.reference(),
        aggregate_id: message.aggregate_id,
        aggregate_version: message.aggregate_version,
        occurred_at: message.occurred_at,
        correlation_id: message.correlation_id,
        causation_id: message.causation_id,
        payload: message.payload.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::application::{
        EnsurePreviewEnvironment, IPreviewEnvironmentPort, PreviewEnvironmentReceipt,
    };
    use crate::modules::developer_workflows::domain::{
        reconcile_pull_request_preview, PreviewForkPolicy, PreviewQuota, PullRequestPreviewPolicy,
        PullRequestPreviewPolicyAuthority, PullRequestPreviewProjectionOutcome,
        PullRequestPreviewProjectionReceipt,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
        PullRequestPreviewPolicyRevisionId, SourcePullRequestChangeId, SourceSubscriptionId,
    };
    use crate::modules::sources::published::{GitProvider, GitRepository};
    use chrono::{TimeZone, Utc};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct RecordingProjection {
        inputs: Mutex<Vec<ProjectCommittedPullRequestChange>>,
    }

    #[derive(Default)]
    struct RecordingEnvironments {
        requests: Mutex<Vec<EnsurePreviewEnvironment>>,
    }

    #[async_trait]
    impl IPreviewEnvironmentPort for RecordingEnvironments {
        async fn ensure_preview_environment(
            &self,
            request: EnsurePreviewEnvironment,
        ) -> Result<IdempotentWrite<PreviewEnvironmentReceipt>, RepositoryError> {
            request.validate().map_err(RepositoryError::Storage)?;
            self.requests.lock().await.push(request.clone());
            Ok(IdempotentWrite {
                value: PreviewEnvironmentReceipt {
                    binding: request.binding,
                    environment_aggregate_version: 1,
                },
                replayed: false,
            })
        }
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
        let projector = PullRequestPreviewProjector::new(
            port.clone(),
            Arc::new(RecordingEnvironments::default()),
        );
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
        assert_eq!(
            Some(input.organization_id.as_uuid()),
            message.organization_id()
        );
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
        let projector = PullRequestPreviewProjector::new(
            port.clone(),
            Arc::new(RecordingEnvironments::default()),
        );
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

    #[tokio::test]
    async fn active_lifecycle_ensures_one_projects_environment_while_cleanup_does_not_create() {
        let previews = Arc::new(RecordingProjection::default());
        let environments = Arc::new(RecordingEnvironments::default());
        let projector = PullRequestPreviewProjector::new(previews, environments.clone());

        let (active, cleanup) = lifecycle_messages();
        projector
            .project(&active)
            .await
            .expect("active Environment handoff");
        let requests = environments.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].binding.name,
            format!(
                "pr-42-{}",
                requests[0].binding.preview_id.as_uuid().simple()
            )
        );
        drop(requests);

        projector
            .project(&cleanup)
            .await
            .expect("cleanup fact is a bounded no-op for Projects creation");
        assert_eq!(environments.requests.lock().await.len(), 1);

        let reordered_environments = Arc::new(RecordingEnvironments::default());
        let reordered = PullRequestPreviewProjector::new(
            Arc::new(RecordingProjection::default()),
            reordered_environments.clone(),
        );
        let (reordered_active, reordered_cleanup) = lifecycle_messages();
        reordered
            .project(&reordered_cleanup)
            .await
            .expect("cleanup may arrive before creation evidence");
        reordered
            .project(&reordered_active)
            .await
            .expect("creation-only handoff is order independent");
        assert_eq!(reordered_environments.requests.lock().await.len(), 1);

        let mut invalid_cleanup = cleanup;
        invalid_cleanup.event_id = Uuid::nil();
        assert!(projector.project(&invalid_cleanup).await.is_err());

        let mut oversized = active.clone();
        oversized.payload["padding"] = serde_json::Value::String("x".repeat(16 * 1024));
        assert!(projector.project(&oversized).await.is_err());

        let mut drifted = active;
        drifted.aggregate_version += 1;
        assert!(projector.project(&drifted).await.is_err());
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
            scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
                crate::modules::shared_kernel::domain::InstallationId::new(),
                crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                    organization_id.as_uuid(),
                ),
            )
            .expect("scope"),
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

    fn lifecycle_messages() -> (OutboxMessage, OutboxMessage) {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let source_environment_id = EnvironmentId::new();
        let source_subscription_id = SourceSubscriptionId::new();
        let base_repository =
            GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
                .expect("repository");
        let opened_at = Utc
            .with_ymd_and_hms(2026, 8, 26, 3, 0, 0)
            .single()
            .expect("timestamp");
        let authority = PullRequestPreviewPolicyAuthority {
            source_environment_id,
            revision_id: PullRequestPreviewPolicyRevisionId::new(),
            revision_number: 1,
            accepted_at: opened_at - chrono::Duration::minutes(1),
            policy: PullRequestPreviewPolicy {
                organization_id,
                project_id,
                source_subscription_id,
                owner_principal_id: PrincipalId::new(),
                installation_id: GithubInstallationRef::parse(42).expect("installation"),
                base_repository: base_repository.clone(),
                base_branch: GitBranch::parse("main").expect("branch"),
                lifetime_seconds: 86_400,
                maximum_active_previews: 8,
                fork_policy: PreviewForkPolicy::Isolated,
                allow_protected_secrets_for_trusted_sources: true,
                quota: PreviewQuota {
                    maximum_workloads: 4,
                    cpu_millis: 2_000,
                    memory_bytes: 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 1024 * 1024 * 1024,
                },
            },
        };
        let opened = PullRequestChange {
            installation_id: GithubInstallationRef::parse(42).expect("installation"),
            base_repository: base_repository.clone(),
            base_branch: GitBranch::parse("main").expect("branch"),
            head_repository: Some(base_repository.clone()),
            head_branch: GitBranch::parse("feature/preview").expect("branch"),
            head_commit_sha: GitCommitSha::parse("a".repeat(40)).expect("commit"),
            pull_request_id: 1_000_042,
            pull_request_number: 42,
            kind: PullRequestChangeKind::Opened,
            merged: false,
            provider_created_at: opened_at,
            provider_updated_at: opened_at,
        };
        let active = reconcile_pull_request_preview(&authority, None, &opened)
            .expect("active Preview")
            .preview
            .expect("Preview");
        let active_event = PullRequestPreviewLifecycleEvent::envelope(
            &active,
            SourcePullRequestChangeId::new(),
            opened_at + chrono::Duration::seconds(1),
            Uuid::now_v7(),
            Uuid::now_v7(),
        )
        .expect("active lifecycle event");
        let closed_at = opened_at + chrono::Duration::seconds(10);
        let closed = PullRequestChange {
            kind: PullRequestChangeKind::Closed,
            provider_updated_at: closed_at,
            head_repository: None,
            ..opened
        };
        let cleanup = reconcile_pull_request_preview(&authority, Some(&active), &closed)
            .expect("cleanup Preview")
            .preview
            .expect("Preview");
        let cleanup_event = PullRequestPreviewLifecycleEvent::envelope(
            &cleanup,
            SourcePullRequestChangeId::new(),
            closed_at + chrono::Duration::seconds(1),
            Uuid::now_v7(),
            Uuid::now_v7(),
        )
        .expect("cleanup lifecycle event");
        (outbox_message(active_event), outbox_message(cleanup_event))
    }

    fn outbox_message(event: DomainEventEnvelope) -> OutboxMessage {
        let event_organization_id = event.organization_id().expect("tenant event");
        OutboxMessage {
            event_id: event.event_id,
            event_key: event.event_key,
            schema_version: event.schema_version,
            scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
                crate::modules::shared_kernel::domain::InstallationId::new(),
                crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                    event_organization_id,
                ),
            )
            .expect("scope"),
            aggregate_id: event.aggregate_id,
            aggregate_version: event.aggregate_version,
            occurred_at: event.occurred_at,
            correlation_id: event.correlation_id,
            causation_id: event.causation_id,
            payload: event.payload,
            delivery_attempts: 1,
        }
    }
}
