use super::*;
use crate::infrastructure::ImmutableObjectClient;
use crate::modules::assets::domain::{
    AssetGitRepositoryError, AssetGitRpcLimits, AssetGitService, AssetGitWriteLease,
    AssetGitWriteOperation, AssetKind, AssetState, IAssetGitRepository,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, GitCommitSha, OrganizationId, ResourceName,
};
use chrono::{Duration as ChronoDuration, Utc};
use std::io::Write;
use std::process::{Command, Stdio};
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

fn store_with_backups(root: &Path, object_root: &Path) -> LocalAssetGitRepository {
    store(root)
        .with_backup_objects(
            ImmutableObjectClient::local(object_root, "asset-git-backups")
                .expect("backup object client"),
            32 * 1024 * 1024,
        )
        .expect("backup-enabled repository store")
}

fn write_lease(asset: &Asset, operation: AssetGitWriteOperation) -> AssetGitWriteLease {
    AssetGitWriteLease {
        organization_id: asset.organization_id,
        asset_id: asset.id,
        lease_id: uuid::Uuid::now_v7(),
        operation,
        actor_id: uuid::Uuid::now_v7(),
        request_id: uuid::Uuid::now_v7(),
        quota_bytes: 32 * 1024 * 1024,
        observed_bytes: 0,
        leased_until: Utc::now() + ChronoDuration::minutes(1),
        recovery: false,
    }
}

#[test]
fn repository_storage_identity_is_root_specific_and_persistent() {
    let first_root = tempfile::tempdir().expect("first repository root");
    let second_root = tempfile::tempdir().expect("second repository root");

    let first = store(first_root.path());
    let restarted = store(first_root.path());
    assert_eq!(restarted.storage_id, first.storage_id);
    let split_replica = store(second_root.path());
    assert_ne!(split_replica.storage_id, first.storage_id);
}

#[test]
fn repository_storage_identity_is_persistent_bounded_and_fail_closed() {
    let directory = tempfile::tempdir().expect("repository root");
    let initial = store(directory.path());
    let storage_id = initial.storage_id;
    drop(initial);
    assert_eq!(store(directory.path()).storage_id, storage_id);

    std::fs::write(
        directory
            .path()
            .join(STORAGE_IDENTITY_DIRECTORY)
            .join(STORAGE_IDENTITY_FILE),
        b"{}",
    )
    .expect("corrupt storage identity");
    assert!(matches!(
        LocalAssetGitRepository::new(directory.path(), Duration::from_secs(10)),
        Err(AssetGitRepositoryError::Integrity(_))
    ));
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

#[tokio::test]
async fn smart_receive_pack_enforces_input_bound_and_publishes_valid_refs() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Smart HTTP");
    store.provision(&asset).await.expect("repository");
    initialize_work_tree(work.path(), asset.kind, "first");
    let raw_advertisement = store
        .git(vec![
            "receive-pack".into(),
            "--stateless-rpc".into(),
            "--advertise-refs".into(),
            store.repository_path(&asset).as_os_str().to_owned(),
        ])
        .await
        .expect("receive-pack advertisement");
    let request = send_pack_request(
        work.path(),
        &store.repository_path(&asset),
        &raw_advertisement,
    );
    assert!(!request.is_empty());
    let rejected_lease = write_lease(&asset, AssetGitWriteOperation::ReceivePack);
    store
        .prepare_write(&asset, &rejected_lease)
        .await
        .expect("rejected write journal");
    assert_eq!(
        store
            .execute_rpc(
                &asset,
                AssetGitService::ReceivePack,
                request.clone(),
                AssetGitRpcLimits {
                    maximum_input_bytes: request.len() as u64 - 1,
                    maximum_repository_bytes: 32 * 1024 * 1024,
                },
                Some(&rejected_lease),
            )
            .await,
        Err(AssetGitRepositoryError::QuotaExceeded)
    );
    store
        .rollback_write(&asset, &rejected_lease)
        .await
        .expect("reject oversized request");
    let accepted_lease = write_lease(&asset, AssetGitWriteOperation::ReceivePack);
    store
        .prepare_write(&asset, &accepted_lease)
        .await
        .expect("accepted write journal");
    let response = store
        .execute_rpc(
            &asset,
            AssetGitService::ReceivePack,
            request.clone(),
            AssetGitRpcLimits {
                maximum_input_bytes: request.len() as u64,
                maximum_repository_bytes: 32 * 1024 * 1024,
            },
            Some(&accepted_lease),
        )
        .await
        .expect("receive pack");
    store
        .settle_write(&asset, &accepted_lease.journal())
        .await
        .expect("settle accepted write");
    assert!(!response.body.is_empty());
    assert!(response.repository_bytes > request.len() as u64);
    assert_eq!(
        git_output(
            directory.path(),
            &[
                format!("--git-dir={}", store.repository_path(&asset).display()).as_str(),
                "rev-parse",
                "refs/heads/main",
            ],
        ),
        git_output(work.path(), &["rev-parse", "HEAD"])
    );
    let advertisement = store
        .advertise(&asset, AssetGitService::UploadPack)
        .await
        .expect("upload-pack advertisement");
    assert!(advertisement.starts_with(b"001e# service=git-upload-pack\n0000"));
}

#[tokio::test]
async fn smart_receive_pack_rolls_back_refs_and_objects_when_repository_quota_is_exceeded() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Quota rollback");
    store.provision(&asset).await.expect("repository");
    let original_bytes = store
        .repository_bytes(&asset)
        .await
        .expect("repository size");
    let original_refs = store.refs_digest(&asset).await.expect("reference digest");
    initialize_work_tree(work.path(), asset.kind, "quota rollback");
    let raw_advertisement = store
        .git(vec![
            "receive-pack".into(),
            "--stateless-rpc".into(),
            "--advertise-refs".into(),
            store.repository_path(&asset).as_os_str().to_owned(),
        ])
        .await
        .expect("receive-pack advertisement");
    let request = send_pack_request(
        work.path(),
        &store.repository_path(&asset),
        &raw_advertisement,
    );

    let lease = write_lease(&asset, AssetGitWriteOperation::ReceivePack);
    store
        .prepare_write(&asset, &lease)
        .await
        .expect("quota rollback journal");

    assert_eq!(
        store
            .execute_rpc(
                &asset,
                AssetGitService::ReceivePack,
                request.clone(),
                AssetGitRpcLimits {
                    maximum_input_bytes: request.len() as u64,
                    maximum_repository_bytes: original_bytes,
                },
                Some(&lease),
            )
            .await,
        Err(AssetGitRepositoryError::QuotaExceeded)
    );
    store
        .rollback_write(&asset, &lease)
        .await
        .expect("quota rollback");
    assert_eq!(
        store.refs_digest(&asset).await.expect("rolled-back refs"),
        original_refs
    );
    assert!(
        store
            .repository_bytes(&asset)
            .await
            .expect("rolled-back size")
            <= original_bytes
    );
}

#[tokio::test]
async fn receive_pack_rejects_missing_or_unprepared_write_journals() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Journal admission");
    store.provision(&asset).await.expect("repository");
    let original_refs = store.refs_digest(&asset).await.expect("original refs");
    initialize_work_tree(work.path(), asset.kind, "journal admission");
    let advertisement = store
        .git(vec![
            "receive-pack".into(),
            "--stateless-rpc".into(),
            "--advertise-refs".into(),
            store.repository_path(&asset).as_os_str().to_owned(),
        ])
        .await
        .expect("receive-pack advertisement");
    let request = send_pack_request(work.path(), &store.repository_path(&asset), &advertisement);
    let limits = AssetGitRpcLimits {
        maximum_input_bytes: 32 * 1024 * 1024,
        maximum_repository_bytes: 32 * 1024 * 1024,
    };
    assert!(matches!(
        store
            .execute_rpc(
                &asset,
                AssetGitService::ReceivePack,
                request.clone(),
                limits,
                None,
            )
            .await,
        Err(AssetGitRepositoryError::Invalid(_))
    ));
    let unprepared = write_lease(&asset, AssetGitWriteOperation::ReceivePack);
    assert!(matches!(
        store
            .execute_rpc(
                &asset,
                AssetGitService::ReceivePack,
                request,
                limits,
                Some(&unprepared),
            )
            .await,
        Err(AssetGitRepositoryError::Integrity(_))
    ));
    assert_eq!(
        store.refs_digest(&asset).await.expect("unchanged refs"),
        original_refs
    );
}

#[tokio::test]
async fn corrupted_write_journal_fails_closed_and_preserves_recovery_evidence() {
    let directory = tempfile::tempdir().expect("repository directory");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Corrupted journal");
    store.provision(&asset).await.expect("repository");
    let original_refs = store.refs_digest(&asset).await.expect("original refs");
    let lease = write_lease(&asset, AssetGitWriteOperation::ReceivePack);
    store
        .prepare_write(&asset, &lease)
        .await
        .expect("prepared write journal");

    let journal_path = store
        .staging_root
        .join(format!("{}.asset-git-write.json", lease.lease_id));
    let bytes = tokio::fs::read(&journal_path)
        .await
        .expect("read write journal");
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&bytes).expect("decode write journal");
    envelope["checksum"] = serde_json::Value::String(format!("sha256:{}", "0".repeat(64)));
    tokio::fs::write(
        &journal_path,
        serde_json::to_vec(&envelope).expect("encode corrupted write journal"),
    )
    .await
    .expect("corrupt write journal");

    assert!(matches!(
        store.rollback_write(&asset, &lease).await,
        Err(AssetGitRepositoryError::Integrity(message))
            if message.contains("checksum changed")
    ));
    assert!(
        tokio::fs::metadata(&journal_path).await.is_ok(),
        "corrupt recovery evidence must remain for operator repair"
    );
    assert_eq!(
        store.refs_digest(&asset).await.expect("unchanged refs"),
        original_refs
    );
}

#[tokio::test]
async fn pinned_asset_acl_uses_a3s_acl_and_rejects_kind_drift() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Manifest");
    store.provision(&asset).await.expect("repository");
    initialize_work_tree(work.path(), asset.kind, "valid");
    push_main(work.path(), &store.repository_path(&asset));
    let first = commit(work.path());
    let admitted = store
        .admit_manifest(&asset, &first)
        .await
        .expect("admitted manifest");
    assert_eq!(admitted.commit_sha, first);
    assert_eq!(admitted.kind, AssetKind::Agent);

    write_manifest(work.path(), AssetKind::Skill);
    git_success(work.path(), &["add", ".a3s/asset.acl"]);
    git_success(work.path(), &["commit", "--quiet", "-m", "kind drift"]);
    push_main(work.path(), &store.repository_path(&asset));
    assert!(matches!(
        store.admit_manifest(&asset, &commit(work.path())).await,
        Err(AssetGitRepositoryError::Integrity(_))
    ));
}

#[tokio::test]
async fn pinned_build_manifest_and_git_archive_share_one_immutable_commit() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store(directory.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Build Input");
    store.provision(&asset).await.expect("repository");
    initialize_work_tree(work.path(), asset.kind, "hosted source");
    std::fs::write(work.path().join("Dockerfile"), "FROM scratch\n").expect("Dockerfile");
    write_build_manifest(work.path(), asset.kind);
    git_success(work.path(), &["add", "."]);
    git_success(
        work.path(),
        &["commit", "--quiet", "-m", "add build contract"],
    );
    push_main(work.path(), &store.repository_path(&asset));
    let source_commit = commit(work.path());

    let admitted = store
        .admit_manifest(&asset, &source_commit)
        .await
        .expect("admitted build manifest");
    let recipe = admitted.build_recipe.expect("build recipe");
    assert_eq!(recipe.context_path(), ".");
    assert_eq!(recipe.dockerfile_path(), "Dockerfile");
    assert_eq!(recipe.target(), Some("release"));
    assert_eq!(recipe.platforms()[0].as_str(), "linux/amd64");

    let build_run_id = BuildRunId::new();
    let first = store
        .prepare_build_input(&asset, &source_commit, build_run_id)
        .await
        .expect("prepared build archive");
    let replay = store
        .prepare_build_input(&asset, &source_commit, build_run_id)
        .await
        .expect("replayed build archive");
    assert_eq!(first, replay);
    assert_eq!(first.commit_sha, admitted.commit_sha);
    assert!(first.path.is_file());

    store
        .remove_build_input(build_run_id)
        .await
        .expect("removed build archive");
    store
        .remove_build_input(build_run_id)
        .await
        .expect("idempotent removal");
    assert!(!first.path.exists());
}

#[tokio::test]
async fn pinned_skill_release_bundle_replays_exact_git_archive_and_cleans_up_idempotently() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store(directory.path());
    let asset = Asset::create(
        AssetId::new(),
        OrganizationId::new(),
        ResourceName::parse("Release bundle").expect("Asset name"),
        AssetKind::Skill,
        Utc::now(),
    )
    .expect("Skill Asset");
    store.provision(&asset).await.expect("repository");
    initialize_work_tree(work.path(), asset.kind, "skill source");
    push_main(work.path(), &store.repository_path(&asset));
    let source_commit = commit(work.path());
    let admission = store
        .admit_manifest(&asset, &source_commit)
        .await
        .expect("admitted Skill manifest");
    assert!(admission.build_recipe.is_none());

    let release_id = AssetReleaseId::new();
    let first = store
        .prepare_release_bundle(&asset, &source_commit, release_id)
        .await
        .expect("prepared Skill release bundle");
    let replay = store
        .prepare_release_bundle(&asset, &source_commit, release_id)
        .await
        .expect("replayed Skill release bundle");
    assert_eq!(first, replay);
    assert_eq!(first.asset_release_id, release_id);
    assert_eq!(first.commit_sha, admission.commit_sha);
    assert!(first.path.is_file());

    store
        .remove_release_bundle(release_id)
        .await
        .expect("removed Skill release bundle");
    store
        .remove_release_bundle(release_id)
        .await
        .expect("idempotent Skill bundle removal");
    assert!(!first.path.exists());
}

#[tokio::test]
async fn immutable_bundle_restore_atomically_reproduces_advertised_refs() {
    let directory = tempfile::tempdir().expect("repository directory");
    let objects = tempfile::tempdir().expect("object directory");
    let work = tempfile::tempdir().expect("work tree");
    let store = store_with_backups(directory.path(), objects.path());
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Backup");
    store.provision(&asset).await.expect("repository");
    initialize_work_tree(work.path(), asset.kind, "before backup");
    push_main(work.path(), &store.repository_path(&asset));
    let original_commit = commit(work.path());
    let backup_lease = write_lease(&asset, AssetGitWriteOperation::Backup);
    store
        .prepare_write(&asset, &backup_lease)
        .await
        .expect("backup journal");
    let backup = store
        .create_backup(&asset, &backup_lease, Utc::now())
        .await
        .expect("repository backup");
    store
        .settle_write(&asset, &backup_lease.journal())
        .await
        .expect("settle backup");
    assert_eq!(
        backup.refs_digest,
        store.refs_digest(&asset).await.expect("refs")
    );

    std::fs::write(work.path().join("README.md"), "after backup\n").expect("change work tree");
    git_success(work.path(), &["add", "README.md"]);
    git_success(work.path(), &["commit", "--quiet", "-m", "after backup"]);
    push_main(work.path(), &store.repository_path(&asset));
    assert_ne!(commit(work.path()), original_commit);
    assert_ne!(
        store.refs_digest(&asset).await.expect("changed refs"),
        backup.refs_digest
    );

    let restore_lease = write_lease(&asset, AssetGitWriteOperation::Restore);
    store
        .prepare_write(&asset, &restore_lease)
        .await
        .expect("restore journal");
    let restored = store
        .restore_backup(&asset, &restore_lease, &backup, 32 * 1024 * 1024)
        .await
        .expect("restore repository");
    store
        .settle_write(&asset, &restore_lease.journal())
        .await
        .expect("settle restore");
    assert_eq!(restored.refs_digest, backup.refs_digest);
    assert_eq!(
        git_output(
            directory.path(),
            &[
                format!("--git-dir={}", store.repository_path(&asset).display()).as_str(),
                "rev-parse",
                "refs/heads/main",
            ],
        ),
        original_commit.as_str()
    );
    let replay_lease = write_lease(&asset, AssetGitWriteOperation::Restore);
    store
        .prepare_write(&asset, &replay_lease)
        .await
        .expect("replay restore journal");
    assert!(
        store
            .restore_backup(&asset, &replay_lease, &backup, 32 * 1024 * 1024)
            .await
            .is_ok(),
        "exact restore replay must be harmless"
    );
    store
        .settle_write(&asset, &replay_lease.journal())
        .await
        .expect("settle restore replay");
}

#[tokio::test]
async fn expired_receive_pack_journal_rolls_back_after_repository_restart() {
    let directory = tempfile::tempdir().expect("repository directory");
    let work = tempfile::tempdir().expect("work tree");
    let asset = asset_with(AssetId::new(), OrganizationId::new(), "Restart rollback");
    let initial = store(directory.path());
    initial.provision(&asset).await.expect("repository");
    let original_refs = initial.refs_digest(&asset).await.expect("original refs");
    let original_bytes = initial
        .repository_bytes(&asset)
        .await
        .expect("original bytes");
    initialize_work_tree(work.path(), asset.kind, "uncommitted push");
    let advertisement = initial
        .git(vec![
            "receive-pack".into(),
            "--stateless-rpc".into(),
            "--advertise-refs".into(),
            initial.repository_path(&asset).as_os_str().to_owned(),
        ])
        .await
        .expect("receive-pack advertisement");
    let request = send_pack_request(
        work.path(),
        &initial.repository_path(&asset),
        &advertisement,
    );
    let lease = write_lease(&asset, AssetGitWriteOperation::ReceivePack);
    initial
        .prepare_write(&asset, &lease)
        .await
        .expect("durable write journal");
    initial
        .execute_rpc(
            &asset,
            AssetGitService::ReceivePack,
            request,
            AssetGitRpcLimits {
                maximum_input_bytes: 32 * 1024 * 1024,
                maximum_repository_bytes: 32 * 1024 * 1024,
            },
            Some(&lease),
        )
        .await
        .expect("uncommitted receive pack");
    assert_ne!(
        initial.refs_digest(&asset).await.expect("changed refs"),
        original_refs
    );
    drop(initial);

    let restarted = store(directory.path());
    let mut recovery = lease;
    recovery.recovery = true;
    restarted
        .rollback_write(&asset, &recovery)
        .await
        .expect("restart rollback");
    assert_eq!(
        restarted.refs_digest(&asset).await.expect("restored refs"),
        original_refs
    );
    assert!(
        restarted
            .repository_bytes(&asset)
            .await
            .expect("restored bytes")
            <= original_bytes
    );
    restarted
        .rollback_write(&asset, &recovery)
        .await
        .expect("idempotent restart rollback");
}

fn initialize_work_tree(path: &Path, kind: AssetKind, contents: &str) {
    git_success(path, &["init", "--quiet", "--initial-branch=main"]);
    git_success(path, &["config", "user.email", "test@a3s.dev"]);
    git_success(path, &["config", "user.name", "A3S Test"]);
    write_manifest(path, kind);
    std::fs::write(path.join("README.md"), format!("{contents}\n")).expect("README");
    git_success(path, &["add", "."]);
    git_success(path, &["commit", "--quiet", "-m", "initial"]);
}

fn write_manifest(path: &Path, kind: AssetKind) {
    std::fs::create_dir_all(path.join(".a3s")).expect("manifest directory");
    std::fs::write(
        path.join(".a3s/asset.acl"),
        format!(
            "asset {{\n  schema = \"a3s.cloud.asset.v1\"\n  kind = \"{}\"\n}}\n",
            kind.as_str()
        ),
    )
    .expect("Asset manifest");
}

fn write_build_manifest(path: &Path, kind: AssetKind) {
    std::fs::create_dir_all(path.join(".a3s")).expect("manifest directory");
    std::fs::write(
        path.join(".a3s/asset.acl"),
        format!(
            concat!(
                "asset {{\n",
                "  schema = \"a3s.cloud.asset.v1\"\n",
                "  kind = \"{}\"\n",
                "  build {{\n",
                "    context = \".\"\n",
                "    file = \"Dockerfile\"\n",
                "    platforms = [\"linux/amd64\"]\n",
                "    target = \"release\"\n",
                "  }}\n",
                "}}\n",
            ),
            kind.as_str()
        ),
    )
    .expect("Asset build manifest");
}

fn push_main(work: &Path, repository: &Path) {
    let destination = repository.to_str().expect("UTF-8 repository path");
    git_success(work, &["push", "--quiet", "--force", destination, "main"]);
}

fn commit(work: &Path) -> GitCommitSha {
    GitCommitSha::parse(git_output(work, &["rev-parse", "HEAD"])).expect("commit SHA")
}

fn send_pack_request(work: &Path, repository: &Path, advertisement: &[u8]) -> Vec<u8> {
    let mut child = Command::new("git")
        .current_dir(work)
        .args([
            "send-pack",
            "--stateless-rpc",
            repository.to_str().expect("UTF-8 repository path"),
            "HEAD:refs/heads/main",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn send-pack");
    child
        .stdin
        .take()
        .expect("send-pack stdin")
        .write_all(advertisement)
        .expect("write receive-pack advertisement");
    unwrap_stateless_rpc(&child.wait_with_output().expect("collect send-pack").stdout)
}

fn unwrap_stateless_rpc(output: &[u8]) -> Vec<u8> {
    let mut request = Vec::new();
    let mut offset = 0_usize;
    while offset < output.len() {
        assert!(output.len() - offset >= 4, "truncated stateless RPC frame");
        let length = std::str::from_utf8(&output[offset..offset + 4])
            .ok()
            .and_then(|value| usize::from_str_radix(value, 16).ok())
            .expect("stateless RPC frame length");
        offset += 4;
        if length == 0 {
            continue;
        }
        assert!(length >= 4 && output.len() - offset >= length - 4);
        request.extend_from_slice(&output[offset..offset + length - 4]);
        offset += length - 4;
    }
    request
}

fn git_success(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "Git fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("run Git fixture query");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("Git fixture UTF-8")
        .trim()
        .to_owned()
}
