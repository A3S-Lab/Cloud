use a3s_cloud_control_plane::modules::artifacts::domain::OCI_IMAGE_MANIFEST_MEDIA_TYPE;
use a3s_cloud_control_plane::modules::assets::{
    AcquireAssetGitWriteLease, Asset, AssetArchived, AssetCreated, AssetGitRepositoryControlError,
    AssetGitWriteOperation, AssetGitWriteRecovery, AssetKind, AssetRelease, AssetReleaseArtifact,
    AssetReleaseDrafted, AssetReleasePublished, AssetReleaseVersion, AssetReleaseYanked,
    ClaimAssetGitWriteRecovery, CompleteAssetGitWriteLease, CreateAssetReleaseWrite,
    CreateAssetWrite, IAssetGitRepositoryControl, IAssetRepository, IMcpServiceProfileRepository,
    McpServiceProfile, McpServiceProfileBinding, McpServiceProfileSpec, PostgresAssetRepository,
    TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, IdempotencyRequest, OrganizationId, RepositoryError,
    ResourceName, Sha256Digest,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn exercise_assets(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
) -> TestResult {
    let repository = PostgresAssetRepository::new(executor.clone());
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("Hosted Agent")?,
        AssetKind::Agent,
        chrono::Utc::now(),
    )?;
    let create = create_asset_write(&asset, "create-hosted-agent", b"hosted-agent")?;
    let (left, right) = tokio::join!(
        repository.create_asset(create.clone()),
        repository.create_asset(create.clone()),
    );
    let created = [left?, right?];
    assert_eq!(created.iter().filter(|write| write.replayed).count(), 1);
    assert!(created.iter().all(|write| write.asset == asset));

    let mut changed_create = create;
    changed_create.idempotency = IdempotencyRequest::new(
        changed_create.idempotency.scope.clone(),
        changed_create.idempotency.key.clone(),
        b"different hosted Agent",
    )?;
    assert_eq!(
        repository.create_asset(changed_create).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        repository.find_asset(organization_id, asset.id).await?,
        Some(asset.clone())
    );
    assert_eq!(
        repository.list_assets(organization_id).await?,
        vec![asset.clone()]
    );
    assert_eq!(
        repository
            .find_asset(other_organization_id, asset.id)
            .await?,
        None
    );

    let duplicate_name = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("hosted agent")?,
        AssetKind::Mcp,
        asset.created_at + Duration::milliseconds(1),
    )?;
    assert!(matches!(
        repository
            .create_asset(create_asset_write(
                &duplicate_name,
                "duplicate-hosted-agent-name",
                b"duplicate-hosted-agent-name",
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let other_asset = Asset::create(
        AssetId::new(),
        other_organization_id,
        ResourceName::parse("Hosted Agent")?,
        AssetKind::Skill,
        asset.created_at + Duration::milliseconds(2),
    )?;
    repository
        .create_asset(create_asset_write(
            &other_asset,
            "create-other-tenant-asset",
            b"other-tenant-asset",
        )?)
        .await?;
    assert_eq!(
        repository
            .find_asset(other_organization_id, other_asset.id)
            .await?,
        Some(other_asset)
    );

    let release = draft_release(
        &asset,
        AssetReleaseId::new(),
        "1.0.0",
        'a',
        'b',
        asset.created_at + Duration::seconds(1),
    )?;
    let create_release =
        create_release_write(&release, "draft-hosted-agent-1-0-0", b"hosted-agent-1.0.0")?;
    let (left, right) = tokio::join!(
        repository.create_release(create_release.clone()),
        repository.create_release(create_release.clone()),
    );
    let drafted = [left?, right?];
    assert_eq!(drafted.iter().filter(|write| write.replayed).count(), 1);
    assert!(drafted
        .iter()
        .all(|write| write.asset == asset && write.release == release));

    let duplicate_version = draft_release(
        &asset,
        AssetReleaseId::new(),
        "1.0.0",
        'c',
        'd',
        release.created_at + Duration::milliseconds(1),
    )?;
    assert!(matches!(
        repository
            .create_release(create_release_write(
                &duplicate_version,
                "duplicate-hosted-agent-version",
                b"duplicate-hosted-agent-version",
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let foreign_asset_reference = Asset::create(
        asset.id,
        other_organization_id,
        ResourceName::parse("Foreign Asset Reference")?,
        AssetKind::Agent,
        asset.created_at,
    )?;
    let foreign_release = draft_release(
        &foreign_asset_reference,
        AssetReleaseId::new(),
        "1.0.0",
        'e',
        'f',
        release.created_at,
    )?;
    assert_eq!(
        repository
            .create_release(create_release_write(
                &foreign_release,
                "cross-tenant-release-reference",
                b"cross-tenant-release-reference",
            )?)
            .await,
        Err(RepositoryError::NotFound)
    );

    let artifact =
        AssetReleaseArtifact::oci_service(digest('1')?, OCI_IMAGE_MANIFEST_MEDIA_TYPE, 4_096)?;
    let mut published = release.clone();
    published.publish(
        &asset,
        artifact.clone(),
        release.updated_at + Duration::seconds(1),
    )?;
    repository
        .transition_release(TransitionAssetReleaseWrite {
            event: AssetReleasePublished::envelope(&published, Uuid::now_v7())?,
            release: published.clone(),
            expected_aggregate_version: release.aggregate_version,
            idempotency: idempotency(
                organization_id,
                format!("assets/{}/releases", asset.id),
                "publish-hosted-agent-1-0-0",
                b"publish-hosted-agent-1.0.0",
            )?,
        })
        .await?;

    let mut stale_publication = release.clone();
    stale_publication.publish(
        &asset,
        artifact.clone(),
        release.updated_at + Duration::seconds(2),
    )?;
    assert!(matches!(
        repository
            .transition_release(TransitionAssetReleaseWrite {
                event: AssetReleasePublished::envelope(&stale_publication, Uuid::now_v7(),)?,
                release: stale_publication,
                expected_aggregate_version: release.aggregate_version,
                idempotency: idempotency(
                    organization_id,
                    format!("assets/{}/releases", asset.id),
                    "stale-publish-hosted-agent-1-0-0",
                    b"stale-publish-hosted-agent-1.0.0",
                )?,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let later_release = draft_release(
        &asset,
        AssetReleaseId::new(),
        "2.0.0",
        '2',
        '3',
        published.updated_at + Duration::seconds(1),
    )?;
    repository
        .create_release(create_release_write(
            &later_release,
            "draft-hosted-agent-2-0-0",
            b"hosted-agent-2.0.0",
        )?)
        .await?;
    let mut later_publication = later_release.clone();
    later_publication.publish(
        &asset,
        AssetReleaseArtifact::oci_service(digest('4')?, OCI_IMAGE_MANIFEST_MEDIA_TYPE, 8_192)?,
        later_release.updated_at + Duration::seconds(1),
    )?;
    let blocked_new_release = draft_release(
        &asset,
        AssetReleaseId::new(),
        "3.0.0",
        '5',
        '6',
        later_release.updated_at + Duration::seconds(2),
    )?;

    let mut archived = asset.clone();
    archived.archive(later_release.updated_at + Duration::seconds(3))?;
    repository
        .transition_asset(TransitionAssetWrite {
            event: AssetArchived::envelope(&archived, Uuid::now_v7())?,
            asset: archived.clone(),
            expected_aggregate_version: asset.aggregate_version,
            idempotency: idempotency(
                organization_id,
                "assets",
                "archive-hosted-agent",
                b"archive-hosted-agent",
            )?,
        })
        .await?;

    let mut stale_archive = asset.clone();
    stale_archive.archive(archived.updated_at + Duration::seconds(1))?;
    assert!(matches!(
        repository
            .transition_asset(TransitionAssetWrite {
                event: AssetArchived::envelope(&stale_archive, Uuid::now_v7())?,
                asset: stale_archive,
                expected_aggregate_version: asset.aggregate_version,
                idempotency: idempotency(
                    organization_id,
                    "assets",
                    "stale-archive-hosted-agent",
                    b"stale-archive-hosted-agent",
                )?,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(matches!(
        repository
            .create_release(create_release_write(
                &blocked_new_release,
                "draft-after-archive",
                b"draft-after-archive",
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(matches!(
        repository
            .transition_release(TransitionAssetReleaseWrite {
                event: AssetReleasePublished::envelope(&later_publication, Uuid::now_v7(),)?,
                release: later_publication,
                expected_aggregate_version: later_release.aggregate_version,
                idempotency: idempotency(
                    organization_id,
                    format!("assets/{}/releases", asset.id),
                    "publish-after-archive",
                    b"publish-after-archive",
                )?,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let mut changed_identity = published.clone();
    changed_identity.yank(archived.updated_at + Duration::seconds(1))?;
    changed_identity.version = AssetReleaseVersion::parse("9.9.9")?;
    changed_identity.commit_sha = GitCommitSha::parse("7".repeat(40))?;
    changed_identity.manifest_digest = digest('8')?;
    changed_identity.artifact = Some(AssetReleaseArtifact::oci_service(
        digest('9')?,
        OCI_IMAGE_MANIFEST_MEDIA_TYPE,
        16_384,
    )?);
    assert!(matches!(
        repository
            .transition_release(TransitionAssetReleaseWrite {
                event: AssetReleaseYanked::envelope(&changed_identity, Uuid::now_v7())?,
                release: changed_identity,
                expected_aggregate_version: published.aggregate_version,
                idempotency: idempotency(
                    organization_id,
                    format!("assets/{}/releases", asset.id),
                    "mutate-published-identity",
                    b"mutate-published-identity",
                )?,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let mut yanked = published.clone();
    yanked.yank(archived.updated_at + Duration::seconds(1))?;
    repository
        .transition_release(TransitionAssetReleaseWrite {
            event: AssetReleaseYanked::envelope(&yanked, Uuid::now_v7())?,
            release: yanked.clone(),
            expected_aggregate_version: published.aggregate_version,
            idempotency: idempotency(
                organization_id,
                format!("assets/{}/releases", asset.id),
                "yank-hosted-agent-1-0-0",
                b"yank-hosted-agent-1.0.0",
            )?,
        })
        .await?;
    assert_eq!(
        repository
            .find_release(organization_id, asset.id, release.id)
            .await?,
        Some(yanked.clone())
    );
    assert_eq!(yanked.artifact, Some(artifact));
    assert_eq!(
        repository
            .find_release(other_organization_id, asset.id, release.id)
            .await?,
        None
    );
    assert!(repository
        .list_releases(other_organization_id, asset.id)
        .await?
        .is_empty());
    assert_eq!(
        repository
            .list_releases(organization_id, asset.id)
            .await?
            .len(),
        2
    );

    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        outbox_count(&database, asset.id.as_uuid()).await?,
        2,
        "Asset create and archive must each commit one outbox event",
    );
    assert_eq!(
        outbox_count(&database, release.id.as_uuid()).await?,
        3,
        "release draft, publication, and yank must each commit one outbox event",
    );
    assert_eq!(
        outbox_count(&database, later_release.id.as_uuid()).await?,
        1,
        "failed publication after archive must not leak an outbox event",
    );
    exercise_mcp_service_profiles(
        &repository,
        organization_id,
        other_organization_id,
        archived.updated_at + Duration::seconds(10),
    )
    .await?;

    Ok(())
}

pub async fn exercise_asset_git_controls(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
) -> TestResult<Asset> {
    let repository = PostgresAssetRepository::new(executor.clone());
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("Hosted Git control")?,
        AssetKind::Agent,
        Utc::now(),
    )?;
    repository
        .create_asset(create_asset_write(
            &asset,
            "create-hosted-git-control",
            b"hosted-git-control",
        )?)
        .await?;

    let acquired_at = Utc::now();
    let left = lease_request(
        &asset,
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        100,
        1_048_576,
        acquired_at,
        acquired_at + Duration::seconds(30),
    );
    let right = lease_request(
        &asset,
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        100,
        1_048_576,
        acquired_at,
        acquired_at + Duration::seconds(30),
    );
    let (left, right) = tokio::join!(
        repository.acquire_write(left),
        repository.acquire_write(right)
    );
    let winner = match (left, right) {
        (Ok(lease), Err(AssetGitRepositoryControlError::Busy))
        | (Err(AssetGitRepositoryControlError::Busy), Ok(lease)) => lease,
        outcomes => {
            return Err(format!("unexpected concurrent lease outcomes: {outcomes:?}").into())
        }
    };
    repository.abandon_write(&winner).await?;

    let original = repository
        .acquire_write(lease_request(
            &asset,
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            100,
            2_097_152,
            acquired_at,
            acquired_at + Duration::seconds(1),
        ))
        .await?;
    assert_eq!(
        original.quota_bytes, 1_048_576,
        "stored quota must remain authoritative"
    );
    let replacement_request = lease_request(
        &asset,
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        100,
        2_097_152,
        acquired_at + Duration::seconds(2),
        acquired_at + Duration::seconds(32),
    );
    assert_eq!(
        repository.acquire_write(replacement_request.clone()).await,
        Err(AssetGitRepositoryControlError::RecoveryRequired)
    );
    let recovered = match repository
        .claim_write_recovery(ClaimAssetGitWriteRecovery {
            asset: asset.clone(),
            claimed_at: acquired_at + Duration::seconds(2),
            leased_until: acquired_at + Duration::seconds(32),
        })
        .await?
    {
        Some(AssetGitWriteRecovery::Rollback(lease)) => lease,
        outcome => return Err(format!("unexpected write recovery outcome: {outcome:?}").into()),
    };
    assert_eq!(recovered.lease_id, original.lease_id);
    assert!(recovered.recovery);
    assert_eq!(recovered.quota_bytes, 1_048_576);
    assert_eq!(
        repository.abandon_write(&original).await,
        Err(AssetGitRepositoryControlError::StaleLease)
    );
    assert_eq!(
        repository
            .complete_write(CompleteAssetGitWriteLease {
                lease: original,
                observed_bytes: 100,
                refs_digest: digest('a')?,
                backup: None,
                completed_at: acquired_at + Duration::seconds(3),
            })
            .await,
        Err(AssetGitRepositoryControlError::StaleLease)
    );
    repository.abandon_write(&recovered).await?;
    let replacement = repository
        .acquire_write(AcquireAssetGitWriteLease {
            acquired_at: acquired_at + Duration::seconds(3),
            leased_until: acquired_at + Duration::seconds(33),
            ..replacement_request
        })
        .await?;
    assert_eq!(replacement.quota_bytes, 1_048_576);
    repository.abandon_write(&replacement).await?;

    let restarted = PostgresAssetRepository::new(executor.clone());
    assert_eq!(
        restarted
            .acquire_write(lease_request(
                &asset,
                Uuid::now_v7(),
                Uuid::now_v7(),
                Uuid::now_v7(),
                1_048_577,
                4_194_304,
                acquired_at + Duration::seconds(40),
                acquired_at + Duration::seconds(70),
            ))
            .await,
        Err(AssetGitRepositoryControlError::QuotaExceeded {
            quota_bytes: 1_048_576,
            observed_bytes: 1_048_577,
        })
    );

    let completion_lease = restarted
        .acquire_write(lease_request(
            &asset,
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            100,
            4_194_304,
            acquired_at + Duration::seconds(41),
            acquired_at + Duration::seconds(71),
        ))
        .await?;
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "alter table audit_records add constraint asset_git_audit_failure_probe check (action <> 'asset.repository.pushed')",
        )
        .await?;
    let completion = CompleteAssetGitWriteLease {
        lease: completion_lease.clone(),
        observed_bytes: 200,
        refs_digest: digest('b')?,
        backup: None,
        completed_at: acquired_at + Duration::seconds(42),
    };
    assert!(matches!(
        restarted.complete_write(completion.clone()).await,
        Err(AssetGitRepositoryControlError::Storage(_))
    ));
    let database = Database::new(PostgresDialect, executor.clone());
    let rolled_back = database
        .fetch_one_as(sql_query::<(Option<Uuid>, i64)>(
            "select write_lease_id, observed_bytes from asset_git_repository_controls where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and asset_id = ")
        .bind(asset.id.as_uuid()))
        .await?;
    assert_eq!(rolled_back, (Some(completion_lease.lease_id), 100));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from audit_records where aggregate_id = ",)
                    .bind(asset.id.as_uuid())
            )
            .await?,
        0
    );
    executor
        .pool()
        .get()
        .await?
        .batch_execute("alter table audit_records drop constraint asset_git_audit_failure_probe")
        .await?;
    restarted.complete_write(completion).await?;
    let completed = database
        .fetch_one_as(sql_query::<(Option<Uuid>, Option<Uuid>, i64)>(
            "select write_lease_id, write_cleanup_lease_id, observed_bytes from asset_git_repository_controls where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and asset_id = ")
        .bind(asset.id.as_uuid()))
        .await?;
    assert_eq!(completed, (None, Some(completion_lease.lease_id), 200));
    let cleanup = match restarted
        .claim_write_recovery(ClaimAssetGitWriteRecovery {
            asset: asset.clone(),
            claimed_at: acquired_at + Duration::seconds(43),
            leased_until: acquired_at + Duration::seconds(73),
        })
        .await?
    {
        Some(AssetGitWriteRecovery::Cleanup(journal)) => journal,
        outcome => return Err(format!("unexpected write cleanup outcome: {outcome:?}").into()),
    };
    assert_eq!(cleanup.lease_id, completion_lease.lease_id);
    restarted.settle_write(&cleanup).await?;
    let settled = database
        .fetch_one_as(sql_query::<(Option<Uuid>, Option<Uuid>, i64)>(
            "select write_lease_id, write_cleanup_lease_id, observed_bytes from asset_git_repository_controls where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and asset_id = ")
        .bind(asset.id.as_uuid()))
        .await?;
    assert_eq!(settled, (None, None, 200));
    let audit = database
        .fetch_one_as(
            sql_query::<(String, serde_json::Value)>(
                "select action, details from audit_records where aggregate_id = ",
            )
            .bind(asset.id.as_uuid()),
        )
        .await?;
    assert_eq!(audit.0, "asset.repository.pushed");
    let audit_text = audit.1.to_string();
    assert!(!audit_text.contains("authorization"));
    assert!(!audit_text.contains("credential"));
    assert!(!audit_text.contains("requestBody"));

    let foreign_asset = Asset::create(
        asset.id,
        other_organization_id,
        ResourceName::parse("Foreign hosted Git control")?,
        AssetKind::Agent,
        asset.created_at,
    )?;
    assert_eq!(
        restarted
            .acquire_write(lease_request(
                &foreign_asset,
                Uuid::now_v7(),
                Uuid::now_v7(),
                Uuid::now_v7(),
                100,
                1_048_576,
                acquired_at + Duration::seconds(80),
                acquired_at + Duration::seconds(110),
            ))
            .await,
        Err(AssetGitRepositoryControlError::NotFound)
    );
    Ok(asset)
}

#[allow(clippy::too_many_arguments)]
fn lease_request(
    asset: &Asset,
    lease_id: Uuid,
    actor_id: Uuid,
    request_id: Uuid,
    observed_bytes: u64,
    default_quota_bytes: u64,
    acquired_at: chrono::DateTime<Utc>,
    leased_until: chrono::DateTime<Utc>,
) -> AcquireAssetGitWriteLease {
    AcquireAssetGitWriteLease {
        asset: asset.clone(),
        lease_id,
        operation: AssetGitWriteOperation::ReceivePack,
        actor_id,
        request_id,
        observed_bytes,
        default_quota_bytes,
        acquired_at,
        leased_until,
    }
}

pub fn receive_pack_fixture(repository: &Path, kind: AssetKind) -> TestResult<Vec<u8>> {
    let work = tempfile::tempdir()?;
    git_fixture(work.path(), &["init", "--quiet", "--initial-branch=main"])?;
    git_fixture(
        work.path(),
        &["config", "user.email", "integration@a3s.dev"],
    )?;
    git_fixture(work.path(), &["config", "user.name", "A3S Integration"])?;
    std::fs::create_dir_all(work.path().join(".a3s"))?;
    std::fs::write(
        work.path().join(".a3s/asset.acl"),
        format!(
            "asset {{\n  schema = \"a3s.cloud.asset.v1\"\n  kind = \"{}\"\n}}\n",
            kind.as_str()
        ),
    )?;
    std::fs::write(work.path().join("README.md"), "Hosted Git integration\n")?;
    git_fixture(work.path(), &["add", "."])?;
    git_fixture(work.path(), &["commit", "--quiet", "-m", "initial"])?;
    let advertisement = Command::new("git")
        .args([
            "receive-pack",
            "--stateless-rpc",
            "--advertise-refs",
            repository
                .to_str()
                .ok_or("repository fixture path is not UTF-8")?,
        ])
        .output()?;
    if !advertisement.status.success() {
        return Err("could not advertise integration repository".into());
    }
    let mut child = Command::new("git")
        .current_dir(work.path())
        .args([
            "send-pack",
            "--stateless-rpc",
            repository
                .to_str()
                .ok_or("repository fixture path is not UTF-8")?,
            "HEAD:refs/heads/main",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("integration send-pack stdin is unavailable")?
        .write_all(&advertisement.stdout)?;
    let output = child.wait_with_output()?;
    unwrap_stateless_rpc(&output.stdout).map_err(|error| {
        format!(
            "integration send-pack did not produce a valid request ({error}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into()
    })
}

fn unwrap_stateless_rpc(output: &[u8]) -> TestResult<Vec<u8>> {
    let mut request = Vec::new();
    let mut offset = 0_usize;
    while offset < output.len() {
        if output.len() - offset < 4 {
            return Err("stateless Git RPC frame is truncated".into());
        }
        let length = std::str::from_utf8(&output[offset..offset + 4])
            .ok()
            .and_then(|value| usize::from_str_radix(value, 16).ok())
            .ok_or("stateless Git RPC frame length is invalid")?;
        offset += 4;
        if length == 0 {
            continue;
        }
        if length < 4 || output.len() - offset < length - 4 {
            return Err("stateless Git RPC frame body is truncated".into());
        }
        request.extend_from_slice(&output[offset..offset + length - 4]);
        offset += length - 4;
    }
    if request.is_empty() {
        return Err("stateless Git RPC request is empty".into());
    }
    Ok(request)
}

fn git_fixture(directory: &Path, arguments: &[&str]) -> TestResult {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "Git integration fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

async fn exercise_mcp_service_profiles(
    repository: &PostgresAssetRepository,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    created_at: chrono::DateTime<chrono::Utc>,
) -> TestResult {
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("Weather MCP")?,
        AssetKind::Mcp,
        created_at,
    )?;
    repository
        .create_asset(create_asset_write(
            &asset,
            "create-weather-mcp",
            b"weather-mcp",
        )?)
        .await?;
    let release = draft_release(
        &asset,
        AssetReleaseId::new(),
        "1.0.0",
        'a',
        'b',
        created_at + Duration::seconds(1),
    )?;
    repository
        .create_release(create_release_write(
            &release,
            "draft-weather-mcp-1-0-0",
            b"weather-mcp-1.0.0",
        )?)
        .await?;

    let profile = McpServiceProfile::from_spec(McpServiceProfileSpec {
        protocol_versions: vec![a3s_cloud_contracts::MCP_PROTOCOL_VERSION.into()],
        endpoint_path: "/mcp".into(),
        runtime_port: "mcp".into(),
        health_path: "/health".into(),
        request_sse: true,
        subscriptions: true,
        server_discover: true,
        expected_capabilities: vec!["tools".into(), "subscriptions".into()],
        max_request_bytes: 1_048_576,
        max_response_bytes: 8_388_608,
        max_stream_seconds: 3_600,
    })?;
    let draft_binding = McpServiceProfileBinding {
        organization_id,
        asset_id: asset.id,
        asset_release_id: release.id,
        profile: profile.clone(),
        created_at: release.updated_at + Duration::seconds(1),
    };
    assert!(matches!(
        repository.bind_mcp_service_profile(draft_binding).await,
        Err(RepositoryError::Conflict(_))
    ));

    let mut published = release.clone();
    published.publish(
        &asset,
        AssetReleaseArtifact::oci_service(digest('c')?, OCI_IMAGE_MANIFEST_MEDIA_TYPE, 4_096)?,
        release.updated_at + Duration::seconds(2),
    )?;
    repository
        .transition_release(TransitionAssetReleaseWrite {
            event: AssetReleasePublished::envelope(&published, Uuid::now_v7())?,
            release: published.clone(),
            expected_aggregate_version: release.aggregate_version,
            idempotency: idempotency(
                organization_id,
                format!("assets/{}/releases", asset.id),
                "publish-weather-mcp-1-0-0",
                b"publish-weather-mcp-1.0.0",
            )?,
        })
        .await?;
    let binding = McpServiceProfileBinding {
        organization_id,
        asset_id: asset.id,
        asset_release_id: published.id,
        profile,
        created_at: published.updated_at + Duration::seconds(1),
    };
    let (left, right) = tokio::join!(
        repository.bind_mcp_service_profile(binding.clone()),
        repository.bind_mcp_service_profile(binding.clone()),
    );
    assert_eq!(left?, binding);
    assert_eq!(right?, binding);
    assert_eq!(
        repository
            .find_mcp_service_profile(organization_id, asset.id, published.id)
            .await?,
        Some(binding.clone())
    );
    assert_eq!(
        repository
            .find_mcp_service_profile(other_organization_id, asset.id, published.id)
            .await?,
        None
    );

    let mut changed_spec = binding.profile.spec().clone();
    changed_spec.endpoint_path = "/other-mcp".into();
    let changed = McpServiceProfileBinding {
        profile: McpServiceProfile::from_spec(changed_spec)?,
        ..binding
    };
    assert!(matches!(
        repository.bind_mcp_service_profile(changed).await,
        Err(RepositoryError::Conflict(_))
    ));
    Ok(())
}

fn create_asset_write(
    asset: &Asset,
    key: &str,
    canonical_request: &[u8],
) -> TestResult<CreateAssetWrite> {
    Ok(CreateAssetWrite {
        event: AssetCreated::envelope(asset, Uuid::now_v7())?,
        idempotency: idempotency(asset.organization_id, "assets", key, canonical_request)?,
        asset: asset.clone(),
    })
}

fn create_release_write(
    release: &AssetRelease,
    key: &str,
    canonical_request: &[u8],
) -> TestResult<CreateAssetReleaseWrite> {
    Ok(CreateAssetReleaseWrite {
        event: AssetReleaseDrafted::envelope(release, Uuid::now_v7())?,
        idempotency: idempotency(
            release.organization_id,
            format!("assets/{}/releases", release.asset_id),
            key,
            canonical_request,
        )?,
        release: release.clone(),
    })
}

fn draft_release(
    asset: &Asset,
    id: AssetReleaseId,
    version: &str,
    commit: char,
    manifest: char,
    created_at: chrono::DateTime<chrono::Utc>,
) -> TestResult<AssetRelease> {
    Ok(AssetRelease::draft(
        asset,
        id,
        AssetReleaseVersion::parse(version)?,
        GitCommitSha::parse(commit.to_string().repeat(40))?,
        digest(manifest)?,
        created_at,
    )?)
}

fn digest(character: char) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
}

fn idempotency(
    organization_id: OrganizationId,
    suffix: impl std::fmt::Display,
    key: &str,
    canonical_request: &[u8],
) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(
        format!("organizations/{organization_id}/{suffix}"),
        key,
        canonical_request,
    )
}

async fn outbox_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    aggregate_id: Uuid,
) -> TestResult<i64> {
    Ok(database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from outbox_events where aggregate_id = ")
                .bind(aggregate_id),
        )
        .await?)
}
