use crate::modules::artifacts::domain::repositories::validate_build_run_transition;
use crate::modules::artifacts::domain::{
    BuildRun, IBuildRunRepository, RequestBuildCancellationBundle,
};
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    RepositoryError, SourceRevisionId,
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
    started_operations: BTreeSet<BuildRunId>,
    cancellation_idempotency: BTreeMap<(String, String), (String, BuildRun)>,
}

#[derive(Clone)]
struct PendingRevision {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    accepted_at: DateTime<Utc>,
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

    pub async fn mark_operation_started(&self, build_run_id: BuildRunId) {
        self.state
            .write()
            .await
            .started_operations
            .insert(build_run_id);
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
            .map(|build| build.source_revision_id)
            .collect::<BTreeSet<_>>();
        let mut revisions = state
            .revisions
            .iter()
            .filter(|(id, _)| !existing_sources.contains(id))
            .map(|(id, revision)| (*id, revision.clone()))
            .collect::<Vec<_>>();
        revisions.sort_by_key(|(id, revision)| (revision.accepted_at, *id));
        let mut reserved = Vec::new();
        for (source_revision_id, revision) in revisions.into_iter().take(limit.max(1)) {
            let build = BuildRun::reserve(
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                source_revision_id,
                reserved_at.max(revision.accepted_at),
            );
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
            .find(|build| {
                build.organization_id == organization_id
                    && build.source_revision_id == source_revision_id
            })
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
                    && build.project_id == project_id
                    && build.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        builds.sort_by_key(|build| std::cmp::Reverse((build.requested_at, build.id)));
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
        forged.project_id = ProjectId::new();
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
}
