use super::AssetCatalogApplicationService;
use crate::modules::artifacts::domain::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactReader, NodeArtifactStoreError,
    NodeArtifactWrite, OpenNodeArtifact,
};
use crate::modules::assets::domain::{
    Asset, AssetGitBackup, AssetGitBuildInput, AssetGitReleaseBundle, AssetGitRepository,
    AssetGitRepositoryError, AssetGitRepositoryWrite, AssetGitRpcLimits, AssetGitRpcResponse,
    AssetGitService, AssetGitWriteJournal, AssetGitWriteLease, AssetManifestAdmission,
    AssetRelease, AssetReleaseArtifact, AssetReleaseState, AssetReleaseVersion, AssetReleaseWrite,
    AssetWrite, CreateAssetReleaseWrite, CreateAssetWrite, IAssetGitRepository, IAssetRepository,
    TransitionAssetReleaseWrite, TransitionAssetWrite, SKILL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::identity::domain::entities::Organization;
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::identity::domain::value_objects::OrganizationName;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, GitCommitSha, IdempotencyRequest, IdempotentWrite,
    OrganizationId, RepositoryError, Sha256Digest,
};
use crate::modules::sources::domain::BuildRecipe;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

struct TestOrganizations {
    organization: Organization,
}

#[async_trait]
impl IOrganizationRepository for TestOrganizations {
    async fn create(
        &self,
        _organization: Organization,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
        Err(RepositoryError::Storage("unused organization write".into()))
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<Organization>, RepositoryError> {
        Ok((self.organization.id == organization_id).then(|| self.organization.clone()))
    }

    async fn list(&self) -> Result<Vec<Organization>, RepositoryError> {
        Ok(vec![self.organization.clone()])
    }
}

#[derive(Default)]
struct CatalogState {
    assets: BTreeMap<(OrganizationId, AssetId), Asset>,
    releases: BTreeMap<(OrganizationId, AssetId, AssetReleaseId), AssetRelease>,
    provisioned: BTreeSet<AssetId>,
    artifacts: BTreeMap<String, NodeArtifactDescriptor>,
    bundle_paths: BTreeMap<AssetReleaseId, PathBuf>,
}

#[derive(Default)]
struct CatalogStore {
    state: Mutex<CatalogState>,
}

impl CatalogStore {
    fn state(&self) -> std::sync::MutexGuard<'_, CatalogState> {
        self.state.lock().expect("catalog state")
    }

    fn seed_release(&self, release: AssetRelease) {
        self.state().releases.insert(
            (release.organization_id, release.asset_id, release.id),
            release,
        );
    }
}

#[async_trait]
impl IAssetRepository for CatalogStore {
    async fn create_asset(&self, bundle: CreateAssetWrite) -> Result<AssetWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Storage)?;
        self.state().assets.insert(
            (bundle.asset.organization_id, bundle.asset.id),
            bundle.asset.clone(),
        );
        Ok(AssetWrite {
            asset: bundle.asset,
            replayed: false,
        })
    }

    async fn transition_asset(
        &self,
        bundle: TransitionAssetWrite,
    ) -> Result<AssetWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state();
        let key = (bundle.asset.organization_id, bundle.asset.id);
        let existing = state.assets.get(&key).ok_or(RepositoryError::NotFound)?;
        bundle
            .validate_against(existing)
            .map_err(RepositoryError::Conflict)?;
        state.assets.insert(key, bundle.asset.clone());
        Ok(AssetWrite {
            asset: bundle.asset,
            replayed: false,
        })
    }

    async fn find_asset(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Option<Asset>, RepositoryError> {
        Ok(self
            .state()
            .assets
            .get(&(organization_id, asset_id))
            .cloned())
    }

    async fn list_assets(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<Asset>, RepositoryError> {
        Ok(self
            .state()
            .assets
            .values()
            .filter(|asset| asset.organization_id == organization_id)
            .cloned()
            .collect())
    }

    async fn create_release(
        &self,
        bundle: CreateAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state();
        let asset = state
            .assets
            .get(&(bundle.release.organization_id, bundle.release.asset_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        bundle
            .release
            .validate_for(&asset)
            .map_err(RepositoryError::Conflict)?;
        state.releases.insert(
            (
                bundle.release.organization_id,
                bundle.release.asset_id,
                bundle.release.id,
            ),
            bundle.release.clone(),
        );
        Ok(AssetReleaseWrite {
            asset,
            release: bundle.release,
            replayed: false,
        })
    }

    async fn transition_release(
        &self,
        bundle: TransitionAssetReleaseWrite,
    ) -> Result<AssetReleaseWrite, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state();
        let asset = state
            .assets
            .get(&(bundle.release.organization_id, bundle.release.asset_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let key = (
            bundle.release.organization_id,
            bundle.release.asset_id,
            bundle.release.id,
        );
        let existing = state.releases.get(&key).ok_or(RepositoryError::NotFound)?;
        bundle
            .validate_against(existing, &asset)
            .map_err(RepositoryError::Conflict)?;
        state.releases.insert(key, bundle.release.clone());
        Ok(AssetReleaseWrite {
            asset,
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
        Ok(self
            .state()
            .releases
            .get(&(organization_id, asset_id, asset_release_id))
            .cloned())
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
    ) -> Result<Vec<AssetRelease>, RepositoryError> {
        Ok(self
            .state()
            .releases
            .values()
            .filter(|release| {
                release.organization_id == organization_id && release.asset_id == asset_id
            })
            .cloned()
            .collect())
    }
}

#[async_trait]
impl IAssetGitRepository for CatalogStore {
    async fn provision(
        &self,
        asset: &Asset,
    ) -> Result<AssetGitRepositoryWrite, AssetGitRepositoryError> {
        let created = self.state().provisioned.insert(asset.id);
        Ok(AssetGitRepositoryWrite {
            repository: AssetGitRepository::for_asset(asset)
                .map_err(AssetGitRepositoryError::Invalid)?,
            created,
        })
    }

    async fn inspect(&self, _asset: &Asset) -> Result<AssetGitRepository, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn prepare_write(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn rollback_write(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn settle_write(
        &self,
        _asset: &Asset,
        _journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn advertise(
        &self,
        _asset: &Asset,
        _service: AssetGitService,
    ) -> Result<Vec<u8>, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn execute_rpc(
        &self,
        _asset: &Asset,
        _service: AssetGitService,
        _request: Vec<u8>,
        _limits: AssetGitRpcLimits,
        _write_lease: Option<&AssetGitWriteLease>,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn repository_bytes(&self, _asset: &Asset) -> Result<u64, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn refs_digest(&self, _asset: &Asset) -> Result<Sha256Digest, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn create_backup(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
        _created_at: DateTime<Utc>,
    ) -> Result<AssetGitBackup, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn restore_backup(
        &self,
        _asset: &Asset,
        _lease: &AssetGitWriteLease,
        _backup: &AssetGitBackup,
        _maximum_repository_bytes: u64,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
        Err(unused_git())
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
            build_recipe: (asset.kind != crate::modules::assets::domain::AssetKind::Skill)
                .then(|| {
                    BuildRecipe::dockerfile(
                        BuildRecipe::SCHEMA,
                        BuildRecipe::DOCKERFILE_KIND,
                        ".",
                        "Dockerfile",
                        None,
                        vec!["linux/amd64".into()],
                    )
                })
                .transpose()
                .map_err(AssetGitRepositoryError::Invalid)?,
        })
    }

    async fn prepare_build_input(
        &self,
        _asset: &Asset,
        _commit_sha: &GitCommitSha,
        _build_run_id: BuildRunId,
    ) -> Result<AssetGitBuildInput, AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn remove_build_input(
        &self,
        _build_run_id: BuildRunId,
    ) -> Result<(), AssetGitRepositoryError> {
        Err(unused_git())
    }

    async fn prepare_release_bundle(
        &self,
        _asset: &Asset,
        commit_sha: &GitCommitSha,
        asset_release_id: AssetReleaseId,
    ) -> Result<AssetGitReleaseBundle, AssetGitRepositoryError> {
        let bytes = format!("Skill bundle for {commit_sha}\n").into_bytes();
        let digest = Sha256Digest::parse(format!("sha256:{:x}", sha2::Sha256::digest(&bytes)))
            .map_err(AssetGitRepositoryError::Invalid)?;
        let path = std::env::temp_dir().join(format!(
            "a3s-cloud-skill-bundle-{asset_release_id}-{}.tar",
            Uuid::now_v7()
        ));
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|error| AssetGitRepositoryError::Storage(error.to_string()))?;
        self.state()
            .bundle_paths
            .insert(asset_release_id, path.clone());
        Ok(AssetGitReleaseBundle {
            asset_release_id,
            commit_sha: commit_sha.clone(),
            digest,
            size_bytes: bytes.len() as u64,
            path,
        })
    }

    async fn remove_release_bundle(
        &self,
        asset_release_id: AssetReleaseId,
    ) -> Result<(), AssetGitRepositoryError> {
        let path = self.state().bundle_paths.remove(&asset_release_id);
        if let Some(path) = path {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| AssetGitRepositoryError::Storage(error.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl INodeArtifactStore for CatalogStore {
    async fn put(
        &self,
        descriptor: &NodeArtifactDescriptor,
        mut reader: NodeArtifactReader,
    ) -> Result<NodeArtifactWrite, NodeArtifactStoreError> {
        descriptor
            .validate()
            .map_err(NodeArtifactStoreError::Invalid)?;
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| NodeArtifactStoreError::Storage(error.to_string()))?;
        let actual = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
        if actual != descriptor.artifact.digest || bytes.len() as u64 != descriptor.size_bytes {
            return Err(NodeArtifactStoreError::Integrity(
                "test Artifact bytes changed their descriptor".into(),
            ));
        }
        let replayed = self
            .state()
            .artifacts
            .insert(descriptor.artifact.digest.clone(), descriptor.clone())
            .is_some();
        Ok(NodeArtifactWrite {
            descriptor: descriptor.clone(),
            replayed,
        })
    }

    async fn open(
        &self,
        _artifact: &ArtifactRef,
    ) -> Result<OpenNodeArtifact, NodeArtifactStoreError> {
        Err(NodeArtifactStoreError::NotFound)
    }
}

fn unused_git() -> AssetGitRepositoryError {
    AssetGitRepositoryError::Storage("unused test Git operation".into())
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn service() -> (
    OrganizationId,
    Arc<CatalogStore>,
    AssetCatalogApplicationService,
) {
    let organization_id = OrganizationId::new();
    let organizations = Arc::new(TestOrganizations {
        organization: Organization::create(
            organization_id,
            OrganizationName::parse("Catalog Tests").expect("organization name"),
            Utc::now(),
        ),
    });
    let store = Arc::new(CatalogStore::default());
    let service = AssetCatalogApplicationService::new(
        organizations,
        store.clone(),
        store.clone(),
        store.clone(),
    );
    (organization_id, store, service)
}

#[tokio::test]
async fn hosted_release_uses_the_admitted_manifest_digest() {
    let (organization_id, store, service) = service();
    let asset = service
        .create_asset(
            organization_id,
            "catalog-agent".into(),
            "agent".into(),
            "create-agent".into(),
            Uuid::now_v7(),
        )
        .await
        .expect("create Agent")
        .asset;
    assert!(store.state().provisioned.contains(&asset.id));

    let release = service
        .create_release(
            organization_id,
            asset.id,
            "1.0.0".into(),
            "a".repeat(40),
            "create-release".into(),
            Uuid::now_v7(),
        )
        .await
        .expect("create hosted release")
        .release;
    assert_eq!(release.manifest_digest, digest('b'));
    assert_eq!(release.commit_sha.as_str(), "a".repeat(40));
}

#[tokio::test]
async fn skill_release_publishes_the_exact_git_bundle_without_a_build_run() {
    let (organization_id, store, service) = service();
    let asset = service
        .create_asset(
            organization_id,
            "catalog-skill".into(),
            "skill".into(),
            "create-skill".into(),
            Uuid::now_v7(),
        )
        .await
        .expect("create Skill")
        .asset;

    let release = service
        .create_release(
            organization_id,
            asset.id,
            "1.0.0".into(),
            "c".repeat(40),
            "publish-skill".into(),
            Uuid::now_v7(),
        )
        .await
        .expect("publish Skill release")
        .release;

    assert_eq!(release.state, AssetReleaseState::Published);
    assert!(release.provenance.is_none());
    let artifact = release.artifact.expect("Skill artifact");
    assert_eq!(artifact.media_type(), SKILL_BUNDLE_MEDIA_TYPE);
    assert_eq!(artifact.kind().as_str(), "skill_bundle");
    assert_eq!(
        store
            .state()
            .artifacts
            .get(artifact.digest().as_str())
            .expect("stored bundle")
            .artifact
            .media_type,
        SKILL_BUNDLE_MEDIA_TYPE
    );
    assert!(store.state().bundle_paths.is_empty());
}

#[tokio::test]
async fn yanked_release_remains_exactly_addressable_but_is_not_selectable() {
    let (organization_id, store, service) = service();
    let asset = service
        .create_asset(
            organization_id,
            "catalog-skill".into(),
            "skill".into(),
            "create-skill".into(),
            Uuid::now_v7(),
        )
        .await
        .expect("create Skill")
        .asset;
    let created_at = Utc::now() - Duration::seconds(2);
    let mut release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0").expect("version"),
        GitCommitSha::parse("c".repeat(40)).expect("commit"),
        digest('d'),
        created_at,
    )
    .expect("draft release");
    release
        .publish_skill(
            &asset,
            AssetReleaseArtifact::skill_bundle(digest('e'), 1024).expect("Skill bundle"),
            created_at + Duration::seconds(1),
        )
        .expect("publish Skill release");
    let release_id = release.id;
    store.seed_release(release);

    let yanked = service
        .yank_release(
            organization_id,
            asset.id,
            release_id,
            "yank-release".into(),
            Uuid::now_v7(),
        )
        .await
        .expect("yank release")
        .release;
    assert_eq!(
        yanked.state,
        crate::modules::assets::domain::AssetReleaseState::Yanked
    );
    assert_eq!(
        service
            .get_release(organization_id, asset.id, release_id)
            .await
            .expect("exact release")
            .id,
        release_id
    );
    assert!(matches!(
        service
            .select_release(organization_id, asset.id, None)
            .await,
        Err(ApplicationError::NotFound(_))
    ));
}
