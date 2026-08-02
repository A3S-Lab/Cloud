use super::{AssetGitApplicationService, AssetGitApplicationServiceOptions};
use crate::modules::assets::domain::{
    AcquireAssetGitWriteLease, Asset, AssetGitBackup, AssetGitRepository,
    AssetGitRepositoryControlError, AssetGitRepositoryError, AssetGitRepositoryWrite,
    AssetGitRpcLimits, AssetGitRpcResponse, AssetGitService, AssetGitWriteJournal,
    AssetGitWriteLease, AssetGitWriteOperation, AssetGitWriteRecovery, AssetKind,
    AssetManifestAdmission, AssetRelease, AssetReleaseWrite, AssetWrite,
    ClaimAssetGitWriteRecovery, CompleteAssetGitWriteLease, CreateAssetReleaseWrite,
    CreateAssetWrite, IAssetGitRepository, IAssetGitRepositoryControl, IAssetRepository,
    TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, OrganizationId, RepositoryError, ResourceName,
    Sha256Digest,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

struct TestStore {
    asset: Asset,
    state: Mutex<TestState>,
}

#[derive(Default)]
struct TestState {
    repository_error: Option<AssetGitRepositoryError>,
    acquire_error: Option<AssetGitRepositoryControlError>,
    complete_error: Option<AssetGitRepositoryControlError>,
    abandon_error: Option<AssetGitRepositoryControlError>,
    recoveries: VecDeque<AssetGitWriteRecovery>,
    prepared: usize,
    rolled_back: usize,
    repository_settled: usize,
    acquired: usize,
    completed: usize,
    abandoned: usize,
    control_settled: usize,
    backups: usize,
    restores: usize,
    operations: Vec<AssetGitWriteOperation>,
}

impl TestStore {
    fn new(asset: Asset) -> Self {
        Self {
            asset,
            state: Mutex::new(TestState::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, TestState> {
        self.state.lock().expect("test state")
    }
}

#[async_trait]
impl IAssetRepository for TestStore {
    async fn create_asset(&self, _bundle: CreateAssetWrite) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused test write".into()))
    }

    async fn transition_asset(
        &self,
        _bundle: TransitionAssetWrite,
    ) -> Result<AssetWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused test write".into()))
    }

    async fn find_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Option<Asset>, RepositoryError> {
        Ok(
            (self.asset.organization_id == organization_id && self.asset.id == asset_id)
                .then(|| self.asset.clone()),
        )
    }

    async fn list_assets(
        &self,
        _organization_id: OrganizationId,
    ) -> Result<Vec<Asset>, RepositoryError> {
        Ok(vec![self.asset.clone()])
    }

    async fn create_release(
        &self,
        _bundle: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused test write".into()))
    }

    async fn transition_release(
        &self,
        _bundle: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::Storage("unused test write".into()))
    }

    async fn find_release(
        &self,
        _organization_id: OrganizationId,
        _asset_id: AssetId,
        _asset_release_id: AssetReleaseId,
    ) -> Result<Option<AssetRelease>, RepositoryError> {
        Ok(None)
    }

    async fn list_releases(
        &self,
        _organization_id: OrganizationId,
        _asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl IAssetGitRepository for TestStore {
    async fn provision(
        &self,
        asset: &Asset,
    ) -> Result<AssetGitRepositoryWrite, AssetGitRepositoryError> {
        Ok(AssetGitRepositoryWrite {
            repository: AssetGitRepository::for_asset(asset)
                .map_err(AssetGitRepositoryError::Invalid)?,
            created: false,
        })
    }

    async fn inspect(&self, asset: &Asset) -> Result<AssetGitRepository, AssetGitRepositoryError> {
        AssetGitRepository::for_asset(asset).map_err(AssetGitRepositoryError::Invalid)
    }

    async fn prepare_write(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError> {
        self.state().prepared += 1;
        Ok(())
    }

    async fn rollback_write(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError> {
        self.state().rolled_back += 1;
        Ok(())
    }

    async fn settle_write(
        &self,
        _asset: &Asset,
        _journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryError> {
        self.state().repository_settled += 1;
        Ok(())
    }

    async fn advertise(
        &self,
        _asset: &Asset,
        _service: AssetGitService,
    ) -> Result<Vec<u8>, AssetGitRepositoryError> {
        Ok(b"advertisement".to_vec())
    }

    async fn execute_rpc(
        &self,
        _asset: &Asset,
        _service: AssetGitService,
        _request: Vec<u8>,
        _limits: AssetGitRpcLimits,
        _write_lease: Option<&AssetGitWriteLease>,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
        if let Some(error) = self.state().repository_error.clone() {
            return Err(error);
        }
        Ok(rpc_response())
    }

    async fn repository_bytes(&self, _asset: &Asset) -> Result<u64, AssetGitRepositoryError> {
        Ok(128)
    }

    async fn refs_digest(&self, _asset: &Asset) -> Result<Sha256Digest, AssetGitRepositoryError> {
        Ok(digest('a'))
    }

    async fn create_backup(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
        _created_at: chrono::DateTime<Utc>,
    ) -> Result<AssetGitBackup, AssetGitRepositoryError> {
        self.state().backups += 1;
        Ok(backup())
    }

    async fn restore_backup(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
        _backup: &AssetGitBackup,
        _maximum_repository_bytes: u64,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
        self.state().restores += 1;
        Ok(rpc_response())
    }

    async fn admit_manifest(
        &self,
        asset: &Asset,
        commit_sha: &GitCommitSha,
    ) -> Result<AssetManifestAdmission, AssetGitRepositoryError> {
        Ok(AssetManifestAdmission {
            commit_sha: commit_sha.clone(),
            manifest_digest: digest('b'),
            kind: asset.kind,
        })
    }
}

#[async_trait]
impl IAssetGitRepositoryControl for TestStore {
    async fn claim_write_recovery(
        &self,
        _request: ClaimAssetGitWriteRecovery,
    ) -> Result<Option<AssetGitWriteRecovery>, AssetGitRepositoryControlError> {
        Ok(self.state().recoveries.pop_front())
    }

    async fn acquire_write(
        &self,
        request: AcquireAssetGitWriteLease,
    ) -> Result<AssetGitWriteLease, AssetGitRepositoryControlError> {
        let mut state = self.state();
        state.acquired += 1;
        state.operations.push(request.operation);
        if let Some(error) = state.acquire_error.clone() {
            return Err(error);
        }
        Ok(AssetGitWriteLease {
            organization_id: request.asset.organization_id,
            asset_id: request.asset.id,
            lease_id: request.lease_id,
            operation: request.operation,
            actor_id: request.actor_id,
            request_id: request.request_id,
            quota_bytes: request.default_quota_bytes,
            observed_bytes: request.observed_bytes,
            leased_until: request.leased_until,
            recovery: false,
        })
    }

    async fn complete_write(
        &self,
        _completion: CompleteAssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryControlError> {
        let mut state = self.state();
        state.completed += 1;
        match state.complete_error.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn abandon_write(
        &self,
        _lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryControlError> {
        let mut state = self.state();
        state.abandoned += 1;
        match state.abandon_error.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn settle_write(
        &self,
        _journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryControlError> {
        self.state().control_settled += 1;
        Ok(())
    }
}

#[tokio::test]
async fn repository_failure_rolls_back_and_releases_the_write_lease() {
    let (asset, store, service) = fixture();
    store.state().repository_error = Some(AssetGitRepositoryError::Storage("failed".into()));
    let result = receive(&service, &asset).await;
    assert!(matches!(result, Err(ApplicationError::Internal(_))));
    let state = store.state();
    assert_eq!(state.prepared, 1);
    assert_eq!(state.rolled_back, 1);
    assert_eq!(state.abandoned, 1);
    assert_eq!(state.completed, 0);
}

#[tokio::test]
async fn lease_release_failure_replaces_the_primary_error_with_internal() {
    let (asset, store, service) = fixture();
    store.state().repository_error = Some(AssetGitRepositoryError::Invalid("bad pack".into()));
    store.state().abandon_error = Some(AssetGitRepositoryControlError::Storage("failed".into()));
    assert!(matches!(
        receive(&service, &asset).await,
        Err(ApplicationError::Internal(message))
            if message == "hosted Git write failed and its lease could not be released"
    ));
}

#[tokio::test]
async fn busy_postgres_writer_maps_to_conflict() {
    let (asset, store, service) = fixture();
    store.state().acquire_error = Some(AssetGitRepositoryControlError::Busy);
    assert!(matches!(
        receive(&service, &asset).await,
        Err(ApplicationError::Conflict(message))
            if message == "hosted Git repository already has a writer"
    ));
    assert_eq!(store.state().prepared, 0);
}

#[tokio::test]
async fn successful_receive_pack_completes_and_settles_exactly_one_lease() {
    let (asset, store, service) = fixture();
    assert_eq!(
        receive(&service, &asset).await.expect("receive pack"),
        rpc_response()
    );
    let state = store.state();
    assert_eq!(state.acquired, 1);
    assert_eq!(state.prepared, 1);
    assert_eq!(state.completed, 1);
    assert_eq!(state.repository_settled, 1);
    assert_eq!(state.control_settled, 1);
    assert_eq!(state.abandoned, 0);
}

#[tokio::test]
async fn uncertain_control_completion_retains_the_journal_for_expiry_recovery() {
    let (asset, store, service) = fixture();
    store.state().complete_error = Some(AssetGitRepositoryControlError::Storage(
        "unknown commit outcome".into(),
    ));
    assert!(matches!(
        receive(&service, &asset).await,
        Err(ApplicationError::Internal(_))
    ));
    let state = store.state();
    assert_eq!(state.completed, 1);
    assert_eq!(state.rolled_back, 0);
    assert_eq!(state.abandoned, 0);
    assert_eq!(state.repository_settled, 0);
    assert_eq!(state.control_settled, 0);
}

#[tokio::test]
async fn backup_and_restore_use_the_same_write_lease_and_journal_path() {
    let (asset, store, service) = fixture();
    let actor = Uuid::now_v7();
    assert_eq!(
        service
            .backup_repository(asset.organization_id, asset.id, actor, Uuid::now_v7())
            .await
            .expect("backup"),
        backup()
    );
    assert_eq!(
        service
            .restore_repository(
                asset.organization_id,
                asset.id,
                actor,
                Uuid::now_v7(),
                backup(),
            )
            .await
            .expect("restore"),
        rpc_response()
    );
    let state = store.state();
    assert_eq!(
        state.operations,
        vec![
            AssetGitWriteOperation::Backup,
            AssetGitWriteOperation::Restore
        ]
    );
    assert_eq!(state.backups, 1);
    assert_eq!(state.restores, 1);
    assert_eq!(state.prepared, 2);
    assert_eq!(state.completed, 2);
    assert_eq!(state.repository_settled, 2);
    assert_eq!(state.control_settled, 2);
}

#[tokio::test]
async fn recovery_reuses_the_expired_lease_and_cleans_committed_journal_before_new_work() {
    let (asset, store, service) = fixture();
    let expired = lease(&asset, AssetGitWriteOperation::ReceivePack, true);
    let cleanup = lease(&asset, AssetGitWriteOperation::Backup, false).journal();
    store.state().recoveries.extend([
        AssetGitWriteRecovery::Rollback(expired),
        AssetGitWriteRecovery::Cleanup(cleanup),
    ]);
    receive(&service, &asset)
        .await
        .expect("recovered receive pack");
    let state = store.state();
    assert_eq!(state.rolled_back, 1);
    assert_eq!(state.abandoned, 1);
    assert_eq!(state.repository_settled, 2);
    assert_eq!(state.control_settled, 2);
    assert_eq!(state.acquired, 1);
    assert_eq!(state.completed, 1);
}

#[tokio::test]
async fn archived_repository_is_readable_but_every_mutation_is_denied() {
    let (mut asset, _, _) = fixture();
    asset
        .archive(asset.updated_at + ChronoDuration::seconds(1))
        .expect("archive Asset");
    let store = Arc::new(TestStore::new(asset.clone()));
    let service = service(Arc::clone(&store));
    let actor = Uuid::now_v7();
    assert_eq!(
        service
            .advertise(asset.organization_id, asset.id, AssetGitService::UploadPack)
            .await
            .expect("advertise archived repository"),
        b"advertisement"
    );
    service
        .upload_pack(asset.organization_id, asset.id, b"read".to_vec())
        .await
        .expect("read archived repository");
    service
        .admit_manifest(
            asset.organization_id,
            asset.id,
            GitCommitSha::parse("c".repeat(40)).expect("commit"),
        )
        .await
        .expect("inspect archived manifest");
    for result in [
        receive(&service, &asset).await.map(|_| ()),
        service
            .backup_repository(asset.organization_id, asset.id, actor, Uuid::now_v7())
            .await
            .map(|_| ()),
        service
            .restore_repository(
                asset.organization_id,
                asset.id,
                actor,
                Uuid::now_v7(),
                backup(),
            )
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(ApplicationError::Forbidden(message))
                if message == "archived Asset repository is read-only"
        ));
    }
    let state = store.state();
    assert_eq!(state.acquired, 0);
    assert_eq!(state.prepared, 0);
}

fn fixture() -> (Asset, Arc<TestStore>, AssetGitApplicationService) {
    let asset = Asset::create(
        AssetId::new(),
        OrganizationId::new(),
        ResourceName::parse("Hosted Git test").expect("name"),
        AssetKind::Agent,
        Utc::now(),
    )
    .expect("Asset");
    let store = Arc::new(TestStore::new(asset.clone()));
    let service = service(Arc::clone(&store));
    (asset, store, service)
}

fn service(store: Arc<TestStore>) -> AssetGitApplicationService {
    let assets: Arc<dyn IAssetRepository> = store.clone();
    let repositories: Arc<dyn IAssetGitRepository> = store.clone();
    let controls: Arc<dyn IAssetGitRepositoryControl> = store;
    AssetGitApplicationService::new(
        assets,
        repositories,
        controls,
        AssetGitApplicationServiceOptions {
            write_lease: Duration::from_secs(30),
            default_repository_quota_bytes: 1024 * 1024,
            maximum_rpc_body_bytes: 64 * 1024,
        },
    )
    .expect("service")
}

async fn receive(
    service: &AssetGitApplicationService,
    asset: &Asset,
) -> Result<AssetGitRpcResponse, ApplicationError> {
    service
        .receive_pack(
            asset.organization_id,
            asset.id,
            Uuid::now_v7(),
            Uuid::now_v7(),
            b"pack".to_vec(),
        )
        .await
}

fn lease(asset: &Asset, operation: AssetGitWriteOperation, recovery: bool) -> AssetGitWriteLease {
    AssetGitWriteLease {
        organization_id: asset.organization_id,
        asset_id: asset.id,
        lease_id: Uuid::now_v7(),
        operation,
        actor_id: Uuid::now_v7(),
        request_id: Uuid::now_v7(),
        quota_bytes: 1024 * 1024,
        observed_bytes: 128,
        leased_until: Utc::now() + ChronoDuration::seconds(30),
        recovery,
    }
}

fn rpc_response() -> AssetGitRpcResponse {
    AssetGitRpcResponse {
        body: b"result".to_vec(),
        repository_bytes: 256,
        refs_digest: digest('a'),
    }
}

fn backup() -> AssetGitBackup {
    AssetGitBackup {
        object_key: "repositories/test/backup.bundle".into(),
        digest: digest('d'),
        size_bytes: 512,
        refs_digest: digest('a'),
        created_at: chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("backup time"),
    }
}

fn digest(value: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("digest")
}
