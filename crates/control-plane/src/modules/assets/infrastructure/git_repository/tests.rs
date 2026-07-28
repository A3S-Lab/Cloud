use super::*;
use crate::modules::assets::domain::{AssetKind, AssetState};
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId, ResourceName};
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::Arc;

fn asset_with(id: AssetId, organization_id: OrganizationId, name: &str) -> Asset {
    Asset::create(
        id,
        organization_id,
        ResourceName::parse(name).expect("Asset name"),
        AssetKind::Agent,
        Utc::now(),
    )
    .expect("Asset")
}

fn store(root: &Path) -> LocalAssetGitRepository {
    LocalAssetGitRepository::new(root, Duration::from_secs(10)).expect("Git repository store")
}

#[tokio::test]
async fn concurrent_provisioning_creates_one_asset_id_addressed_bare_repository() {
    let directory = tempfile::tempdir().expect("repository directory");
    let store = Arc::new(store(directory.path()));
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Research Assistant");
    let (left, right) = tokio::join!(store.provision(&asset), store.provision(&asset));
    let writes = [
        left.expect("first provision"),
        right.expect("second provision"),
    ];
    assert_eq!(writes.iter().filter(|write| write.created).count(), 1);
    assert!(writes.iter().all(|write| {
        write.repository.asset_id() == asset.id
            && write.repository.organization_id() == asset.organization_id
            && write.repository.default_branch() == DEFAULT_ASSET_BRANCH
    }));
    assert_eq!(
        store.inspect(&asset).await.expect("inspect repository"),
        writes[0].repository
    );
    let path = store.repository_path(&asset);
    assert!(path.is_dir());
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some(format!("{}.git", asset.id).as_str())
    );
    assert!(!path.to_string_lossy().contains(asset.name.as_str()));
    assert_eq!(
        std::fs::read_dir(&store.staging_root)
            .expect("staging directory")
            .count(),
        0
    );
}

#[tokio::test]
async fn organization_namespace_prevents_cross_tenant_asset_id_collision() {
    let directory = tempfile::tempdir().expect("repository directory");
    let store = store(directory.path());
    let asset_id = AssetId::new();
    let first = asset_with(asset_id, OrganizationId::new(), "First");
    let second = asset_with(asset_id, OrganizationId::new(), "Second");
    store.provision(&first).await.expect("first repository");
    store.provision(&second).await.expect("second repository");
    assert_ne!(
        store.repository_path(&first),
        store.repository_path(&second)
    );
    assert_eq!(
        store
            .inspect(&first)
            .await
            .expect("first inspection")
            .asset_id(),
        asset_id
    );
    assert_eq!(
        store
            .inspect(&second)
            .await
            .expect("second inspection")
            .asset_id(),
        asset_id
    );
}

#[tokio::test]
async fn archived_assets_cannot_create_repositories_but_existing_repositories_remain_readable() {
    let directory = tempfile::tempdir().expect("repository directory");
    let store = store(directory.path());
    let mut existing = asset_with(AssetId::new(), OrganizationId::new(), "Existing");
    store
        .provision(&existing)
        .await
        .expect("existing repository");
    existing
        .archive(existing.updated_at + ChronoDuration::seconds(1))
        .expect("archive Asset");
    assert_eq!(existing.state, AssetState::Archived);
    store
        .inspect(&existing)
        .await
        .expect("archived repository remains readable");

    let mut missing = asset_with(AssetId::new(), OrganizationId::new(), "Missing");
    missing
        .archive(missing.updated_at + ChronoDuration::seconds(1))
        .expect("archive Asset");
    assert!(matches!(
        store.provision(&missing).await,
        Err(AssetGitRepositoryError::Invalid(_))
    ));
    assert_eq!(
        store.inspect(&missing).await,
        Err(AssetGitRepositoryError::NotFound)
    );
}

#[tokio::test]
async fn changed_repository_identity_fails_closed() {
    let directory = tempfile::tempdir().expect("repository directory");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Identity");
    store.provision(&asset).await.expect("repository");
    let config = store.repository_path(&asset).join("config");
    let changed = std::fs::read_to_string(&config)
        .expect("read config")
        .replace(&asset.id.to_string(), &AssetId::new().to_string());
    std::fs::write(config, changed).expect("change config");
    assert!(matches!(
        store.inspect(&asset).await,
        Err(AssetGitRepositoryError::Integrity(_))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_tenant_and_repository_paths_fail_closed() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().expect("repository directory");
    let outside = tempfile::tempdir().expect("outside directory");
    let store = store(directory.path());
    let tenant_symlink = asset_with(AssetId::new(), OrganizationId::new(), "Tenant link");
    symlink(outside.path(), store.organization_path(&tenant_symlink)).expect("tenant symlink");
    assert!(matches!(
        store.provision(&tenant_symlink).await,
        Err(AssetGitRepositoryError::Integrity(_))
    ));

    let repository_symlink = asset_with(AssetId::new(), OrganizationId::new(), "Repository link");
    std::fs::create_dir(store.organization_path(&repository_symlink))
        .expect("organization directory");
    symlink(outside.path(), store.repository_path(&repository_symlink))
        .expect("repository symlink");
    assert!(matches!(
        store.inspect(&repository_symlink).await,
        Err(AssetGitRepositoryError::Integrity(_))
    ));
}
