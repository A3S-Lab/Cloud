use crate::modules::artifacts::domain::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactStoreError,
};
use crate::modules::assets::domain::{
    Asset, AssetArchived, AssetCreated, AssetGitRepositoryError, AssetKind, AssetRelease,
    AssetReleaseArtifact, AssetReleaseDrafted, AssetReleasePublished, AssetReleaseState,
    AssetReleaseVersion, AssetReleaseWrite, AssetState, AssetWrite, CreateAssetReleaseWrite,
    CreateAssetWrite, IAssetGitRepository, IAssetRepository, TransitionAssetReleaseWrite,
    TransitionAssetWrite, SKILL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, IdempotencyRequest, OrganizationId, ResourceName,
};
use a3s_cloud_contracts::artifact_uri;
use a3s_runtime::contract::ArtifactRef;
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

pub struct AssetCatalogApplicationService {
    organizations: Arc<dyn IOrganizationRepository>,
    assets: Arc<dyn IAssetRepository>,
    repositories: Arc<dyn IAssetGitRepository>,
    artifacts: Arc<dyn INodeArtifactStore>,
}

impl AssetCatalogApplicationService {
    pub fn new(
        organizations: Arc<dyn IOrganizationRepository>,
        assets: Arc<dyn IAssetRepository>,
        repositories: Arc<dyn IAssetGitRepository>,
        artifacts: Arc<dyn INodeArtifactStore>,
    ) -> Self {
        Self {
            organizations,
            assets,
            repositories,
            artifacts,
        }
    }

    pub async fn create_asset(
        &self,
        organization_id: OrganizationId,
        name: String,
        kind: String,
        idempotency_key: String,
        request_id: Uuid,
    ) -> ApplicationResult<AssetWrite> {
        validate_request_id(request_id)?;
        match self.organizations.find(organization_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return Err(ApplicationError::NotFound("organization not found".into())),
            Err(error) => return Err(error.into()),
        }
        let name = ResourceName::parse(name).map_err(ApplicationError::Invalid)?;
        let kind = AssetKind::parse(&kind).map_err(ApplicationError::Invalid)?;
        let idempotency = idempotency(
            format!("organizations/{organization_id}/assets"),
            idempotency_key,
            &serde_json::json!({
                "organization_id": organization_id.as_uuid(),
                "name": name.as_str(),
                "kind": kind.as_str(),
            }),
        )?;
        let asset = Asset::create(AssetId::new(), organization_id, name, kind, Utc::now())
            .map_err(ApplicationError::Invalid)?;
        let event = AssetCreated::envelope(&asset, request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .assets
            .create_asset(CreateAssetWrite {
                asset,
                event,
                idempotency,
            })
            .await
            .map_err(ApplicationError::from)?;
        let repository = self
            .repositories
            .provision(&write.asset)
            .await
            .map_err(map_catalog_repository_error)?;
        repository
            .repository
            .validate_for(&write.asset)
            .map_err(ApplicationError::Internal)?;
        Ok(write)
    }

    pub async fn archive_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        idempotency_key: String,
        request_id: Uuid,
    ) -> ApplicationResult<AssetWrite> {
        validate_request_id(request_id)?;
        let mut asset = self.get_asset(organization_id, asset_id).await?;
        if asset.state == AssetState::Archived {
            return Ok(AssetWrite {
                asset,
                replayed: true,
            });
        }
        let idempotency = idempotency(
            format!("organizations/{organization_id}/assets/{asset_id}/archive"),
            idempotency_key,
            &serde_json::json!({
                "organization_id": organization_id.as_uuid(),
                "asset_id": asset_id.as_uuid(),
            }),
        )?;
        let expected_aggregate_version = asset.aggregate_version;
        asset
            .archive(Utc::now())
            .map_err(ApplicationError::Conflict)?;
        let event = AssetArchived::envelope(&asset, request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        self.assets
            .transition_asset(TransitionAssetWrite {
                asset,
                expected_aggregate_version,
                event,
                idempotency,
            })
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn create_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        version: String,
        commit_sha: String,
        idempotency_key: String,
        request_id: Uuid,
    ) -> ApplicationResult<AssetReleaseWrite> {
        validate_request_id(request_id)?;
        let asset = self.get_asset(organization_id, asset_id).await?;
        if asset.state != AssetState::Active {
            return Err(ApplicationError::Conflict(
                "archived Asset cannot create a release".into(),
            ));
        }
        let version = AssetReleaseVersion::parse(version).map_err(ApplicationError::Invalid)?;
        let commit_sha = GitCommitSha::parse(commit_sha).map_err(ApplicationError::Invalid)?;
        let publication_key = idempotency_key.clone();
        let idempotency = idempotency(
            format!("organizations/{organization_id}/assets/{asset_id}/releases"),
            idempotency_key,
            &serde_json::json!({
                "organization_id": organization_id.as_uuid(),
                "asset_id": asset_id.as_uuid(),
                "version": version.as_str(),
                "commit_sha": commit_sha.as_str(),
            }),
        )?;
        let admission = self
            .repositories
            .admit_manifest(&asset, &commit_sha)
            .await
            .map_err(map_catalog_repository_error)?;
        admission
            .validate_for(asset.kind)
            .map_err(ApplicationError::Conflict)?;
        if admission.commit_sha != commit_sha {
            return Err(ApplicationError::Conflict(
                "Asset manifest admission changed the requested commit".into(),
            ));
        }
        match (asset.kind, admission.build_recipe.is_some()) {
            (AssetKind::Skill, false) | (AssetKind::Agent | AssetKind::Mcp, true) => {}
            (AssetKind::Skill, true) => {
                return Err(ApplicationError::Conflict(
                    "Skill releases cannot contain a Workload build recipe".into(),
                ))
            }
            (AssetKind::Agent | AssetKind::Mcp, false) => {
                return Err(ApplicationError::Conflict(
                    "Agent and MCP releases require a pinned build recipe".into(),
                ))
            }
        }
        let release = AssetRelease::draft(
            &asset,
            AssetReleaseId::new(),
            version,
            commit_sha,
            admission.manifest_digest,
            Utc::now(),
        )
        .map_err(ApplicationError::Conflict)?;
        let event = AssetReleaseDrafted::envelope(&release, request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let drafted = self
            .assets
            .create_release(CreateAssetReleaseWrite {
                release,
                event,
                idempotency,
            })
            .await
            .map_err(ApplicationError::from)?;
        if asset.kind != AssetKind::Skill || drafted.release.state != AssetReleaseState::Draft {
            return Ok(drafted);
        }

        let bundle = self
            .repositories
            .prepare_release_bundle(&asset, &commit_sha, drafted.release.id)
            .await
            .map_err(map_catalog_repository_error)?;
        bundle.validate().map_err(ApplicationError::Conflict)?;
        if bundle.commit_sha != drafted.release.commit_sha {
            return Err(ApplicationError::Conflict(
                "Skill bundle changed the pinned Asset release commit".into(),
            ));
        }
        let artifact = ArtifactRef {
            uri: artifact_uri(bundle.digest.as_str()).map_err(ApplicationError::Invalid)?,
            digest: bundle.digest.as_str().into(),
            media_type: SKILL_BUNDLE_MEDIA_TYPE.into(),
        };
        let descriptor = NodeArtifactDescriptor::new(artifact, bundle.size_bytes)
            .map_err(ApplicationError::Invalid)?;
        let file = tokio::fs::File::open(&bundle.path).await.map_err(|_| {
            ApplicationError::Internal("could not open the prepared Skill bundle".into())
        })?;
        let stored = self
            .artifacts
            .put(&descriptor, Box::pin(file))
            .await
            .map_err(map_artifact_store_error)?;
        if stored.descriptor != descriptor {
            return Err(ApplicationError::Internal(
                "Skill bundle storage changed its immutable descriptor".into(),
            ));
        }
        self.repositories
            .remove_release_bundle(drafted.release.id)
            .await
            .map_err(map_catalog_repository_error)?;

        let mut release = drafted.release;
        let expected_aggregate_version = release.aggregate_version;
        release
            .publish_skill(
                &asset,
                AssetReleaseArtifact::skill_bundle(bundle.digest, bundle.size_bytes)
                    .map_err(ApplicationError::Conflict)?,
                Utc::now(),
            )
            .map_err(ApplicationError::Conflict)?;
        let event = AssetReleasePublished::envelope(&release, request_id)
            .map_err(ApplicationError::Internal)?;
        let publication = idempotency(
            format!(
                "organizations/{organization_id}/assets/{asset_id}/releases/{}/publish-skill",
                release.id
            ),
            publication_key,
            &serde_json::json!({
                "organization_id": organization_id.as_uuid(),
                "asset_id": asset_id.as_uuid(),
                "asset_release_id": release.id.as_uuid(),
                "artifact_digest": release.artifact.as_ref().map(|artifact| artifact.digest().as_str()),
                "artifact_size_bytes": release.artifact.as_ref().map(|artifact| artifact.size_bytes()),
            }),
        )?;
        let mut published = self
            .assets
            .transition_release(TransitionAssetReleaseWrite {
                release,
                expected_aggregate_version,
                event,
                idempotency: publication,
            })
            .await
            .map_err(ApplicationError::from)?;
        published.replayed |= drafted.replayed;
        Ok(published)
    }

    pub async fn yank_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        idempotency_key: String,
        request_id: Uuid,
    ) -> ApplicationResult<AssetReleaseWrite> {
        validate_request_id(request_id)?;
        let asset = self.get_asset(organization_id, asset_id).await?;
        let mut release = self
            .get_release(organization_id, asset_id, asset_release_id)
            .await?;
        if release.state == AssetReleaseState::Yanked {
            return Ok(AssetReleaseWrite {
                asset,
                release,
                replayed: true,
            });
        }
        let idempotency = idempotency(
            format!(
                "organizations/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/yank"
            ),
            idempotency_key,
            &serde_json::json!({
                "organization_id": organization_id.as_uuid(),
                "asset_id": asset_id.as_uuid(),
                "asset_release_id": asset_release_id.as_uuid(),
            }),
        )?;
        let expected_aggregate_version = release.aggregate_version;
        release
            .yank(Utc::now())
            .map_err(ApplicationError::Conflict)?;
        let event =
            crate::modules::assets::domain::AssetReleaseYanked::envelope(&release, request_id)
                .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        self.assets
            .transition_release(TransitionAssetReleaseWrite {
                release,
                expected_aggregate_version,
                event,
                idempotency,
            })
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn get_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> ApplicationResult<Asset> {
        self.assets
            .find_asset(organization_id, asset_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("Asset not found".into()))
    }

    pub async fn list_assets(
        &self,
        organization_id: OrganizationId,
    ) -> ApplicationResult<Vec<Asset>> {
        self.assets
            .list_assets(organization_id)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn get_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> ApplicationResult<AssetRelease> {
        self.assets
            .find_release(organization_id, asset_id, asset_release_id)
            .await
            .map_err(ApplicationError::from)?
            .ok_or_else(|| ApplicationError::NotFound("Asset release not found".into()))
    }

    pub async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> ApplicationResult<Vec<AssetRelease>> {
        self.get_asset(organization_id, asset_id).await?;
        self.assets
            .list_releases(organization_id, asset_id)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn select_release(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        requested_version: Option<String>,
    ) -> ApplicationResult<AssetRelease> {
        let asset = self.get_asset(organization_id, asset_id).await?;
        let requested_version = requested_version
            .map(AssetReleaseVersion::parse)
            .transpose()
            .map_err(ApplicationError::Invalid)?;
        let releases = self
            .assets
            .list_releases(organization_id, asset_id)
            .await
            .map_err(ApplicationError::from)?;
        AssetRelease::select_for_new_binding(&asset, releases, requested_version.as_ref())
            .map_err(ApplicationError::Internal)?
            .ok_or_else(|| ApplicationError::NotFound("no selectable Asset release found".into()))
    }
}

fn validate_request_id(request_id: Uuid) -> ApplicationResult<()> {
    if request_id.is_nil() {
        return Err(ApplicationError::Invalid(
            "request identity must be a UUID".into(),
        ));
    }
    Ok(())
}

fn idempotency<T: Serialize>(
    scope: String,
    key: String,
    input: &T,
) -> ApplicationResult<IdempotencyRequest> {
    let canonical =
        serde_json::to_vec(input).map_err(|error| ApplicationError::Internal(error.to_string()))?;
    IdempotencyRequest::new(scope, key, &canonical).map_err(ApplicationError::Invalid)
}

fn map_catalog_repository_error(error: AssetGitRepositoryError) -> ApplicationError {
    match error {
        AssetGitRepositoryError::Invalid(message) => ApplicationError::Invalid(message),
        AssetGitRepositoryError::NotFound => {
            ApplicationError::NotFound("hosted Git repository or commit not found".into())
        }
        AssetGitRepositoryError::Integrity(_) => ApplicationError::Conflict(
            "Asset manifest admission failed integrity validation".into(),
        ),
        AssetGitRepositoryError::QuotaExceeded => {
            ApplicationError::Conflict("hosted Git repository quota exceeded".into())
        }
        AssetGitRepositoryError::BackupUnavailable => {
            ApplicationError::Unavailable("hosted Git repository is unavailable".into())
        }
        AssetGitRepositoryError::Storage(_) => {
            ApplicationError::Internal("hosted Git repository operation failed".into())
        }
    }
}

fn map_artifact_store_error(error: NodeArtifactStoreError) -> ApplicationError {
    match error {
        NodeArtifactStoreError::Invalid(message) => ApplicationError::Invalid(message),
        NodeArtifactStoreError::NotFound => {
            ApplicationError::NotFound("Skill bundle storage was not found".into())
        }
        NodeArtifactStoreError::Conflict | NodeArtifactStoreError::Integrity(_) => {
            ApplicationError::Conflict("Skill bundle failed immutable storage admission".into())
        }
        NodeArtifactStoreError::Storage(_) => {
            ApplicationError::Internal("Skill bundle storage failed".into())
        }
    }
}
