use crate::modules::artifacts::application::{
    hosted_build_outcome_event, BuildCandidate, BuildCandidateEvidence,
    IBuildCandidateProjectionPort, IPreviewBuildLifecycleProjectionPort,
    PreviewBuildLifecycleProjectionOutcome, PreviewBuildLifecycleProjectionReceipt,
    PreviewBuildLifecycleState, PreviewBuildRetirement, ProjectPreviewBuildLifecycle,
};
use crate::modules::artifacts::domain::repositories::{
    validate_build_run_finalization, validate_build_run_retry, validate_build_run_transition,
    BuildRunFinalizationMode,
};
use crate::modules::artifacts::domain::{
    BuildRun, BuildSubject, IBuildRunRepository, RequestBuildCancellationBundle,
    RequestBuildRetryBundle,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, EnvironmentId, GitCommitSha, IdempotencyRequest,
    IdempotentWrite, OrganizationId, ProjectId, PullRequestPreviewId, RepositoryError,
    Sha256Digest, SourceRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

mod build_run_repository;
mod candidate_projection;

#[derive(Default)]
pub struct InMemoryBuildRunRepository {
    state: RwLock<State>,
}

#[derive(Default, Clone)]
struct State {
    builds: BTreeMap<(OrganizationId, BuildRunId), BuildRun>,
    candidates: BTreeMap<(OrganizationId, u8, uuid::Uuid), BuildCandidate>,
    preview_lifecycle_receipts: BTreeMap<
        (OrganizationId, PullRequestPreviewId, u64),
        PreviewBuildLifecycleProjectionReceipt,
    >,
    preview_lifecycle_events: BTreeMap<uuid::Uuid, (OrganizationId, PullRequestPreviewId, u64)>,
    started_operations: BTreeSet<BuildRunId>,
    cancellation_idempotency: BTreeMap<(String, String), (String, BuildRun)>,
    retry_idempotency: BTreeMap<(String, String), (String, BuildRun)>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

impl InMemoryBuildRunRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add_source_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        accepted_at: DateTime<Utc>,
    ) {
        self.project_candidate(
            BuildCandidate::new(
                organization_id,
                BuildSubject::external_source_revision(
                    project_id,
                    environment_id,
                    source_revision_id,
                ),
                BuildCandidateEvidence::external_source_revision(
                    "github:github.com/a3s-lab/test-build-candidate".into(),
                    GitCommitSha::parse("a".repeat(40)).expect("test Source commit"),
                    Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
                        .expect("test Source recipe digest"),
                )
                .expect("test Source evidence"),
                accepted_at,
            )
            .expect("test Source build candidate"),
        )
        .await
        .expect("project test Source build candidate");
    }

    pub async fn add_asset_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        drafted_at: DateTime<Utc>,
    ) {
        self.project_candidate(
            BuildCandidate::new(
                organization_id,
                BuildSubject::asset_release(asset_id, asset_release_id),
                BuildCandidateEvidence::hosted_asset_release(
                    GitCommitSha::parse("c".repeat(40)).expect("test hosted Asset commit"),
                    Sha256Digest::parse(format!("sha256:{}", "d".repeat(64)))
                        .expect("test hosted Asset manifest digest"),
                ),
                drafted_at,
            )
            .expect("test hosted Asset build candidate"),
        )
        .await
        .expect("project test hosted Asset build candidate");
    }

    pub async fn mark_operation_started(&self, build_run_id: BuildRunId) {
        self.state
            .write()
            .await
            .started_operations
            .insert(build_run_id);
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    #[cfg(test)]
    pub(crate) async fn seed_build(&self, build: BuildRun) {
        self.state
            .write()
            .await
            .builds
            .insert((build.organization_id, build.id), build);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::application::PreviewBuildSourceRevision;
    use crate::modules::artifacts::domain::test_support::hosted_build_ready_for_completion;
    use crate::modules::artifacts::domain::{BuildRunStatus, BuildSubject};
    use crate::modules::artifacts::published::{
        HostedBuildOutcome, HOSTED_BUILD_OUTCOME_EVENT_KEY,
    };
    use crate::modules::shared_kernel::domain::{SourcePullRequestChangeId, SourceSubscriptionId};
    use chrono::{Duration, TimeZone};
    use std::sync::Arc;

    struct PreviewFixture {
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        preview_id: PullRequestPreviewId,
        preview_environment_id: EnvironmentId,
        correlation_id: uuid::Uuid,
        base_at: DateTime<Utc>,
    }

    impl PreviewFixture {
        fn new() -> Self {
            Self {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                source_environment_id: EnvironmentId::new(),
                source_subscription_id: SourceSubscriptionId::new(),
                preview_id: PullRequestPreviewId::new(),
                preview_environment_id: EnvironmentId::new(),
                correlation_id: uuid::Uuid::now_v7(),
                base_at: Utc
                    .with_ymd_and_hms(2026, 8, 26, 1, 0, 0)
                    .single()
                    .expect("Preview base time"),
            }
        }

        fn active(
            &self,
            version: u64,
            source_revision_id: SourceRevisionId,
            accepted_at: DateTime<Utc>,
            marker: char,
        ) -> ProjectPreviewBuildLifecycle {
            self.input(
                version,
                PreviewBuildLifecycleState::Active,
                Some(PreviewBuildSourceRevision {
                    source_revision_id,
                    repository_identity: format!("github:github.com/a3s-lab/preview-{marker}"),
                    commit_sha: GitCommitSha::parse(marker.to_string().repeat(40))
                        .expect("Preview commit"),
                    recipe_digest: Sha256Digest::parse(format!(
                        "sha256:{}",
                        marker.to_string().repeat(64)
                    ))
                    .expect("Preview recipe digest"),
                    accepted_at,
                }),
            )
        }

        fn inactive(
            &self,
            version: u64,
            state: PreviewBuildLifecycleState,
        ) -> ProjectPreviewBuildLifecycle {
            self.input(version, state, None)
        }

        fn input(
            &self,
            version: u64,
            state: PreviewBuildLifecycleState,
            source_revision: Option<PreviewBuildSourceRevision>,
        ) -> ProjectPreviewBuildLifecycle {
            ProjectPreviewBuildLifecycle {
                lifecycle_event_id: uuid::Uuid::now_v7(),
                correlation_id: self.correlation_id,
                lifecycle_causation_id: uuid::Uuid::now_v7(),
                source_pull_request_change_id: SourcePullRequestChangeId::new(),
                organization_id: self.organization_id,
                project_id: self.project_id,
                source_environment_id: self.source_environment_id,
                source_subscription_id: self.source_subscription_id,
                preview_id: self.preview_id,
                preview_aggregate_version: version,
                preview_environment_id: self.preview_environment_id,
                state,
                source_revision,
                fact_occurred_at: self.base_at + Duration::seconds(version as i64),
            }
        }
    }

    #[tokio::test]
    async fn concurrent_reservation_creates_one_build_per_revision() {
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let organization_id = OrganizationId::new();
        let source_revision_id = SourceRevisionId::new();
        let accepted_at = Utc::now();
        repository
            .add_source_revision(
                organization_id,
                ProjectId::new(),
                EnvironmentId::new(),
                source_revision_id,
                accepted_at,
            )
            .await;

        let (left, right) =
            tokio::join!(repository.reserve_pending(1), repository.reserve_pending(1));
        let reserved =
            left.expect("left reservation").len() + right.expect("right reservation").len();
        assert_eq!(reserved, 1);
        assert!(repository
            .find_by_source_revision(organization_id, source_revision_id)
            .await
            .expect("find build")
            .is_some());
    }

    #[tokio::test]
    async fn concurrent_reservation_repairs_one_unbuilt_hosted_release() {
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        let drafted_at = Utc::now();
        repository
            .add_asset_release(organization_id, asset_id, asset_release_id, drafted_at)
            .await;

        let (left, right) =
            tokio::join!(repository.reserve_pending(1), repository.reserve_pending(1));
        let reserved =
            left.expect("left reservation").len() + right.expect("right reservation").len();
        assert_eq!(reserved, 1);
        let build = repository
            .find_by_asset_release(organization_id, asset_release_id)
            .await
            .expect("find hosted build")
            .expect("hosted build");
        assert_eq!(
            build.subject,
            BuildSubject::asset_release(asset_id, asset_release_id)
        );
    }

    #[tokio::test]
    async fn preview_lifecycle_cancels_retired_build_and_reopens_same_revision_once() {
        let repository = InMemoryBuildRunRepository::new();
        let fixture = PreviewFixture::new();
        let source_revision_id = SourceRevisionId::new();
        let accepted_at = fixture.base_at + Duration::seconds(1);
        let active = fixture.active(1, source_revision_id, accepted_at, 'a');
        let projected = repository
            .project_preview_build_lifecycle(active.clone())
            .await
            .expect("project active Preview build");
        assert!(!projected.replayed);
        assert_eq!(
            projected.value.outcome,
            PreviewBuildLifecycleProjectionOutcome::Applied
        );

        let first = repository
            .reserve_pending(1)
            .await
            .expect("reserve active Preview build")
            .pop()
            .expect("first Preview build");
        assert_eq!(first.source_revision_id(), Some(source_revision_id));
        assert_eq!(first.attempt, 1);

        let cleanup = fixture.inactive(2, PreviewBuildLifecycleState::CleanupRequired);
        let cleaned = repository
            .project_preview_build_lifecycle(cleanup)
            .await
            .expect("project Preview cleanup");
        assert_eq!(
            cleaned.value.retirement,
            PreviewBuildRetirement::CancellationRequested {
                source_revision_id,
                build_run_id: first.id,
            }
        );
        let cancelling = repository
            .find(fixture.organization_id, first.id)
            .await
            .expect("cancelled Preview build");
        assert_eq!(cancelling.status, BuildRunStatus::Cancelling);
        assert!(repository
            .reserve_pending(1)
            .await
            .expect("suppressed reservation")
            .is_empty());

        let expected_version = cancelling.aggregate_version;
        let mut cancelled = cancelling;
        cancelled
            .complete(fixture.base_at + Duration::seconds(2) + Duration::milliseconds(1))
            .expect("complete Preview cancellation");
        let cancelled = repository
            .finalize(cancelled, expected_version)
            .await
            .expect("finalize Preview cancellation");
        assert_eq!(cancelled.status, BuildRunStatus::Cancelled);

        repository
            .project_preview_build_lifecycle(fixture.active(
                3,
                source_revision_id,
                accepted_at,
                'a',
            ))
            .await
            .expect("reopen Preview revision");
        let retry = repository
            .reserve_pending(1)
            .await
            .expect("reserve reopened Preview build")
            .pop()
            .expect("Preview retry");
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.retry_of_build_run_id, Some(first.id));
        assert!(repository
            .reserve_pending(1)
            .await
            .expect("repeat reopened reservation")
            .is_empty());
    }

    #[tokio::test]
    async fn preview_head_suppresses_unreserved_predecessor_and_ignores_late_fact() {
        let repository = InMemoryBuildRunRepository::new();
        let fixture = PreviewFixture::new();
        let first_revision_id = SourceRevisionId::new();
        let second_revision_id = SourceRevisionId::new();
        let first = fixture.active(
            1,
            first_revision_id,
            fixture.base_at + Duration::seconds(1),
            'b',
        );
        repository
            .project_preview_build_lifecycle(first.clone())
            .await
            .expect("project first Preview revision");
        let second = fixture.active(
            3,
            second_revision_id,
            fixture.base_at + Duration::seconds(3),
            'c',
        );
        let advanced = repository
            .project_preview_build_lifecycle(second.clone())
            .await
            .expect("advance Preview revision");
        assert_eq!(
            advanced.value.retirement,
            PreviewBuildRetirement::PendingSuppressed {
                source_revision_id: first_revision_id,
            }
        );
        let reserved = repository
            .reserve_pending(2)
            .await
            .expect("reserve current Preview build");
        assert_eq!(reserved.len(), 1);
        assert_eq!(reserved[0].source_revision_id(), Some(second_revision_id));

        let replay = repository
            .project_preview_build_lifecycle(second.clone())
            .await
            .expect("replay current Preview fact");
        assert!(replay.replayed);
        let mut conflict = second;
        conflict.correlation_id = uuid::Uuid::now_v7();
        assert!(matches!(
            repository.project_preview_build_lifecycle(conflict).await,
            Err(RepositoryError::Conflict(_))
        ));

        let late = ProjectPreviewBuildLifecycle {
            lifecycle_event_id: uuid::Uuid::now_v7(),
            lifecycle_causation_id: uuid::Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            preview_aggregate_version: 2,
            fact_occurred_at: fixture.base_at + Duration::seconds(2),
            ..first
        };
        let ignored = repository
            .project_preview_build_lifecycle(late)
            .await
            .expect("ignore late Preview fact");
        assert_eq!(
            ignored.value.outcome,
            PreviewBuildLifecycleProjectionOutcome::IgnoredStale
        );
        assert!(repository
            .find_by_source_revision(fixture.organization_id, first_revision_id)
            .await
            .expect("find suppressed Preview build")
            .is_none());
    }

    #[tokio::test]
    async fn save_accepts_one_domain_transition_and_rejects_stale_or_forged_state() {
        let repository = InMemoryBuildRunRepository::new();
        let organization_id = OrganizationId::new();
        let source_revision_id = SourceRevisionId::new();
        let accepted_at = Utc::now();
        repository
            .add_source_revision(
                organization_id,
                ProjectId::new(),
                EnvironmentId::new(),
                source_revision_id,
                accepted_at,
            )
            .await;
        let reserved = repository
            .reserve_pending(1)
            .await
            .expect("reserve build")
            .pop()
            .expect("reserved build");
        let stale = reserved.clone();
        let mut preparing = reserved;
        preparing
            .begin_preparation(accepted_at + Duration::milliseconds(1))
            .expect("begin preparation");
        let preparing = repository
            .save(preparing, stale.aggregate_version)
            .await
            .expect("save preparation");

        let mut stale_update = stale;
        stale_update
            .begin_preparation(accepted_at + Duration::milliseconds(2))
            .expect("prepare stale build");
        let stale_expected_version = stale_update.aggregate_version - 1;
        assert!(matches!(
            repository.save(stale_update, stale_expected_version).await,
            Err(RepositoryError::Conflict(_))
        ));

        assert!(matches!(
            repository
                .save(preparing.clone(), preparing.aggregate_version)
                .await,
            Err(RepositoryError::Conflict(_))
        ));

        let mut forged = preparing.clone();
        forged.subject = BuildSubject::external_source_revision(
            ProjectId::new(),
            forged.environment_id().expect("external environment"),
            forged
                .source_revision_id()
                .expect("external source revision"),
        );
        forged.aggregate_version += 1;
        forged.updated_at += Duration::milliseconds(3);
        assert!(matches!(
            repository.save(forged, preparing.aggregate_version).await,
            Err(RepositoryError::Conflict(_))
        ));

        assert_eq!(
            repository
                .find(organization_id, preparing.id)
                .await
                .expect("stored build"),
            preparing
        );
        assert_eq!(
            repository.find(OrganizationId::new(), preparing.id).await,
            Err(RepositoryError::NotFound)
        );
    }

    #[tokio::test]
    async fn cancellation_is_atomic_and_replays_only_the_same_idempotent_request() {
        let repository = InMemoryBuildRunRepository::new();
        let organization_id = OrganizationId::new();
        let source_revision_id = SourceRevisionId::new();
        let requested_at = Utc::now();
        repository
            .add_source_revision(
                organization_id,
                ProjectId::new(),
                EnvironmentId::new(),
                source_revision_id,
                requested_at,
            )
            .await;
        let queued = repository
            .reserve_pending(1)
            .await
            .expect("reserve build")
            .pop()
            .expect("queued build");
        let mut cancelling = queued.clone();
        cancelling
            .request_cancellation(requested_at + Duration::milliseconds(1))
            .expect("request cancellation");
        let idempotency = IdempotencyRequest::new(
            format!("build-runs/{}/cancellation", queued.id),
            "cancel-once",
            queued.id.to_string().as_bytes(),
        )
        .expect("idempotency");
        let request = RequestBuildCancellationBundle {
            build_run: cancelling.clone(),
            expected_version: queued.aggregate_version,
            idempotency: idempotency.clone(),
        };

        let accepted = repository
            .request_cancellation(request.clone())
            .await
            .expect("accept cancellation");
        assert!(!accepted.replayed);
        assert_eq!(accepted.value, cancelling);
        let replayed = repository
            .request_cancellation(request)
            .await
            .expect("replay cancellation");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, cancelling);
        assert_eq!(
            repository
                .replay_cancellation(&idempotency)
                .await
                .expect("load replay"),
            Some(cancelling.clone())
        );

        let conflicting =
            IdempotencyRequest::new(idempotency.scope, idempotency.key, b"different input")
                .expect("conflicting idempotency");
        assert_eq!(
            repository.replay_cancellation(&conflicting).await,
            Err(RepositoryError::IdempotencyConflict)
        );
        assert_eq!(
            repository
                .find(organization_id, queued.id)
                .await
                .expect("stored cancellation"),
            cancelling
        );
    }

    #[tokio::test]
    async fn retry_creates_one_new_attempt_and_replays_idempotently() {
        let repository = InMemoryBuildRunRepository::new();
        let organization_id = OrganizationId::new();
        let source_revision_id = SourceRevisionId::new();
        let requested_at = Utc::now();
        repository
            .add_source_revision(
                organization_id,
                ProjectId::new(),
                EnvironmentId::new(),
                source_revision_id,
                requested_at,
            )
            .await;
        let queued = repository
            .reserve_pending(1)
            .await
            .expect("reserve build")
            .pop()
            .expect("queued build");
        let mut failed = queued.clone();
        failed
            .record_failure(
                "builder timed out".into(),
                requested_at + Duration::milliseconds(1),
            )
            .expect("record failure");
        let failed = repository
            .save(failed, queued.aggregate_version)
            .await
            .expect("save failure");
        let mut completed = failed.clone();
        completed
            .complete(requested_at + Duration::milliseconds(2))
            .expect("complete failure");
        assert!(matches!(
            repository
                .save(completed.clone(), failed.aggregate_version)
                .await,
            Err(RepositoryError::Conflict(_))
        ));
        let completed = repository
            .finalize(completed, failed.aggregate_version)
            .await
            .expect("finalize terminal failure");
        let retry = BuildRun::retry(&completed, requested_at + Duration::milliseconds(3))
            .expect("retry build");
        let idempotency = IdempotencyRequest::new(
            format!("build-runs/{}/retry", completed.id),
            "retry-once",
            completed.id.to_string().as_bytes(),
        )
        .expect("idempotency");
        let request = RequestBuildRetryBundle {
            retry: retry.clone(),
            expected_previous_version: completed.aggregate_version,
            idempotency: idempotency.clone(),
        };

        let accepted = repository
            .request_retry(request.clone())
            .await
            .expect("accept retry");
        assert!(!accepted.replayed);
        assert_eq!(accepted.value, retry);
        let replayed = repository
            .request_retry(request)
            .await
            .expect("replay retry");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, retry);
        assert_eq!(
            repository
                .replay_retry(&idempotency)
                .await
                .expect("load replay"),
            Some(retry.clone())
        );
        assert_eq!(
            repository
                .find_by_source_revision(organization_id, source_revision_id)
                .await
                .expect("find latest attempt"),
            Some(retry.clone())
        );
        assert_eq!(
            repository
                .list(
                    organization_id,
                    retry.project_id().expect("external project"),
                    retry.environment_id().expect("external environment"),
                    10,
                )
                .await
                .expect("list attempts")
                .len(),
            2
        );

        let another = RequestBuildRetryBundle {
            retry,
            expected_previous_version: completed.aggregate_version,
            idempotency: IdempotencyRequest::new(
                format!("build-runs/{}/retry", completed.id),
                "retry-again",
                completed.id.to_string().as_bytes(),
            )
            .expect("second idempotency"),
        };
        assert!(matches!(
            repository.request_retry(another).await,
            Err(RepositoryError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn hosted_success_finalizes_once_and_replays_one_owner_outcome() {
        let repository = InMemoryBuildRunRepository::new();
        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        let requested_at = Utc::now();
        repository
            .add_asset_release(organization_id, asset_id, asset_release_id, requested_at)
            .await;
        let queued = repository
            .reserve_pending(1)
            .await
            .expect("reserve hosted build")
            .pop()
            .expect("queued hosted build");
        let ready = hosted_build_ready_for_completion(
            organization_id,
            asset_id,
            asset_release_id,
            requested_at,
        );
        assert_eq!(ready.id, queued.id);
        repository
            .state
            .write()
            .await
            .builds
            .insert((organization_id, ready.id), ready.clone());

        let expected = ready.aggregate_version;
        let mut succeeded = ready;
        succeeded
            .complete(succeeded.updated_at + Duration::milliseconds(1))
            .expect("complete hosted build");
        assert!(matches!(
            repository.save(succeeded.clone(), expected).await,
            Err(RepositoryError::Conflict(_))
        ));
        let finalized = repository
            .finalize(succeeded.clone(), expected)
            .await
            .expect("finalize hosted build");
        assert_eq!(finalized, succeeded);
        let events = repository.outbox_events().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_key, HOSTED_BUILD_OUTCOME_EVENT_KEY);
        let outcome: HostedBuildOutcome =
            serde_json::from_value(events[0].payload.clone()).expect("hosted outcome");
        assert_eq!(outcome.asset_release_id(), asset_release_id);
        assert_eq!(outcome.build_run_id(), succeeded.id);

        let replayed = repository
            .finalize(succeeded.clone(), succeeded.aggregate_version)
            .await
            .expect("replay hosted finalization");
        assert_eq!(replayed, succeeded);
        assert_eq!(repository.outbox_events().await.len(), 1);
    }
}
