use crate::modules::artifacts::domain::repositories::{
    validate_build_run_finalization, validate_build_run_retry, validate_build_run_transition,
};
use crate::modules::artifacts::domain::{
    BuildRun, BuildRunFinalization, BuildRunStatus, IBuildRunRepository,
    RequestBuildCancellationBundle, RequestBuildRetryBundle,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryBuildRunRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    builds: BTreeMap<(OrganizationId, BuildRunId), BuildRun>,
    revisions: BTreeMap<SourceRevisionId, PendingRevision>,
    asset_releases: BTreeMap<(OrganizationId, AssetReleaseId), PendingAssetRelease>,
    started_operations: BTreeSet<BuildRunId>,
    cancellation_idempotency: BTreeMap<(String, String), (String, BuildRun)>,
    retry_idempotency: BTreeMap<(String, String), (String, BuildRun)>,
    hosted_publications: BTreeMap<(OrganizationId, AssetReleaseId), (BuildRunId, String)>,
}

#[derive(Clone)]
struct PendingRevision {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    accepted_at: DateTime<Utc>,
}

#[derive(Clone)]
struct PendingAssetRelease {
    organization_id: OrganizationId,
    asset_id: AssetId,
    drafted_at: DateTime<Utc>,
}

#[derive(Clone)]
enum PendingBuild {
    External(SourceRevisionId, PendingRevision),
    AssetRelease(AssetReleaseId, PendingAssetRelease),
}

impl PendingBuild {
    fn sort_key(&self) -> (DateTime<Utc>, u8, uuid::Uuid) {
        match self {
            Self::External(source_revision_id, revision) => {
                (revision.accepted_at, 0, source_revision_id.as_uuid())
            }
            Self::AssetRelease(asset_release_id, release) => {
                (release.drafted_at, 1, asset_release_id.as_uuid())
            }
        }
    }
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
        self.state.write().await.revisions.insert(
            source_revision_id,
            PendingRevision {
                organization_id,
                project_id,
                environment_id,
                accepted_at,
            },
        );
    }

    pub async fn add_asset_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        drafted_at: DateTime<Utc>,
    ) {
        self.state.write().await.asset_releases.insert(
            (organization_id, asset_release_id),
            PendingAssetRelease {
                organization_id,
                asset_id,
                drafted_at,
            },
        );
    }

    pub async fn mark_operation_started(&self, build_run_id: BuildRunId) {
        self.state
            .write()
            .await
            .started_operations
            .insert(build_run_id);
    }

    pub async fn hosted_release_publication(
        &self,
        organization_id: OrganizationId,
        asset_release_id: AssetReleaseId,
    ) -> Option<(BuildRunId, String)> {
        self.state
            .read()
            .await
            .hosted_publications
            .get(&(organization_id, asset_release_id))
            .cloned()
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

#[async_trait]
impl IBuildRunRepository for InMemoryBuildRunRepository {
    async fn reserve_pending(
        &self,
        limit: usize,
        reserved_at: DateTime<Utc>,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        let mut state = self.state.write().await;
        let existing_sources = state
            .builds
            .values()
            .filter_map(BuildRun::source_revision_id)
            .collect::<BTreeSet<_>>();
        let existing_releases = state
            .builds
            .values()
            .filter_map(|build| {
                build
                    .asset_release_id()
                    .map(|asset_release_id| (build.organization_id, asset_release_id))
            })
            .collect::<BTreeSet<_>>();
        let mut pending = state
            .revisions
            .iter()
            .filter(|(id, _)| !existing_sources.contains(id))
            .map(|(id, revision)| PendingBuild::External(*id, revision.clone()))
            .chain(
                state
                    .asset_releases
                    .iter()
                    .filter(|(id, _)| !existing_releases.contains(id))
                    .map(|((_, id), release)| PendingBuild::AssetRelease(*id, release.clone())),
            )
            .collect::<Vec<_>>();
        pending.sort_by_key(PendingBuild::sort_key);
        let mut reserved = Vec::new();
        for candidate in pending.into_iter().take(limit.max(1)) {
            let build = match candidate {
                PendingBuild::External(source_revision_id, revision) => BuildRun::reserve(
                    revision.organization_id,
                    revision.project_id,
                    revision.environment_id,
                    source_revision_id,
                    reserved_at.max(revision.accepted_at),
                ),
                PendingBuild::AssetRelease(asset_release_id, release) => {
                    BuildRun::reserve_asset_release(
                        release.organization_id,
                        release.asset_id,
                        asset_release_id,
                        reserved_at.max(release.drafted_at),
                    )
                }
            };
            state
                .builds
                .insert((build.organization_id, build.id), build.clone());
            reserved.push(build);
        }
        Ok(reserved)
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        let state = self.state.read().await;
        let mut builds = state
            .builds
            .values()
            .filter(|build| !state.started_operations.contains(&build.id))
            .cloned()
            .collect::<Vec<_>>();
        builds.sort_by_key(|build| (build.requested_at, build.id));
        builds.truncate(limit.max(1));
        Ok(builds)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<BuildRun, RepositoryError> {
        self.state
            .read()
            .await
            .builds
            .get(&(organization_id, build_run_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn find_by_source_revision(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .builds
            .values()
            .filter(|build| {
                build.organization_id == organization_id
                    && build.source_revision_id() == Some(source_revision_id)
            })
            .max_by_key(|build| build.attempt)
            .cloned())
    }

    async fn find_by_asset_release(
        &self,
        organization_id: OrganizationId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .builds
            .values()
            .filter(|build| {
                build.organization_id == organization_id
                    && build.asset_release_id() == Some(asset_release_id)
            })
            .max_by_key(|build| build.attempt)
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        let mut builds = self
            .state
            .read()
            .await
            .builds
            .values()
            .filter(|build| {
                build.organization_id == organization_id
                    && build.project_id() == Some(project_id)
                    && build.environment_id() == Some(environment_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        builds
            .sort_by_key(|build| std::cmp::Reverse((build.requested_at, build.attempt, build.id)));
        builds.truncate(limit.max(1));
        Ok(builds)
    }

    async fn request_cancellation(
        &self,
        request: RequestBuildCancellationBundle,
    ) -> Result<IdempotentWrite<BuildRun>, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (
            request.idempotency.scope.clone(),
            request.idempotency.key.clone(),
        );
        if let Some((digest, build_run)) = state.cancellation_idempotency.get(&key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: build_run.clone(),
                replayed: true,
            });
        }
        let storage_key = (request.build_run.organization_id, request.build_run.id);
        let current = state
            .builds
            .get(&storage_key)
            .ok_or(RepositoryError::NotFound)?;
        validate_build_run_transition(current, &request.build_run, request.expected_version)?;
        state.builds.insert(storage_key, request.build_run.clone());
        state.cancellation_idempotency.insert(
            key,
            (
                request.idempotency.request_digest,
                request.build_run.clone(),
            ),
        );
        Ok(IdempotentWrite {
            value: request.build_run,
            replayed: false,
        })
    }

    async fn replay_cancellation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        let state = self.state.read().await;
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        let Some((digest, build_run)) = state.cancellation_idempotency.get(&key) else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(build_run.clone()))
    }

    async fn request_retry(
        &self,
        request: RequestBuildRetryBundle,
    ) -> Result<IdempotentWrite<BuildRun>, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (
            request.idempotency.scope.clone(),
            request.idempotency.key.clone(),
        );
        if let Some((digest, build_run)) = state.retry_idempotency.get(&key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: build_run.clone(),
                replayed: true,
            });
        }
        let previous_id = request
            .retry
            .retry_of_build_run_id
            .ok_or_else(|| RepositoryError::Conflict("build retry has no parent".into()))?;
        let previous = state
            .builds
            .get(&(request.retry.organization_id, previous_id))
            .ok_or(RepositoryError::NotFound)?;
        validate_build_run_retry(previous, &request.retry, request.expected_previous_version)?;
        if state.builds.values().any(|build| {
            build.organization_id == request.retry.organization_id
                && build.retry_of_build_run_id == Some(previous_id)
        }) {
            return Err(RepositoryError::Conflict(
                "build run already has a retry attempt".into(),
            ));
        }
        let storage_key = (request.retry.organization_id, request.retry.id);
        if state.builds.contains_key(&storage_key) {
            return Err(RepositoryError::Conflict(
                "build retry identity already exists".into(),
            ));
        }
        state.builds.insert(storage_key, request.retry.clone());
        state.retry_idempotency.insert(
            key,
            (request.idempotency.request_digest, request.retry.clone()),
        );
        Ok(IdempotentWrite {
            value: request.retry,
            replayed: false,
        })
    }

    async fn replay_retry(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        let state = self.state.read().await;
        let key = (idempotency.scope.clone(), idempotency.key.clone());
        let Some((digest, build_run)) = state.retry_idempotency.get(&key) else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(build_run.clone()))
    }

    async fn save(
        &self,
        build_run: BuildRun,
        expected_version: u64,
    ) -> Result<BuildRun, RepositoryError> {
        let build_run = BuildRun::restore(build_run).map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (build_run.organization_id, build_run.id);
        let existing = state.builds.get(&key).ok_or(RepositoryError::NotFound)?;
        validate_build_run_transition(existing, &build_run, expected_version)?;
        state.builds.insert(key, build_run.clone());
        Ok(build_run)
    }

    async fn finalize(
        &self,
        build_run: BuildRun,
        expected_version: u64,
    ) -> Result<BuildRunFinalization, RepositoryError> {
        let build_run = BuildRun::restore(build_run).map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (build_run.organization_id, build_run.id);
        let existing = state
            .builds
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_build_run_finalization(&existing, &build_run, expected_version)?;
        if let Some(asset_release_id) = build_run.asset_release_id() {
            let publication_key = (build_run.organization_id, asset_release_id);
            if build_run.status == BuildRunStatus::Succeeded {
                let provenance_digest = build_run
                    .evidence
                    .as_deref()
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "successful hosted BuildRun has no verified evidence".into(),
                        )
                    })?
                    .provenance_digest
                    .clone();
                match state.hosted_publications.get(&publication_key) {
                    Some((build_run_id, digest))
                        if *build_run_id == build_run.id && digest == &provenance_digest => {}
                    Some(_) => {
                        return Err(RepositoryError::Conflict(
                            "hosted release publication changed during replay".into(),
                        ))
                    }
                    None => {
                        state
                            .hosted_publications
                            .insert(publication_key, (build_run.id, provenance_digest));
                    }
                }
            } else if state.hosted_publications.contains_key(&publication_key) {
                return Err(RepositoryError::Conflict(
                    "failed or cancelled hosted BuildRun cannot own a published release".into(),
                ));
            }
        }
        state.builds.insert(key, build_run.clone());
        Ok(BuildRunFinalization::Completed(build_run))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::test_support::hosted_build_ready_for_completion;
    use crate::modules::artifacts::domain::BuildSubject;
    use chrono::Duration;
    use std::sync::Arc;

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

        let (left, right) = tokio::join!(
            repository.reserve_pending(1, accepted_at),
            repository.reserve_pending(1, accepted_at)
        );
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

        let (left, right) = tokio::join!(
            repository.reserve_pending(1, drafted_at),
            repository.reserve_pending(1, drafted_at)
        );
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
            .reserve_pending(1, accepted_at)
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
            .reserve_pending(1, requested_at)
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
            .reserve_pending(1, requested_at)
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
        let BuildRunFinalization::Completed(completed) = completed else {
            panic!("external BuildRun finalization was rejected");
        };
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
    async fn hosted_success_finalizes_once_and_replays_the_same_publication_binding() {
        let repository = InMemoryBuildRunRepository::new();
        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        let requested_at = Utc::now();
        repository
            .add_asset_release(organization_id, asset_id, asset_release_id, requested_at)
            .await;
        let queued = repository
            .reserve_pending(1, requested_at)
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
        assert_eq!(
            finalized,
            BuildRunFinalization::Completed(succeeded.clone())
        );
        let evidence = succeeded.evidence.as_deref().expect("hosted evidence");
        assert_eq!(
            repository
                .hosted_release_publication(organization_id, asset_release_id)
                .await,
            Some((succeeded.id, evidence.provenance_digest.clone()))
        );

        let replayed = repository
            .finalize(succeeded.clone(), succeeded.aggregate_version)
            .await
            .expect("replay hosted finalization");
        assert_eq!(replayed, BuildRunFinalization::Completed(succeeded));
    }
}
