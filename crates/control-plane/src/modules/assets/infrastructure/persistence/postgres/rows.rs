use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseAgentManifest, AssetReleaseArtifact,
    AssetReleaseArtifactKind, AssetReleaseProvenance, AssetReleaseState, AssetReleaseVersion,
    AssetState,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, GitCommitSha, OrganizationId, RepositoryError,
    ResourceName, Sha256Digest,
};
use a3s_orm::{DecodeError, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) const SELECT_ASSETS: &str = "select a.id, a.organization_id, a.name, a.kind, a.state, a.aggregate_version, a.created_at, a.updated_at, a.archived_at from assets a";
pub(super) const SELECT_RELEASES: &str = "select r.id, r.organization_id, r.asset_id, r.version, r.state, r.commit_sha, r.manifest_digest, r.artifact_kind, r.artifact_digest, r.artifact_media_type, r.artifact_size_bytes, r.build_run_id, r.provenance_digest, r.agent_manifest_identity, r.agent_manifest_acl, r.agent_manifest_archive_digest, r.agent_manifest_archive_size_bytes, r.agent_manifest_source_content_digest, r.aggregate_version, r.created_at, r.updated_at, r.published_at, r.yanked_at from asset_releases r";

pub(super) struct AssetRow {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    kind: String,
    state: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

pub(super) struct AssetReleaseRow {
    id: Uuid,
    organization_id: Uuid,
    asset_id: Uuid,
    version: String,
    state: String,
    commit_sha: String,
    manifest_digest: String,
    artifact_kind: Option<String>,
    artifact_digest: Option<String>,
    artifact_media_type: Option<String>,
    artifact_size_bytes: Option<u64>,
    build_run_id: Option<Uuid>,
    provenance_digest: Option<String>,
    agent_manifest_identity: Option<String>,
    agent_manifest_acl: Option<String>,
    agent_manifest_archive_digest: Option<String>,
    agent_manifest_archive_size_bytes: Option<u64>,
    agent_manifest_source_content_digest: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
    yanked_at: Option<DateTime<Utc>>,
}

impl FromRow for AssetRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            name: decode(row, 2)?,
            kind: decode(row, 3)?,
            state: decode(row, 4)?,
            aggregate_version: decode(row, 5)?,
            created_at: decode(row, 6)?,
            updated_at: decode(row, 7)?,
            archived_at: decode(row, 8)?,
        })
    }
}

impl FromRow for AssetReleaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            asset_id: decode(row, 2)?,
            version: decode(row, 3)?,
            state: decode(row, 4)?,
            commit_sha: decode(row, 5)?,
            manifest_digest: decode(row, 6)?,
            artifact_kind: decode(row, 7)?,
            artifact_digest: decode(row, 8)?,
            artifact_media_type: decode(row, 9)?,
            artifact_size_bytes: decode(row, 10)?,
            build_run_id: decode(row, 11)?,
            provenance_digest: decode(row, 12)?,
            agent_manifest_identity: decode(row, 13)?,
            agent_manifest_acl: decode(row, 14)?,
            agent_manifest_archive_digest: decode(row, 15)?,
            agent_manifest_archive_size_bytes: decode(row, 16)?,
            agent_manifest_source_content_digest: decode(row, 17)?,
            aggregate_version: decode(row, 18)?,
            created_at: decode(row, 19)?,
            updated_at: decode(row, 20)?,
            published_at: decode(row, 21)?,
            yanked_at: decode(row, 22)?,
        })
    }
}

impl AssetRow {
    pub(super) fn asset(self) -> Result<Asset, RepositoryError> {
        let asset = Asset {
            id: AssetId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            name: ResourceName::parse(self.name).map_err(stored("Asset name"))?,
            kind: AssetKind::parse(&self.kind).map_err(stored("Asset kind"))?,
            state: AssetState::parse(&self.state).map_err(stored("Asset state"))?,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            archived_at: self.archived_at,
        };
        asset.validate().map_err(stored("Asset aggregate"))?;
        Ok(asset)
    }
}

impl AssetReleaseRow {
    pub(super) fn release(self) -> Result<AssetRelease, RepositoryError> {
        let artifact = restore_artifact(
            self.artifact_kind,
            self.artifact_digest,
            self.artifact_media_type,
            self.artifact_size_bytes,
        )?;
        let provenance = restore_provenance(self.build_run_id, self.provenance_digest)?;
        let agent_release_manifest = restore_agent_manifest(
            self.agent_manifest_identity,
            self.agent_manifest_acl,
            self.agent_manifest_archive_digest,
            self.agent_manifest_archive_size_bytes,
            self.agent_manifest_source_content_digest,
        )?;
        let release = AssetRelease {
            id: AssetReleaseId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            asset_id: AssetId::from_uuid(self.asset_id),
            version: AssetReleaseVersion::parse(self.version)
                .map_err(stored("Asset release version"))?,
            state: AssetReleaseState::parse(&self.state).map_err(stored("Asset release state"))?,
            commit_sha: GitCommitSha::parse(self.commit_sha)
                .map_err(stored("Asset release commit"))?,
            manifest_digest: Sha256Digest::parse(self.manifest_digest)
                .map_err(stored("Asset release manifest digest"))?,
            artifact,
            provenance,
            agent_release_manifest,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            published_at: self.published_at,
            yanked_at: self.yanked_at,
        };
        release
            .validate()
            .map_err(stored("Asset release aggregate"))?;
        Ok(release)
    }
}

fn restore_agent_manifest(
    identity: Option<String>,
    canonical_acl: Option<String>,
    archive_digest: Option<String>,
    archive_size_bytes: Option<u64>,
    source_content_digest: Option<String>,
) -> Result<Option<AssetReleaseAgentManifest>, RepositoryError> {
    match (
        identity,
        canonical_acl,
        archive_digest,
        archive_size_bytes,
        source_content_digest,
    ) {
        (None, None, None, None, None) => Ok(None),
        (
            Some(identity),
            Some(canonical_acl),
            Some(archive_digest),
            Some(archive_size_bytes),
            Some(source_content_digest),
        ) => AssetReleaseAgentManifest::restore(
            Sha256Digest::parse(identity).map_err(stored("Agent manifest identity"))?,
            canonical_acl,
            Sha256Digest::parse(archive_digest).map_err(stored("Agent manifest archive digest"))?,
            archive_size_bytes,
            Sha256Digest::parse(source_content_digest)
                .map_err(stored("Agent manifest source content digest"))?,
        )
        .map(Some)
        .map_err(stored("Agent release manifest")),
        _ => Err(RepositoryError::Storage(
            "stored Agent release manifest is incomplete".into(),
        )),
    }
}

fn restore_provenance(
    build_run_id: Option<Uuid>,
    provenance_digest: Option<String>,
) -> Result<Option<AssetReleaseProvenance>, RepositoryError> {
    match (build_run_id, provenance_digest) {
        (None, None) => Ok(None),
        (Some(build_run_id), Some(provenance_digest)) => AssetReleaseProvenance::new(
            BuildRunId::from_uuid(build_run_id),
            Sha256Digest::parse(provenance_digest)
                .map_err(stored("Asset release provenance digest"))?,
        )
        .map(Some)
        .map_err(stored("Asset release provenance")),
        _ => Err(RepositoryError::Storage(
            "stored Asset release provenance is incomplete".into(),
        )),
    }
}

fn restore_artifact(
    kind: Option<String>,
    digest: Option<String>,
    media_type: Option<String>,
    size_bytes: Option<u64>,
) -> Result<Option<AssetReleaseArtifact>, RepositoryError> {
    match (kind, digest, media_type, size_bytes) {
        (None, None, None, None) => Ok(None),
        (Some(kind), Some(digest), Some(media_type), Some(size_bytes)) => {
            AssetReleaseArtifact::restore(
                AssetReleaseArtifactKind::parse(&kind)
                    .map_err(stored("Asset release artifact kind"))?,
                Sha256Digest::parse(digest).map_err(stored("Asset release artifact digest"))?,
                media_type,
                size_bytes,
            )
            .map(Some)
            .map_err(stored("Asset release artifact"))
        }
        _ => Err(RepositoryError::Storage(
            "stored Asset release artifact is incomplete".into(),
        )),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
