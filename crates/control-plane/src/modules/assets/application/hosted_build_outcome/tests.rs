use super::*;
use crate::modules::artifacts::application::project_hosted_build_outcome;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_build;
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseVersion, AssetReleaseWrite, AssetWrite,
    CreateAssetReleaseWrite, CreateAssetWrite, TransitionAssetWrite,
};
use crate::modules::assets::infrastructure::HostedBuildOutcomeProjector;
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, OrganizationId, ResourceName, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use tokio::sync::RwLock;

fn now() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_800_000_000, 0)
        .single()
        .expect("timestamp")
}

fn fixture() -> (Asset, AssetRelease, HostedBuildOutcome) {
    let asset = Asset::create(
        AssetId::new(),
        OrganizationId::new(),
        ResourceName::parse("Hosted Agent").expect("name"),
        AssetKind::Agent,
        now(),
    )
    .expect("Asset");
    let release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0").expect("version"),
        GitCommitSha::parse("a".repeat(40)).expect("commit"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("manifest"),
        now(),
    )
    .expect("release");
    let build = succeeded_hosted_build(
        asset.organization_id,
        asset.id,
        release.id,
        release.created_at,
    );
    let outcome = project_hosted_build_outcome(&build)
        .expect("projection")
        .expect("hosted outcome");
    (asset, release, outcome)
}

struct TestAssetRepository {
    state: RwLock<TestState>,
}

struct TestState {
    asset: Asset,
    release: AssetRelease,
    transitions: usize,
}

impl TestAssetRepository {
    fn new(asset: Asset, release: AssetRelease) -> Self {
        Self {
            state: RwLock::new(TestState {
                asset,
                release,
                transitions: 0,
            }),
        }
    }
}

#[async_trait]
impl IAssetRepository for TestAssetRepository {
    async fn create_asset(&self, _: CreateAssetWrite) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unsupported test write".into()))
    }

    async fn transition_asset(
        &self,
        _: TransitionAssetWrite,
    ) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unsupported test write".into()))
    }

    async fn find_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Option<Asset>, RepositoryError> {
        let state = self.state.read().await;
        Ok(
            (state.asset.organization_id == organization_id && state.asset.id == asset_id)
                .then(|| state.asset.clone()),
        )
    }

    async fn list_assets(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Asset>, RepositoryError> {
        Ok(self
            .find_asset(organization_id, self.state.read().await.asset.id)
            .await?
            .into_iter()
            .collect())
    }

    async fn create_release(
        &self,
        _: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unsupported test write".into()))
    }

    async fn transition_release(
        &self,
        bundle: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        bundle
            .validate_against(&state.release, &state.asset)
            .map_err(RepositoryError::Conflict)?;
        state.release = bundle.release.clone();
        state.transitions += 1;
        Ok(AssetReleaseWrite {
            asset: state.asset.clone(),
            release: bundle.release,
            replayed: false,
        })
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<AssetRelease>, RepositoryError> {
        let state = self.state.read().await;
        Ok((state.release.organization_id == organization_id
            && state.release.asset_id == asset_id
            && state.release.id == asset_release_id)
            .then(|| state.release.clone()))
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError> {
        Ok(self
            .find_release(
                organization_id,
                asset_id,
                self.state.read().await.release.id,
            )
            .await?
            .into_iter()
            .collect())
    }
}

#[tokio::test]
async fn owner_projection_publishes_once_and_replay_preserves_binding() {
    let (asset, release, outcome) = fixture();
    let repository = Arc::new(TestAssetRepository::new(asset, release));
    let service = HostedBuildOutcomeApplicationService::new(repository.clone());
    let source_event_id = Uuid::now_v7();
    service
        .project(
            outcome.clone(),
            source_event_id,
            outcome.operation_id().as_uuid(),
        )
        .await
        .expect("first projection");
    service
        .project(
            outcome.clone(),
            source_event_id,
            outcome.operation_id().as_uuid(),
        )
        .await
        .expect("replayed projection");

    let state = repository.state.read().await;
    assert_eq!(state.transitions, 1);
    assert_eq!(state.release.state, AssetReleaseState::Published);
    assert_eq!(
        state
            .release
            .provenance
            .as_ref()
            .expect("provenance")
            .build_run_id(),
        outcome.build_run_id()
    );
}

#[tokio::test]
async fn archived_owner_ignores_success_without_reopening_or_failing_build() {
    let (mut asset, release, outcome) = fixture();
    asset
        .archive(asset.updated_at + Duration::seconds(1))
        .expect("archive");
    let repository = Arc::new(TestAssetRepository::new(asset, release));
    HostedBuildOutcomeApplicationService::new(repository.clone())
        .project(
            outcome.clone(),
            Uuid::now_v7(),
            outcome.operation_id().as_uuid(),
        )
        .await
        .expect("terminal archive is an acknowledged no-op");

    let state = repository.state.read().await;
    assert_eq!(state.transitions, 0);
    assert_eq!(state.release.state, AssetReleaseState::Draft);
}

#[tokio::test]
async fn archived_owner_still_rejects_source_identity_drift() {
    let (mut asset, release, outcome) = fixture();
    asset
        .archive(asset.updated_at + Duration::seconds(1))
        .expect("archive");
    let repository = Arc::new(TestAssetRepository::new(asset, release));
    let mut payload = serde_json::to_value(&outcome).expect("outcome JSON");
    payload["commitSha"] = serde_json::Value::String("f".repeat(40));
    let drifted: HostedBuildOutcome = serde_json::from_value(payload).expect("valid drifted fact");

    let error = HostedBuildOutcomeApplicationService::new(repository.clone())
        .project(drifted, Uuid::now_v7(), outcome.operation_id().as_uuid())
        .await
        .expect_err("source drift must fail closed");
    assert!(matches!(error, RepositoryError::Conflict(_)));
    let state = repository.state.read().await;
    assert_eq!(state.transitions, 0);
    assert_eq!(state.release.state, AssetReleaseState::Draft);
}

#[tokio::test]
async fn outbox_adapter_rejects_envelope_drift_before_mutating_assets() {
    let (asset, release, outcome) = fixture();
    let repository = Arc::new(TestAssetRepository::new(asset, release));
    let projector = HostedBuildOutcomeProjector::new(repository.clone());
    let mut message = OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: crate::modules::artifacts::published::HOSTED_BUILD_OUTCOME_EVENT_KEY.into(),
        schema_version: 1,
        organization_id: outcome.organization_id().as_uuid(),
        aggregate_id: outcome.build_run_id().as_uuid(),
        aggregate_version: outcome.build_run_version(),
        occurred_at: outcome.finished_at(),
        correlation_id: outcome.operation_id().as_uuid(),
        causation_id: None,
        payload: serde_json::to_value(outcome).expect("outcome JSON"),
        delivery_attempts: 1,
    };
    message.aggregate_version += 1;

    let error = projector
        .project(&message)
        .await
        .expect_err("envelope drift must fail closed");
    assert!(matches!(error, RepositoryError::Storage(_)));
    let state = repository.state.read().await;
    assert_eq!(state.transitions, 0);
    assert_eq!(state.release.state, AssetReleaseState::Draft);
}
