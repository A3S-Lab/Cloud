use crate::modules::artifacts::published::HostedBuildOutcome;
use crate::modules::assets::domain::{
    AssetReleasePublished, AssetReleaseState, AssetState, IAssetRepository,
    TransitionAssetReleaseWrite,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use std::sync::Arc;
use uuid::Uuid;

/// Assets-owned application policy for consuming one committed hosted build
/// fact. Artifacts cannot call this service or write Asset release state.
pub(in crate::modules::assets) struct HostedBuildOutcomeApplicationService {
    assets: Arc<dyn IAssetRepository>,
}

impl HostedBuildOutcomeApplicationService {
    pub(in crate::modules::assets) fn new(assets: Arc<dyn IAssetRepository>) -> Self {
        Self { assets }
    }

    pub(in crate::modules::assets) async fn project(
        &self,
        outcome: HostedBuildOutcome,
        source_event_id: Uuid,
        correlation_id: Uuid,
    ) -> Result<(), RepositoryError> {
        outcome.validate().map_err(invalid_outcome)?;
        if source_event_id.is_nil()
            || correlation_id.is_nil()
            || correlation_id != outcome.operation_id().as_uuid()
        {
            return Err(invalid_outcome(
                "hosted build outcome event identity is invalid".into(),
            ));
        }
        let asset = self
            .assets
            .find_asset(outcome.organization_id(), outcome.asset_id())
            .await?
            .ok_or(RepositoryError::NotFound)?;
        let mut release = self
            .assets
            .find_release(
                outcome.organization_id(),
                outcome.asset_id(),
                outcome.asset_release_id(),
            )
            .await?
            .ok_or(RepositoryError::NotFound)?;

        match release.state {
            AssetReleaseState::Draft if asset.state == AssetState::Archived => {
                // Archival is terminal. The committed build fact remains valid,
                // but it cannot reopen the Asset or create a second failure
                // state machine in Artifacts.
                release
                    .validate_hosted_build_outcome(&asset, &outcome)
                    .map_err(invalid_outcome)?;
                return Ok(());
            }
            AssetReleaseState::Published | AssetReleaseState::Yanked => {
                return release
                    .validate_hosted_build_publication(&asset, &outcome)
                    .map_err(invalid_outcome);
            }
            AssetReleaseState::Draft => {}
        }

        let expected_aggregate_version = release.aggregate_version;
        release
            .publish_from_hosted_build(&asset, &outcome)
            .map_err(invalid_outcome)?;
        let mut event =
            AssetReleasePublished::envelope(&release, correlation_id).map_err(invalid_outcome)?;
        event.causation_id = Some(source_event_id);
        let canonical = serde_json::to_vec(&outcome).map_err(|error| {
            RepositoryError::Storage(format!("serialize hosted build publication: {error}"))
        })?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/assets/{}/releases/{}/hosted-build-publication",
                outcome.organization_id(),
                outcome.asset_id(),
                outcome.asset_release_id()
            ),
            "hosted-build-outcome",
            &canonical,
        )
        .map_err(invalid_outcome)?;
        self.assets
            .transition_release(TransitionAssetReleaseWrite {
                release,
                expected_aggregate_version,
                event,
                idempotency,
            })
            .await?;
        Ok(())
    }
}

fn invalid_outcome(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("hosted build outcome is invalid: {error}"))
}

#[cfg(test)]
mod tests;
