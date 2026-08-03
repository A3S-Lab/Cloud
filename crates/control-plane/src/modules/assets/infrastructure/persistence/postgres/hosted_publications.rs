use super::queries::{lock_asset, lock_release};
use super::writes::persist_release_transition;
use crate::infrastructure::{store_outbox, PostgresPersistenceError};
use crate::modules::artifacts::domain::BuildRun;
use crate::modules::assets::domain::{
    AssetRelease, AssetReleasePublished, AssetReleaseState, AssetState,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::PostgresTransaction;

pub(crate) enum HostedReleasePlan {
    Publish(Box<HostedReleaseWrite>),
    Replay,
    Reject(String),
}

pub(crate) struct HostedReleaseWrite {
    release: AssetRelease,
    expected_aggregate_version: u64,
    event: DomainEventEnvelope,
}

pub(crate) async fn plan_hosted_release(
    transaction: &PostgresTransaction,
    build: &BuildRun,
) -> Result<HostedReleasePlan, PostgresPersistenceError> {
    let asset_id = build.asset_id().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "hosted BuildRun finalization omitted its Asset identity".into(),
        )
    })?;
    let asset_release_id = build.asset_release_id().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "hosted BuildRun finalization omitted its release identity".into(),
        )
    })?;
    let asset = lock_asset(transaction, build.organization_id, asset_id).await?;
    let release = lock_release(
        transaction,
        build.organization_id,
        asset_id,
        asset_release_id,
    )
    .await?;

    match release.state {
        AssetReleaseState::Draft if asset.state == AssetState::Archived => {
            Ok(HostedReleasePlan::Reject(
                "hosted Asset was archived before release publication".into(),
            ))
        }
        AssetReleaseState::Draft => {
            let expected_aggregate_version = release.aggregate_version;
            let mut published = release;
            published
                .publish_from_build(&asset, build)
                .map_err(invalid_publication)?;
            let event = AssetReleasePublished::envelope_from_build(&published, build)
                .map_err(invalid_publication)?;
            Ok(HostedReleasePlan::Publish(Box::new(HostedReleaseWrite {
                release: published,
                expected_aggregate_version,
                event,
            })))
        }
        AssetReleaseState::Published | AssetReleaseState::Yanked => {
            release
                .validate_build_publication(&asset, build)
                .map_err(invalid_publication)?;
            Ok(HostedReleasePlan::Replay)
        }
    }
}

pub(crate) async fn apply_hosted_release(
    transaction: &PostgresTransaction,
    plan: HostedReleasePlan,
) -> Result<(), PostgresPersistenceError> {
    match plan {
        HostedReleasePlan::Publish(write) => {
            persist_release_transition(
                transaction,
                &write.release,
                write.expected_aggregate_version,
            )
            .await?;
            store_outbox(transaction, &write.event).await?;
            Ok(())
        }
        HostedReleasePlan::Replay => Ok(()),
        HostedReleasePlan::Reject(reason) => Err(PostgresPersistenceError::Invariant(format!(
            "rejected hosted release plan was applied: {reason}"
        ))),
    }
}

pub(crate) async fn verify_hosted_release_unpublished(
    transaction: &PostgresTransaction,
    build: &BuildRun,
) -> Result<(), PostgresPersistenceError> {
    let asset_id = build.asset_id().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "hosted BuildRun finalization omitted its Asset identity".into(),
        )
    })?;
    let asset_release_id = build.asset_release_id().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "hosted BuildRun finalization omitted its release identity".into(),
        )
    })?;
    let release = lock_release(
        transaction,
        build.organization_id,
        asset_id,
        asset_release_id,
    )
    .await?;
    if release.state != AssetReleaseState::Draft
        || release.artifact.is_some()
        || release.provenance.is_some()
    {
        return Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict(
                "failed or cancelled hosted BuildRun cannot own a published release".into(),
            ),
        ));
    }
    Ok(())
}

fn invalid_publication(error: String) -> PostgresPersistenceError {
    PostgresPersistenceError::Repository(RepositoryError::Conflict(format!(
        "hosted Asset release publication is invalid: {error}"
    )))
}
