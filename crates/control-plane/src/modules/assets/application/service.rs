use crate::modules::assets::application::resource_access::{AssetAccess, AssetResourceAccess};
use crate::modules::assets::domain::{
    validate_asset_repository_mutation, AcquireAssetGitWriteLease, Asset, AssetGitBackup,
    AssetGitRepositoryControlError, AssetGitRepositoryError, AssetGitRpcLimits,
    AssetGitRpcResponse, AssetGitService, AssetGitWriteLease, AssetGitWriteOperation,
    AssetGitWriteRecovery, AssetManifestAdmission, ClaimAssetGitWriteRecovery,
    CompleteAssetGitWriteLease, IAssetGitRepository, IAssetGitRepositoryControl, IAssetRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AssetId, GitCommitSha, OrganizationId};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetGitApplicationServiceOptions {
    pub write_lease: Duration,
    pub default_repository_quota_bytes: u64,
    pub maximum_rpc_body_bytes: u64,
}

pub struct AssetGitApplicationService {
    assets: Arc<dyn IAssetRepository>,
    resource_access: AssetResourceAccess,
    repositories: Arc<dyn IAssetGitRepository>,
    controls: Arc<dyn IAssetGitRepositoryControl>,
    options: AssetGitApplicationServiceOptions,
}

impl AssetGitApplicationService {
    pub fn new(
        assets: Arc<dyn IAssetRepository>,
        repositories: Arc<dyn IAssetGitRepository>,
        controls: Arc<dyn IAssetGitRepositoryControl>,
        options: AssetGitApplicationServiceOptions,
    ) -> Result<Self, String> {
        if options.write_lease.is_zero()
            || options.write_lease > Duration::from_secs(3_600)
            || options.default_repository_quota_bytes == 0
            || options.maximum_rpc_body_bytes == 0
            || options.maximum_rpc_body_bytes > options.default_repository_quota_bytes
        {
            return Err("Asset Git application service options are invalid".into());
        }
        Ok(Self {
            assets: Arc::clone(&assets),
            resource_access: AssetResourceAccess::new(assets),
            repositories,
            controls,
            options,
        })
    }

    pub async fn advertise(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        service: AssetGitService,
        access: &AssetAccess,
    ) -> ApplicationResult<Vec<u8>> {
        let asset = self
            .load_consistent_authorized_asset(
                organization_id,
                asset_id,
                access,
                AssetGitAccess::Read,
            )
            .await?;
        self.repositories
            .advertise(&asset, service)
            .await
            .map_err(map_repository_error)
    }

    pub async fn upload_pack(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        request: Vec<u8>,
        access: &AssetAccess,
    ) -> ApplicationResult<AssetGitRpcResponse> {
        let asset = self
            .load_consistent_authorized_asset(
                organization_id,
                asset_id,
                access,
                AssetGitAccess::Read,
            )
            .await?;
        self.repositories
            .execute_rpc(
                &asset,
                AssetGitService::UploadPack,
                request,
                AssetGitRpcLimits {
                    maximum_input_bytes: self.options.maximum_rpc_body_bytes,
                    maximum_repository_bytes: self.options.default_repository_quota_bytes,
                },
                None,
            )
            .await
            .map_err(map_repository_error)
    }

    pub async fn receive_pack(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        access: &AssetAccess,
        actor_id: Uuid,
        request_id: Uuid,
        request: Vec<u8>,
    ) -> ApplicationResult<AssetGitRpcResponse> {
        validate_actor_request(actor_id, request_id)?;
        let asset = self
            .load_authorized_asset(organization_id, asset_id, access)
            .await?;
        let lease = self
            .acquire_and_prepare(
                &asset,
                AssetGitWriteOperation::ReceivePack,
                actor_id,
                request_id,
            )
            .await?;
        let result = self
            .repositories
            .execute_rpc(
                &asset,
                AssetGitService::ReceivePack,
                request,
                AssetGitRpcLimits {
                    maximum_input_bytes: self.options.maximum_rpc_body_bytes,
                    maximum_repository_bytes: lease.quota_bytes,
                },
                Some(&lease),
            )
            .await;
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                return Err(self
                    .rollback_after_error(&asset, &lease, map_repository_error(error))
                    .await)
            }
        };
        if response.repository_bytes > lease.quota_bytes {
            return Err(self
                .rollback_after_error(
                    &asset,
                    &lease,
                    ApplicationError::Conflict("hosted Git repository quota exceeded".into()),
                )
                .await);
        }
        self.complete(&asset, &lease, &response, None).await?;
        Ok(response)
    }

    pub async fn backup_repository(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        actor_id: Uuid,
        request_id: Uuid,
    ) -> ApplicationResult<AssetGitBackup> {
        validate_actor_request(actor_id, request_id)?;
        let asset = self.load_asset(organization_id, asset_id).await?;
        let lease = self
            .acquire_and_prepare(&asset, AssetGitWriteOperation::Backup, actor_id, request_id)
            .await?;
        let backup = match self
            .repositories
            .create_backup(&asset, &lease, Utc::now())
            .await
        {
            Ok(backup) => backup,
            Err(error) => {
                return Err(self
                    .rollback_after_error(&asset, &lease, map_repository_error(error))
                    .await)
            }
        };
        let response = AssetGitRpcResponse {
            body: Vec::new(),
            repository_bytes: match self.repositories.repository_bytes(&asset).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Err(self
                        .rollback_after_error(&asset, &lease, map_repository_error(error))
                        .await)
                }
            },
            refs_digest: backup.refs_digest.clone(),
        };
        self.complete(&asset, &lease, &response, Some(backup.clone()))
            .await?;
        Ok(backup)
    }

    pub async fn restore_repository(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        actor_id: Uuid,
        request_id: Uuid,
        backup: AssetGitBackup,
    ) -> ApplicationResult<AssetGitRpcResponse> {
        validate_actor_request(actor_id, request_id)?;
        backup.validate().map_err(ApplicationError::Invalid)?;
        let asset = self.load_asset(organization_id, asset_id).await?;
        let lease = self
            .acquire_and_prepare(
                &asset,
                AssetGitWriteOperation::Restore,
                actor_id,
                request_id,
            )
            .await?;
        let response = match self
            .repositories
            .restore_backup(&asset, &lease, &backup, lease.quota_bytes)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return Err(self
                    .rollback_after_error(&asset, &lease, map_repository_error(error))
                    .await)
            }
        };
        self.complete(&asset, &lease, &response, None).await?;
        Ok(response)
    }

    pub async fn admit_manifest(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        commit_sha: GitCommitSha,
    ) -> ApplicationResult<AssetManifestAdmission> {
        let asset = self
            .load_consistent_asset(organization_id, asset_id, AssetGitAccess::Read)
            .await?;
        self.repositories
            .admit_manifest(&asset, &commit_sha)
            .await
            .map_err(map_repository_error)
    }

    async fn load_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> ApplicationResult<Asset> {
        self.assets
            .find_asset(organization_id, asset_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("hosted Git repository not found".into()))
    }

    async fn load_consistent_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        access: AssetGitAccess,
    ) -> ApplicationResult<Asset> {
        let asset = self.load_asset(organization_id, asset_id).await?;
        self.recover_pending(&asset, access).await?;
        Ok(asset)
    }

    async fn load_authorized_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        access: &AssetAccess,
    ) -> ApplicationResult<Asset> {
        self.resource_access
            .asset(
                organization_id,
                asset_id,
                access,
                "hosted Git repository not found",
            )
            .await
    }

    async fn load_consistent_authorized_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        access: &AssetAccess,
        repository_access: AssetGitAccess,
    ) -> ApplicationResult<Asset> {
        let asset = self
            .load_authorized_asset(organization_id, asset_id, access)
            .await?;
        self.recover_pending(&asset, repository_access).await?;
        Ok(asset)
    }

    async fn acquire_and_prepare(
        &self,
        asset: &Asset,
        operation: AssetGitWriteOperation,
        actor_id: Uuid,
        request_id: Uuid,
    ) -> ApplicationResult<AssetGitWriteLease> {
        self.recover_pending(asset, AssetGitAccess::Write).await?;
        validate_asset_repository_mutation(asset).map_err(ApplicationError::Forbidden)?;
        let lease = self.acquire(asset, operation, actor_id, request_id).await?;
        if let Err(error) = self.repositories.prepare_write(asset, &lease).await {
            return Err(self
                .rollback_after_error(asset, &lease, map_repository_error(error))
                .await);
        }
        Ok(lease)
    }

    async fn acquire(
        &self,
        asset: &Asset,
        operation: AssetGitWriteOperation,
        actor_id: Uuid,
        request_id: Uuid,
    ) -> ApplicationResult<AssetGitWriteLease> {
        loop {
            self.recover_pending(asset, AssetGitAccess::Write).await?;
            self.repositories
                .inspect(asset)
                .await
                .map_err(map_repository_error)?;
            let observed_bytes = self
                .repositories
                .repository_bytes(asset)
                .await
                .map_err(map_repository_error)?;
            let acquired_at = Utc::now();
            let leased_until = self.leased_until(acquired_at)?;
            match self
                .controls
                .acquire_write(AcquireAssetGitWriteLease {
                    asset: asset.clone(),
                    lease_id: Uuid::now_v7(),
                    operation,
                    actor_id,
                    request_id,
                    observed_bytes,
                    default_quota_bytes: self.options.default_repository_quota_bytes,
                    acquired_at,
                    leased_until,
                })
                .await
            {
                Ok(lease) => return Ok(lease),
                Err(AssetGitRepositoryControlError::RecoveryRequired) => continue,
                Err(error) => return Err(map_control_error(error)),
            }
        }
    }

    async fn complete(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
        response: &AssetGitRpcResponse,
        backup: Option<AssetGitBackup>,
    ) -> ApplicationResult<()> {
        self.controls
            .complete_write(CompleteAssetGitWriteLease {
                lease: lease.clone(),
                observed_bytes: response.repository_bytes,
                refs_digest: response.refs_digest.clone(),
                backup,
                completed_at: Utc::now(),
            })
            .await
            .map_err(map_control_error)?;
        let journal = lease.journal();
        self.repositories
            .settle_write(asset, &journal)
            .await
            .map_err(|error| {
                tracing::error!(
                    asset_id = %lease.asset_id,
                    lease_id = %lease.lease_id,
                    error = %error,
                    "committed hosted Git write journal could not be removed"
                );
                ApplicationError::Internal(
                    "committed hosted Git write journal could not be settled".into(),
                )
            })?;
        self.controls
            .settle_write(&journal)
            .await
            .map_err(map_control_error)
    }

    async fn rollback_after_error(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
        primary: ApplicationError,
    ) -> ApplicationError {
        if let Err(error) = self.repositories.rollback_write(asset, lease).await {
            tracing::error!(
                asset_id = %lease.asset_id,
                lease_id = %lease.lease_id,
                primary = %primary,
                rollback = %error,
                "hosted Git write failed and its repository journal could not be rolled back"
            );
            return ApplicationError::Internal(
                "hosted Git write failed and its repository could not be rolled back".into(),
            );
        }
        match self.controls.abandon_write(lease).await {
            Ok(()) => primary,
            Err(error) => {
                tracing::error!(
                    asset_id = %lease.asset_id,
                    lease_id = %lease.lease_id,
                    primary = %primary,
                    abandon = %error,
                    "hosted Git write failed and its lease could not be abandoned"
                );
                ApplicationError::Internal(
                    "hosted Git write failed and its lease could not be released".into(),
                )
            }
        }
    }

    async fn recover_pending(
        &self,
        asset: &Asset,
        access: AssetGitAccess,
    ) -> ApplicationResult<()> {
        loop {
            let claimed_at = Utc::now();
            let recovery = self
                .controls
                .claim_write_recovery(ClaimAssetGitWriteRecovery {
                    asset: asset.clone(),
                    claimed_at,
                    leased_until: self.leased_until(claimed_at)?,
                })
                .await
                .map_err(map_control_error)?;
            match recovery {
                None => return Ok(()),
                Some(AssetGitWriteRecovery::Active) => {
                    return Err(match access {
                        AssetGitAccess::Read => ApplicationError::Unavailable(
                            "hosted Git repository has an active writer".into(),
                        ),
                        AssetGitAccess::Write => ApplicationError::Conflict(
                            "hosted Git repository already has a writer".into(),
                        ),
                    })
                }
                Some(AssetGitWriteRecovery::Rollback(lease)) => {
                    self.repositories
                        .rollback_write(asset, &lease)
                        .await
                        .map_err(|error| {
                            tracing::error!(
                                asset_id = %lease.asset_id,
                                lease_id = %lease.lease_id,
                                error = %error,
                                "expired hosted Git write could not be rolled back"
                            );
                            ApplicationError::Internal(
                                "expired hosted Git write could not be recovered".into(),
                            )
                        })?;
                    self.controls
                        .abandon_write(&lease)
                        .await
                        .map_err(map_control_error)?;
                }
                Some(AssetGitWriteRecovery::Cleanup(journal)) => {
                    self.repositories
                        .settle_write(asset, &journal)
                        .await
                        .map_err(|error| {
                            tracing::error!(
                                asset_id = %journal.asset_id,
                                lease_id = %journal.lease_id,
                                error = %error,
                                "committed hosted Git write recovery could not remove its journal"
                            );
                            ApplicationError::Internal(
                                "committed hosted Git write could not be recovered".into(),
                            )
                        })?;
                    self.controls
                        .settle_write(&journal)
                        .await
                        .map_err(map_control_error)?;
                }
            }
        }
    }

    fn leased_until(
        &self,
        acquired_at: chrono::DateTime<Utc>,
    ) -> ApplicationResult<chrono::DateTime<Utc>> {
        let duration = chrono::Duration::from_std(self.options.write_lease)
            .map_err(|_| ApplicationError::Internal("hosted Git lease is invalid".into()))?;
        acquired_at
            .checked_add_signed(duration)
            .ok_or_else(|| ApplicationError::Internal("hosted Git lease overflowed".into()))
    }
}

#[derive(Debug, Clone, Copy)]
enum AssetGitAccess {
    Read,
    Write,
}

fn validate_actor_request(actor_id: Uuid, request_id: Uuid) -> ApplicationResult<()> {
    if actor_id.is_nil() || request_id.is_nil() {
        return Err(ApplicationError::Invalid(
            "hosted Git actor and request identities must be UUIDs".into(),
        ));
    }
    Ok(())
}

fn map_repository_error(error: AssetGitRepositoryError) -> ApplicationError {
    match error {
        AssetGitRepositoryError::Invalid(message) => ApplicationError::Invalid(message),
        AssetGitRepositoryError::NotFound => {
            ApplicationError::NotFound("hosted Git repository not found".into())
        }
        AssetGitRepositoryError::QuotaExceeded => {
            ApplicationError::Conflict("hosted Git repository quota exceeded".into())
        }
        AssetGitRepositoryError::BackupUnavailable => {
            ApplicationError::Unavailable("hosted Git backup is unavailable".into())
        }
        AssetGitRepositoryError::Integrity(_) | AssetGitRepositoryError::Storage(_) => {
            ApplicationError::Internal("hosted Git repository operation failed".into())
        }
    }
}

fn map_control_error(error: AssetGitRepositoryControlError) -> ApplicationError {
    match error {
        AssetGitRepositoryControlError::Invalid(message) => ApplicationError::Invalid(message),
        AssetGitRepositoryControlError::NotFound => {
            ApplicationError::NotFound("hosted Git repository not found".into())
        }
        AssetGitRepositoryControlError::Busy => {
            ApplicationError::Conflict("hosted Git repository already has a writer".into())
        }
        AssetGitRepositoryControlError::QuotaExceeded { .. } => {
            ApplicationError::Conflict("hosted Git repository quota exceeded".into())
        }
        AssetGitRepositoryControlError::StaleLease => {
            ApplicationError::Conflict("hosted Git repository write lease is stale".into())
        }
        AssetGitRepositoryControlError::RecoveryRequired => {
            ApplicationError::Unavailable("hosted Git repository requires write recovery".into())
        }
        AssetGitRepositoryControlError::Storage(_) => {
            ApplicationError::Internal("hosted Git repository control failed".into())
        }
    }
}
