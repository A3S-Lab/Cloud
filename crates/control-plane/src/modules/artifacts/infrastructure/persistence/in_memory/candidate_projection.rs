use super::*;

fn candidate_key(candidate: &BuildCandidate) -> (OrganizationId, u8, uuid::Uuid) {
    match candidate.subject() {
        BuildSubject::ExternalSourceRevision {
            source_revision_id, ..
        } => (candidate.organization_id(), 0, source_revision_id.as_uuid()),
        BuildSubject::AssetRelease {
            asset_release_id, ..
        } => (candidate.organization_id(), 1, asset_release_id.as_uuid()),
    }
}

pub(super) fn candidate_sort_key(
    candidate: &BuildCandidate,
) -> (DateTime<Utc>, u8, uuid::Uuid, uuid::Uuid) {
    let (organization_id, kind, subject_id) = candidate_key(candidate);
    (
        candidate.requested_at(),
        kind,
        organization_id.as_uuid(),
        subject_id,
    )
}

fn project_candidate_into(
    state: &mut State,
    candidate: BuildCandidate,
) -> Result<(), RepositoryError> {
    candidate.validate().map_err(RepositoryError::Storage)?;
    let key = candidate_key(&candidate);
    match state.candidates.get(&key) {
        Some(existing) if existing == &candidate => Ok(()),
        Some(_) => Err(RepositoryError::Conflict(
            "build candidate fact conflicts with its existing projection".into(),
        )),
        None => {
            state.candidates.insert(key, candidate);
            Ok(())
        }
    }
}

fn latest_preview_receipt(
    state: &State,
    organization_id: OrganizationId,
    preview_id: PullRequestPreviewId,
) -> Option<&PreviewBuildLifecycleProjectionReceipt> {
    state
        .preview_lifecycle_receipts
        .iter()
        .filter(
            |((candidate_organization_id, candidate_preview_id, _), _)| {
                *candidate_organization_id == organization_id && *candidate_preview_id == preview_id
            },
        )
        .max_by_key(|((_, _, version), _)| *version)
        .map(|(_, receipt)| receipt)
}

pub(super) fn latest_build_for_candidate<'a>(
    state: &'a State,
    candidate: &BuildCandidate,
) -> Option<&'a BuildRun> {
    state
        .builds
        .values()
        .filter(|build| {
            build.organization_id == candidate.organization_id()
                && build.subject == candidate.subject()
        })
        .max_by_key(|build| build.attempt)
}

pub(super) fn preview_candidate_is_current(state: &State, candidate: &BuildCandidate) -> bool {
    let Some(preview_id) = candidate.preview_id() else {
        return true;
    };
    let Some(receipt) = latest_preview_receipt(state, candidate.organization_id(), preview_id)
    else {
        return false;
    };
    let BuildSubject::ExternalSourceRevision {
        project_id,
        environment_id,
        source_revision_id,
    } = candidate.subject()
    else {
        return false;
    };
    receipt.outcome == PreviewBuildLifecycleProjectionOutcome::Applied
        && receipt.state == PreviewBuildLifecycleState::Active
        && receipt.source_revision_id() == Some(source_revision_id)
        && receipt.project_id == project_id
        && receipt.preview_environment_id == environment_id
}

pub(super) fn preview_retry_requested_at(
    state: &State,
    candidate: &BuildCandidate,
    previous: &BuildRun,
) -> Option<DateTime<Utc>> {
    let preview_id = candidate.preview_id()?;
    let head = latest_preview_receipt(state, candidate.organization_id(), preview_id)?;
    if !matches!(
        previous.status,
        crate::modules::artifacts::domain::BuildRunStatus::Failed
            | crate::modules::artifacts::domain::BuildRunStatus::Cancelled
    ) {
        return None;
    }
    state
        .preview_lifecycle_receipts
        .values()
        .filter(|receipt| {
            receipt.organization_id == candidate.organization_id()
                && receipt.preview_id == preview_id
                && receipt.preview_aggregate_version < head.preview_aggregate_version
                && receipt.retirement.source_revision_id()
                    == candidate.subject().source_revision_id()
                && receipt.retirement.build_run_id() == Some(previous.id)
                && matches!(
                    receipt.retirement,
                    PreviewBuildRetirement::CancellationRequested { .. }
                        | PreviewBuildRetirement::TerminalObserved { .. }
                )
        })
        .max_by_key(|receipt| receipt.preview_aggregate_version)
        .map(|_| std::cmp::max(head.fact_occurred_at, previous.updated_at))
}

#[async_trait]
impl IBuildCandidateProjectionPort for InMemoryBuildRunRepository {
    async fn project_candidate(&self, candidate: BuildCandidate) -> Result<(), RepositoryError> {
        let mut state = self.state.write().await;
        project_candidate_into(&mut state, candidate)
    }
}

#[async_trait]
impl IPreviewBuildLifecycleProjectionPort for InMemoryBuildRunRepository {
    async fn project_preview_build_lifecycle(
        &self,
        input: ProjectPreviewBuildLifecycle,
    ) -> Result<IdempotentWrite<PreviewBuildLifecycleProjectionReceipt>, RepositoryError> {
        input.validate().map_err(|error| {
            RepositoryError::Storage(format!(
                "invalid Preview build lifecycle projection: {error}"
            ))
        })?;
        let mut state = self.state.write().await;
        let receipt_key = (
            input.organization_id,
            input.preview_id,
            input.preview_aggregate_version,
        );
        if let Some(existing) = state.preview_lifecycle_receipts.get(&receipt_key) {
            if !existing.matches_input(&input) {
                return Err(RepositoryError::Conflict(
                    "Preview aggregate version changed its Artifacts build projection".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        if state
            .preview_lifecycle_events
            .get(&input.lifecycle_event_id)
            .is_some_and(|existing_key| existing_key != &receipt_key)
        {
            return Err(RepositoryError::Conflict(
                "Preview lifecycle event identity was reused for another Artifacts projection"
                    .into(),
            ));
        }

        let latest =
            latest_preview_receipt(&state, input.organization_id, input.preview_id).cloned();
        let decision = input.decide(latest.as_ref()).map_err(|error| {
            RepositoryError::Conflict(format!(
                "Preview build projection authority is invalid: {error}"
            ))
        })?;
        let mut next = state.clone();
        let retirement = if decision.outcome == PreviewBuildLifecycleProjectionOutcome::IgnoredStale
        {
            PreviewBuildRetirement::NotRequired
        } else if let Some(source_revision_id) = decision.retired_source_revision_id {
            let candidate = next
                .candidates
                .get(&(input.organization_id, 0, source_revision_id.as_uuid()))
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "retired Preview SourceRevision has no Artifacts candidate".into(),
                    )
                })?;
            if candidate.preview_id() != Some(input.preview_id) {
                return Err(RepositoryError::Conflict(
                    "retired Preview SourceRevision belongs to another Preview".into(),
                ));
            }
            match latest_build_for_candidate(&next, candidate).cloned() {
                None => PreviewBuildRetirement::PendingSuppressed { source_revision_id },
                Some(build) if build.status.is_terminal() => {
                    PreviewBuildRetirement::TerminalObserved {
                        source_revision_id,
                        build_run_id: build.id,
                    }
                }
                Some(build) if build.cancellation_requested_at.is_some() => {
                    PreviewBuildRetirement::CancellationRequested {
                        source_revision_id,
                        build_run_id: build.id,
                    }
                }
                Some(build) => {
                    let mut cancellation = build.clone();
                    cancellation
                        .request_cancellation(std::cmp::max(
                            input.fact_occurred_at,
                            build.updated_at,
                        ))
                        .map_err(RepositoryError::Conflict)?;
                    validate_build_run_transition(&build, &cancellation, build.aggregate_version)?;
                    next.builds.insert(
                        (cancellation.organization_id, cancellation.id),
                        cancellation,
                    );
                    PreviewBuildRetirement::CancellationRequested {
                        source_revision_id,
                        build_run_id: build.id,
                    }
                }
            }
        } else {
            PreviewBuildRetirement::NotRequired
        };
        if let Some(candidate) = decision.candidate {
            project_candidate_into(&mut next, candidate)?;
        }
        let receipt = PreviewBuildLifecycleProjectionReceipt::from_input(
            &input,
            decision.outcome,
            retirement,
        )
        .map_err(RepositoryError::Storage)?;
        next.preview_lifecycle_events
            .insert(input.lifecycle_event_id, receipt_key);
        next.preview_lifecycle_receipts
            .insert(receipt_key, receipt.clone());
        *state = next;
        Ok(IdempotentWrite {
            value: receipt,
            replayed: false,
        })
    }
}
