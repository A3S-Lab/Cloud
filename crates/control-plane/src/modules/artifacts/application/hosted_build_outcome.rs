use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus, BuildSubject};
use crate::modules::artifacts::published::{
    HostedBuildOutcome, ValidatedHostedBuildOutcomeProjection, HOSTED_BUILD_OUTCOME_EVENT_KEY,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use uuid::Uuid;

/// Project the only Artifacts fact that can authorize a hosted release
/// publication. Non-hosted or non-successful builds intentionally publish no
/// outcome.
pub(crate) fn project_hosted_build_outcome(
    build: &BuildRun,
) -> Result<Option<HostedBuildOutcome>, String> {
    build.validate()?;
    let BuildSubject::AssetRelease {
        asset_id,
        asset_release_id,
    } = build.subject
    else {
        return Ok(None);
    };
    if build.status != BuildRunStatus::Succeeded {
        return Ok(None);
    }
    let evidence = build
        .evidence
        .as_deref()
        .ok_or_else(|| "successful hosted BuildRun omitted verified evidence".to_owned())?;
    let artifact = build
        .published_artifact
        .as_ref()
        .ok_or_else(|| "successful hosted BuildRun omitted its published artifact".to_owned())?;
    let finished_at = build
        .finished_at
        .ok_or_else(|| "successful hosted BuildRun omitted its finish time".to_owned())?;
    let manifest_digest = evidence
        .manifest_digest
        .as_deref()
        .ok_or_else(|| "successful hosted BuildRun omitted its Asset manifest digest".to_owned())?;
    HostedBuildOutcome::from_validated_build(ValidatedHostedBuildOutcomeProjection {
        organization_id: build.organization_id,
        asset_id,
        asset_release_id,
        build_run_id: build.id,
        build_run_version: build.aggregate_version,
        attempt: build.attempt,
        operation_id: build.operation_id,
        commit_sha: evidence.commit_sha.clone(),
        manifest_digest: manifest_digest.into(),
        artifact_digest: artifact.digest.clone(),
        artifact_media_type: artifact.media_type.clone(),
        artifact_size_bytes: artifact.size_bytes,
        provenance_digest: evidence.provenance_digest.clone(),
        finished_at,
    })
    .map(Some)
}

pub(crate) fn hosted_build_outcome_event(
    build: &BuildRun,
) -> Result<Option<DomainEventEnvelope>, String> {
    let Some(outcome) = project_hosted_build_outcome(build)? else {
        return Ok(None);
    };
    Ok(Some(DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: HOSTED_BUILD_OUTCOME_EVENT_KEY.into(),
        schema_version: 1,
        organization_id: outcome.organization_id().as_uuid(),
        aggregate_id: outcome.build_run_id().as_uuid(),
        aggregate_version: outcome.build_run_version(),
        occurred_at: outcome.finished_at(),
        correlation_id: outcome.operation_id().as_uuid(),
        causation_id: None,
        payload: serde_json::to_value(outcome)
            .map_err(|error| format!("serialize hosted build outcome: {error}"))?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::test_support::hosted_build_ready_for_completion;
    use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
    use chrono::Duration;

    #[test]
    fn successful_hosted_build_projects_one_bounded_owner_fact() {
        let mut build = hosted_build_ready_for_completion(
            OrganizationId::new(),
            AssetId::new(),
            AssetReleaseId::new(),
            chrono::Utc::now(),
        );
        build
            .complete(build.updated_at + Duration::milliseconds(1))
            .expect("complete hosted build");

        let event = hosted_build_outcome_event(&build)
            .expect("project outcome")
            .expect("hosted outcome");
        assert_eq!(event.event_key, HOSTED_BUILD_OUTCOME_EVENT_KEY);
        assert_eq!(event.aggregate_id, build.id.as_uuid());
        assert_eq!(event.aggregate_version, build.aggregate_version);
        assert_eq!(event.correlation_id, build.operation_id.as_uuid());
        let serialized = serde_json::to_string(&event.payload).expect("outcome JSON");
        for forbidden in ["registry", "uri", "nodeId", "commandId", "credential"] {
            assert!(!serialized.contains(forbidden));
        }
        let restored: HostedBuildOutcome =
            serde_json::from_value(event.payload).expect("restore outcome");
        restored.validate().expect("valid outcome");
    }

    #[test]
    fn failed_hosted_build_publishes_no_success_outcome() {
        let mut build = hosted_build_ready_for_completion(
            OrganizationId::new(),
            AssetId::new(),
            AssetReleaseId::new(),
            chrono::Utc::now(),
        );
        build
            .record_failure(
                "provider failed".into(),
                build.updated_at + Duration::milliseconds(1),
            )
            .expect("fail hosted build");
        build
            .complete(build.updated_at + Duration::milliseconds(1))
            .expect("complete failed build");
        assert_eq!(
            hosted_build_outcome_event(&build).expect("projection"),
            None
        );
    }
}
