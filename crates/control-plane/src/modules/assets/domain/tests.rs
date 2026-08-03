use super::*;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_build;
use crate::modules::artifacts::domain::OCI_IMAGE_INDEX_MEDIA_TYPE;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, IdempotencyRequest, OrganizationId, ResourceName,
    Sha256Digest,
};
use chrono::{Duration, TimeZone, Utc};
use uuid::Uuid;

fn now() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_800_000_000, 123_456_000)
        .single()
        .expect("timestamp")
}

fn asset(kind: AssetKind) -> Asset {
    Asset::create(
        AssetId::new(),
        OrganizationId::new(),
        ResourceName::parse("Research Assistant").expect("name"),
        kind,
        now(),
    )
    .expect("Asset")
}

fn draft(asset: &Asset) -> AssetRelease {
    AssetRelease::draft(
        asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.2.3").expect("version"),
        GitCommitSha::parse("a".repeat(40)).expect("commit"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("manifest digest"),
        now(),
    )
    .expect("draft")
}

fn oci_artifact() -> AssetReleaseArtifact {
    AssetReleaseArtifact::oci_service(
        Sha256Digest::parse(format!("sha256:{}", "c".repeat(64))).expect("artifact digest"),
        OCI_IMAGE_INDEX_MEDIA_TYPE,
        1024,
    )
    .expect("OCI artifact")
}

fn skill_artifact() -> AssetReleaseArtifact {
    AssetReleaseArtifact::skill_bundle(
        Sha256Digest::parse(format!("sha256:{}", "d".repeat(64))).expect("artifact digest"),
        2048,
    )
    .expect("Skill artifact")
}

#[test]
fn asset_kinds_and_release_versions_are_closed_and_canonical() {
    assert_eq!(AssetKind::parse("agent"), Ok(AssetKind::Agent));
    assert_eq!(AssetKind::parse("mcp"), Ok(AssetKind::Mcp));
    assert_eq!(AssetKind::parse("skill"), Ok(AssetKind::Skill));
    assert!(AssetKind::parse("model").is_err());
    assert!(AssetKind::parse("workflow").is_err());
    assert!(AssetReleaseVersion::parse("1.0.0").is_ok());
    assert!(AssetReleaseVersion::parse("v1.0.0").is_err());
    assert!(AssetReleaseVersion::parse("1.0").is_err());
}

#[test]
fn asset_archive_is_terminal_idempotent_and_time_monotonic() {
    let mut asset = asset(AssetKind::Agent);
    assert!(asset.archive(now() - Duration::seconds(1)).is_err());
    let archived_at = now() + Duration::seconds(1);
    asset.archive(archived_at).expect("archive");
    assert_eq!(asset.state, AssetState::Archived);
    assert_eq!(asset.aggregate_version, 2);
    assert_eq!(asset.archived_at, Some(archived_at));
    asset
        .archive(archived_at + Duration::seconds(1))
        .expect("archive replay");
    assert_eq!(asset.aggregate_version, 2);
}

#[test]
fn release_publication_binds_immutable_source_and_artifact_before_yanking() {
    let asset = asset(AssetKind::Agent);
    let mut release = draft(&asset);
    let immutable_identity = (
        release.version.clone(),
        release.commit_sha.clone(),
        release.manifest_digest.clone(),
    );
    let build = succeeded_hosted_build(asset.organization_id, asset.id, release.id, now());
    let artifact = AssetReleaseArtifact::oci_service(
        Sha256Digest::parse(
            &build
                .published_artifact
                .as_ref()
                .expect("published artifact")
                .digest,
        )
        .expect("artifact digest"),
        build
            .published_artifact
            .as_ref()
            .expect("published artifact")
            .media_type
            .clone(),
        build
            .published_artifact
            .as_ref()
            .expect("published artifact")
            .size_bytes,
    )
    .expect("release artifact");
    let published_at = build.finished_at.expect("build finish time");
    release.publish_from_build(&asset, &build).expect("publish");
    assert_eq!(release.state, AssetReleaseState::Published);
    assert_eq!(release.artifact, Some(artifact.clone()));
    assert_eq!(
        release
            .provenance
            .as_ref()
            .map(AssetReleaseProvenance::build_run_id),
        Some(build.id)
    );
    assert_eq!(release.aggregate_version, 2);
    release
        .publish_from_build(&asset, &build)
        .expect("exact publication replay");
    assert_eq!(release.aggregate_version, 2);

    let yanked_at = published_at + Duration::seconds(1);
    release.yank(yanked_at).expect("yank");
    assert_eq!(release.state, AssetReleaseState::Yanked);
    assert_eq!(release.yanked_at, Some(yanked_at));
    assert_eq!(
        (
            release.version.clone(),
            release.commit_sha.clone(),
            release.manifest_digest.clone(),
        ),
        immutable_identity
    );
    assert!(release.publish_from_build(&asset, &build).is_err());
}

#[test]
fn publication_profile_matches_the_exact_asset_kind() {
    let agent = asset(AssetKind::Agent);
    assert!(draft(&agent)
        .publish_skill(&agent, skill_artifact(), now() + Duration::seconds(1))
        .is_err());

    let skill = asset(AssetKind::Skill);
    assert!(draft(&skill)
        .publish_skill(&skill, oci_artifact(), now() + Duration::seconds(1))
        .is_err());
    let mut skill_release = draft(&skill);
    skill_release
        .publish_skill(&skill, skill_artifact(), now() + Duration::seconds(1))
        .expect("publish Skill bundle");
    assert_eq!(skill_release.state, AssetReleaseState::Published);
}

#[test]
fn archived_asset_cannot_create_or_publish_a_release() {
    let mut asset = asset(AssetKind::Mcp);
    let mut existing = draft(&asset);
    let mut published = draft(&asset);
    let existing_build =
        succeeded_hosted_build(asset.organization_id, asset.id, existing.id, now());
    let published_build =
        succeeded_hosted_build(asset.organization_id, asset.id, published.id, now());
    published
        .publish_from_build(&asset, &published_build)
        .expect("publish before archive");
    asset
        .archive(now() + Duration::seconds(1))
        .expect("archive");
    assert!(AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("2.0.0").expect("version"),
        GitCommitSha::parse("e".repeat(40)).expect("commit"),
        Sha256Digest::parse(format!("sha256:{}", "f".repeat(64))).expect("manifest"),
        now() + Duration::seconds(2),
    )
    .is_err());
    assert!(existing
        .publish_from_build(&asset, &existing_build)
        .is_err());
    published
        .publish_from_build(&asset, &published_build)
        .expect("exact publication replay after archive");
}

#[test]
fn malformed_restored_state_fails_closed() {
    let asset = asset(AssetKind::Agent);
    let mut release = draft(&asset);
    release.state = AssetReleaseState::Published;
    assert!(release.validate_for(&asset).is_err());

    let mut asset = asset;
    asset.state = AssetState::Archived;
    assert!(asset.validate().is_err());
}

#[test]
fn repository_writes_reject_forged_event_metadata_and_payloads() {
    let asset = asset(AssetKind::Agent);
    let mut created_event =
        AssetCreated::envelope(&asset, Uuid::now_v7()).expect("Asset created event");
    created_event.payload["name"] = serde_json::Value::String("Forged Asset".into());
    let create = CreateAssetWrite {
        asset: asset.clone(),
        event: created_event,
        idempotency: IdempotencyRequest::new("assets", "create", b"create").expect("idempotency"),
    };
    assert!(create.validate().is_err());

    let mut invalid_envelope =
        AssetCreated::envelope(&asset, Uuid::now_v7()).expect("Asset created event");
    invalid_envelope.event_id = Uuid::nil();
    let create = CreateAssetWrite {
        asset: asset.clone(),
        event: invalid_envelope,
        idempotency: IdempotencyRequest::new("assets", "invalid", b"invalid").expect("idempotency"),
    };
    assert!(create.validate().is_err());

    let draft = draft(&asset);
    let mut published = draft.clone();
    let build = succeeded_hosted_build(asset.organization_id, asset.id, draft.id, now());
    published
        .publish_from_build(&asset, &build)
        .expect("publish");
    let mut published_event =
        AssetReleasePublished::envelope(&published, Uuid::now_v7()).expect("published event");
    published_event.payload["artifact_digest"] =
        serde_json::Value::String(format!("sha256:{}", "0".repeat(64)));
    let transition = TransitionAssetReleaseWrite {
        release: published,
        expected_aggregate_version: draft.aggregate_version,
        event: published_event,
        idempotency: IdempotencyRequest::new("asset-releases", "publish", b"publish")
            .expect("idempotency"),
    };
    assert!(transition.validate().is_err());
}
