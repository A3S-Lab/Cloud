use crate::modules::artifacts::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactReader, NodeArtifactStoreError,
    NodeArtifactWrite, OpenNodeArtifact,
};
use crate::modules::assets::{
    AcquireAssetGitWriteLease, Asset, AssetGitBackup, AssetGitBuildInput, AssetGitRepository,
    AssetGitRepositoryControlError, AssetGitRepositoryError, AssetGitRepositoryWrite,
    AssetGitRpcLimits, AssetGitRpcResponse, AssetGitService, AssetGitWriteJournal,
    AssetGitWriteLease, AssetGitWriteRecovery, AssetManifestAdmission, AssetRelease,
    AssetReleaseWrite, AssetWrite, BindMcpServiceProfileWrite, ClaimAssetGitWriteRecovery,
    CompleteAssetGitWriteLease, CreateAssetReleaseWrite, CreateAssetWrite, IAssetGitRepository,
    IAssetGitRepositoryControl, IAssetRepository, IMcpServiceProfileRepository,
    McpServiceProfileBinding, McpServiceProfileWrite, TransitionAssetReleaseWrite,
    TransitionAssetWrite,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, GitCommitSha, OrganizationId, RepositoryError,
    Sha256Digest,
};
use a3s_runtime::contract::ArtifactRef;
use chrono::{DateTime, Utc};

pub(super) struct UnavailableAssetStore;

type UnavailableResult<T, E> = Result<T, E>;

#[async_trait::async_trait]
impl IAssetRepository for UnavailableAssetStore {
    async fn create_asset(
        &self,
        _bundle: CreateAssetWrite,
    ) -> UnavailableResult<AssetWrite, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn transition_asset(
        &self,
        _bundle: TransitionAssetWrite,
    ) -> UnavailableResult<AssetWrite, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn find_asset(
        &self,
        _organization_id: OrganizationId,
        _asset_id: AssetId,
    ) -> UnavailableResult<Option<Asset>, RepositoryError> {
        Ok(None)
    }

    async fn list_assets(
        &self,
        _organization_id: OrganizationId,
    ) -> UnavailableResult<Vec<Asset>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn create_release(
        &self,
        _bundle: CreateAssetReleaseWrite,
    ) -> UnavailableResult<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn transition_release(
        &self,
        _bundle: TransitionAssetReleaseWrite,
    ) -> UnavailableResult<AssetReleaseWrite, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn find_release(
        &self,
        _organization_id: OrganizationId,
        _asset_id: AssetId,
        _asset_release_id: AssetReleaseId,
    ) -> UnavailableResult<Option<AssetRelease>, RepositoryError> {
        Ok(None)
    }

    async fn list_releases(
        &self,
        _organization_id: OrganizationId,
        _asset_id: AssetId,
    ) -> UnavailableResult<Vec<AssetRelease>, RepositoryError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
impl IMcpServiceProfileRepository for UnavailableAssetStore {
    async fn bind_mcp_service_profile(
        &self,
        _bundle: BindMcpServiceProfileWrite,
    ) -> UnavailableResult<McpServiceProfileWrite, RepositoryError> {
        Err(RepositoryError::NotFound)
    }

    async fn find_mcp_service_profile(
        &self,
        _organization_id: OrganizationId,
        _asset_id: AssetId,
        _asset_release_id: AssetReleaseId,
    ) -> UnavailableResult<Option<McpServiceProfileBinding>, RepositoryError> {
        Ok(None)
    }
}

#[async_trait::async_trait]
impl IAssetGitRepository for UnavailableAssetStore {
    async fn provision(
        &self,
        _asset: &Asset,
    ) -> UnavailableResult<AssetGitRepositoryWrite, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn inspect(
        &self,
        _asset: &Asset,
    ) -> UnavailableResult<AssetGitRepository, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn prepare_write(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
    ) -> UnavailableResult<(), AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn rollback_write(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
    ) -> UnavailableResult<(), AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn settle_write(
        &self,
        _asset: &Asset,
        _journal: &AssetGitWriteJournal,
    ) -> UnavailableResult<(), AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn advertise(
        &self,
        _asset: &Asset,
        _service: AssetGitService,
    ) -> UnavailableResult<Vec<u8>, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn execute_rpc(
        &self,
        _asset: &Asset,
        _service: AssetGitService,
        _request: Vec<u8>,
        _limits: AssetGitRpcLimits,
        _write_lease: Option<&AssetGitWriteLease>,
    ) -> UnavailableResult<AssetGitRpcResponse, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn repository_bytes(
        &self,
        _asset: &Asset,
    ) -> UnavailableResult<u64, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn refs_digest(
        &self,
        _asset: &Asset,
    ) -> UnavailableResult<Sha256Digest, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn create_backup(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
        _created_at: DateTime<Utc>,
    ) -> UnavailableResult<AssetGitBackup, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn restore_backup(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
        _backup: &AssetGitBackup,
        _maximum_repository_bytes: u64,
    ) -> UnavailableResult<AssetGitRpcResponse, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn admit_manifest(
        &self,
        _asset: &Asset,
        _commit_sha: &GitCommitSha,
    ) -> UnavailableResult<AssetManifestAdmission, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn prepare_build_input(
        &self,
        _asset: &Asset,
        _commit_sha: &GitCommitSha,
        _build_run_id: BuildRunId,
    ) -> UnavailableResult<AssetGitBuildInput, AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }

    async fn remove_build_input(
        &self,
        _build_run_id: BuildRunId,
    ) -> UnavailableResult<(), AssetGitRepositoryError> {
        Err(AssetGitRepositoryError::NotFound)
    }
}

#[async_trait::async_trait]
impl INodeArtifactStore for UnavailableAssetStore {
    async fn put(
        &self,
        _descriptor: &NodeArtifactDescriptor,
        _reader: NodeArtifactReader,
    ) -> Result<NodeArtifactWrite, NodeArtifactStoreError> {
        Err(NodeArtifactStoreError::Storage(
            "test Artifact store is unavailable".into(),
        ))
    }

    async fn open(
        &self,
        _artifact: &ArtifactRef,
    ) -> Result<OpenNodeArtifact, NodeArtifactStoreError> {
        Err(NodeArtifactStoreError::NotFound)
    }
}

#[async_trait::async_trait]
impl IAssetGitRepositoryControl for UnavailableAssetStore {
    async fn claim_write_recovery(
        &self,
        _request: ClaimAssetGitWriteRecovery,
    ) -> UnavailableResult<Option<AssetGitWriteRecovery>, AssetGitRepositoryControlError> {
        Err(AssetGitRepositoryControlError::NotFound)
    }

    async fn acquire_write(
        &self,
        _request: AcquireAssetGitWriteLease,
    ) -> UnavailableResult<AssetGitWriteLease, AssetGitRepositoryControlError> {
        Err(AssetGitRepositoryControlError::NotFound)
    }

    async fn complete_write(
        &self,
        _completion: CompleteAssetGitWriteLease,
    ) -> UnavailableResult<(), AssetGitRepositoryControlError> {
        Err(AssetGitRepositoryControlError::NotFound)
    }

    async fn abandon_write(
        &self,
        _lease: &AssetGitWriteLease,
    ) -> UnavailableResult<(), AssetGitRepositoryControlError> {
        Err(AssetGitRepositoryControlError::NotFound)
    }

    async fn settle_write(
        &self,
        _journal: &AssetGitWriteJournal,
    ) -> UnavailableResult<(), AssetGitRepositoryControlError> {
        Err(AssetGitRepositoryControlError::NotFound)
    }
}
