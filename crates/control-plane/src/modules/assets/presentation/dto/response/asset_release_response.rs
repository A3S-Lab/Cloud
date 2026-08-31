use crate::modules::assets::domain::{AssetRelease, AssetReleaseWrite};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReleaseArtifactResponse {
    pub kind: String,
    pub digest: String,
    pub media_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReleaseProvenanceResponse {
    pub build_run_id: Uuid,
    pub provenance_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReleaseAgentManifestResponse {
    pub identity: String,
    pub canonical_acl: String,
    pub archive_digest: String,
    pub archive_size_bytes: u64,
    pub source_content_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetReleaseResponse {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub id: Uuid,
    pub version: String,
    pub state: String,
    pub commit_sha: String,
    pub manifest_digest: String,
    pub artifact: Option<AssetReleaseArtifactResponse>,
    pub provenance: Option<AssetReleaseProvenanceResponse>,
    pub agent_release_manifest: Option<AssetReleaseAgentManifestResponse>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub yanked_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
}

impl From<AssetRelease> for AssetReleaseResponse {
    fn from(release: AssetRelease) -> Self {
        Self {
            organization_id: release.organization_id.as_uuid(),
            asset_id: release.asset_id.as_uuid(),
            id: release.id.as_uuid(),
            version: release.version.as_str().to_owned(),
            state: release.state.as_str().to_owned(),
            commit_sha: release.commit_sha.as_str().to_owned(),
            manifest_digest: release.manifest_digest.as_str().to_owned(),
            artifact: release
                .artifact
                .map(|artifact| AssetReleaseArtifactResponse {
                    kind: artifact.kind().as_str().to_owned(),
                    digest: artifact.digest().as_str().to_owned(),
                    media_type: artifact.media_type().to_owned(),
                    size_bytes: artifact.size_bytes(),
                }),
            provenance: release
                .provenance
                .map(|provenance| AssetReleaseProvenanceResponse {
                    build_run_id: provenance.build_run_id().as_uuid(),
                    provenance_digest: provenance.provenance_digest().as_str().to_owned(),
                }),
            agent_release_manifest: release.agent_release_manifest.map(|manifest| {
                AssetReleaseAgentManifestResponse {
                    identity: manifest.identity().to_string(),
                    canonical_acl: manifest.canonical_acl().into(),
                    archive_digest: manifest.archive_digest().to_string(),
                    archive_size_bytes: manifest.archive_size_bytes(),
                    source_content_digest: manifest.source_content_digest().to_string(),
                }
            }),
            aggregate_version: release.aggregate_version,
            created_at: release.created_at,
            updated_at: release.updated_at,
            published_at: release.published_at,
            yanked_at: release.yanked_at,
            replayed: None,
        }
    }
}

impl From<AssetReleaseWrite> for AssetReleaseResponse {
    fn from(write: AssetReleaseWrite) -> Self {
        let mut response = Self::from(write.release);
        response.replayed = Some(write.replayed);
        response
    }
}
