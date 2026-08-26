use super::candidate_projection::{
    candidate_sort_key, latest_build_for_candidate, preview_candidate_is_current,
    preview_retry_requested_at,
};
use super::*;

#[async_trait]
impl IBuildRunRepository for InMemoryBuildRunRepository {
    async fn reserve_pending(&self, limit: usize) -> Result<Vec<BuildRun>, RepositoryError> {
        let mut state = self.state.write().await;
        let mut pending = state
            .candidates
            .values()
            .filter(|candidate| preview_candidate_is_current(&state, candidate))
            .cloned()
            .collect::<Vec<_>>();
        pending.sort_by_key(candidate_sort_key);
        let mut reserved = Vec::new();
        for candidate in pending {
            if reserved.len() >= limit.max(1) {
                break;
            }
            let previous = latest_build_for_candidate(&state, &candidate).cloned();
            let build = match previous {
                Some(previous) => {
                    let Some(requested_at) =
                        preview_retry_requested_at(&state, &candidate, &previous)
                    else {
                        continue;
                    };
                    BuildRun::retry(&previous, requested_at).map_err(RepositoryError::Storage)?
                }
                None => match candidate.subject() {
                    BuildSubject::ExternalSourceRevision {
                        project_id,
                        environment_id,
                        source_revision_id,
                    } => BuildRun::reserve(
                        candidate.organization_id(),
                        project_id,
                        environment_id,
                        source_revision_id,
                        candidate.requested_at(),
                    ),
                    BuildSubject::AssetRelease {
                        asset_id,
                        asset_release_id,
                    } => BuildRun::reserve_asset_release(
                        candidate.organization_id(),
                        asset_id,
                        asset_release_id,
                        candidate.requested_at(),
                    ),
                },
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
    ) -> Result<BuildRun, RepositoryError> {
        let build_run = BuildRun::restore(build_run).map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (build_run.organization_id, build_run.id);
        let existing = state
            .builds
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let mode = validate_build_run_finalization(&existing, &build_run, expected_version)?;
        let outcome = if mode == BuildRunFinalizationMode::Transition {
            hosted_build_outcome_event(&build_run).map_err(RepositoryError::Storage)?
        } else {
            None
        };
        state.builds.insert(key, build_run.clone());
        if let Some(outcome) = outcome {
            state.outbox.push(outcome);
        }
        Ok(build_run)
    }
}
