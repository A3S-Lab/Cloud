use a3s_cloud_control_plane::modules::artifacts::domain::OCI_IMAGE_MANIFEST_MEDIA_TYPE;
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetArchived, AssetCreated, AssetKind, AssetRelease, AssetReleaseArtifact,
    AssetReleaseDrafted, AssetReleasePublished, AssetReleaseVersion, AssetReleaseYanked,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, PostgresAssetRepository,
    TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, IdempotencyRequest, OrganizationId, RepositoryError,
    ResourceName, Sha256Digest,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::Duration;
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
