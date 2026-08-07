use crate::modules::artifacts::domain::BuildRun;
use crate::modules::assets::domain::{Asset, AssetRelease, McpServiceProfileBinding};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetCreated {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub name: String,
    pub kind: String,
}

impl AssetCreated {
    pub fn envelope(
        asset: &Asset,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(asset_event(
            asset,
            "asset.asset.created",
            correlation_id,
            serde_json::to_value(Self {
                organization_id: asset.organization_id.as_uuid(),
                asset_id: asset.id.as_uuid(),
                name: asset.name.as_str().into(),
                kind: asset.kind.as_str().into(),
            })?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetArchived {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
}

impl AssetArchived {
    pub fn envelope(
        asset: &Asset,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(asset_event(
            asset,
            "asset.asset.archived",
            correlation_id,
            serde_json::to_value(Self {
                organization_id: asset.organization_id.as_uuid(),
                asset_id: asset.id.as_uuid(),
            })?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReleaseDrafted {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub version: String,
    pub commit_sha: String,
    pub manifest_digest: String,
}

impl AssetReleaseDrafted {
    pub fn envelope(
        release: &AssetRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(release_event(
            release,
            "asset.release.drafted",
            correlation_id,
            serde_json::to_value(Self {
                organization_id: release.organization_id.as_uuid(),
                asset_id: release.asset_id.as_uuid(),
                asset_release_id: release.id.as_uuid(),
                version: release.version.as_str().into(),
                commit_sha: release.commit_sha.as_str().into(),
                manifest_digest: release.manifest_digest.as_str().into(),
            })?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReleasePublished {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub version: String,
    pub artifact_kind: String,
    pub artifact_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_digest: Option<String>,
}

impl AssetReleasePublished {
    pub fn envelope(
        release: &AssetRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        let artifact = release
            .artifact
            .as_ref()
            .ok_or_else(|| "published Asset release has no artifact".to_owned())?;
        let payload = serde_json::to_value(Self {
            organization_id: release.organization_id.as_uuid(),
            asset_id: release.asset_id.as_uuid(),
            asset_release_id: release.id.as_uuid(),
            version: release.version.as_str().into(),
            artifact_kind: artifact.kind().as_str().into(),
            artifact_digest: artifact.digest().as_str().into(),
            build_run_id: release
                .provenance
                .as_ref()
                .map(|provenance| provenance.build_run_id().as_uuid()),
            provenance_digest: release
                .provenance
                .as_ref()
                .map(|provenance| provenance.provenance_digest().as_str().into()),
        })
        .map_err(|error| error.to_string())?;
        Ok(release_event(
            release,
            "asset.release.published",
            correlation_id,
            payload,
        ))
    }

    pub fn envelope_from_build(
        release: &AssetRelease,
        build: &BuildRun,
    ) -> Result<DomainEventEnvelope, String> {
        if release
            .provenance
            .as_ref()
            .is_none_or(|provenance| provenance.build_run_id() != build.id)
            || release.organization_id != build.organization_id
            || build.asset_id() != Some(release.asset_id)
            || build.asset_release_id() != Some(release.id)
        {
            return Err("published Asset release does not match its BuildRun provenance".into());
        }
        let mut event = Self::envelope(release, build.operation_id.as_uuid())?;
        event.causation_id = Some(build.id.as_uuid());
        Ok(event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReleaseYanked {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServiceProfileBound {
    pub organization_id: Uuid,
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub profile_digest: String,
}

impl McpServiceProfileBound {
    pub fn envelope(
        binding: &McpServiceProfileBinding,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        binding.validate()?;
        let payload = serde_json::to_value(Self {
            organization_id: binding.organization_id.as_uuid(),
            asset_id: binding.asset_id.as_uuid(),
            asset_release_id: binding.asset_release_id.as_uuid(),
            profile_digest: binding.profile.digest().to_string(),
        })
        .map_err(|error| error.to_string())?;
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "asset.mcp-service-profile.bound".into(),
            schema_version: 1,
            organization_id: binding.organization_id.as_uuid(),
            aggregate_id: binding.asset_release_id.as_uuid(),
            aggregate_version: 1,
            occurred_at: binding.created_at,
            correlation_id,
            causation_id: None,
            payload,
        })
    }
}

impl AssetReleaseYanked {
    pub fn envelope(
        release: &AssetRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(release_event(
            release,
            "asset.release.yanked",
            correlation_id,
            serde_json::to_value(Self {
                organization_id: release.organization_id.as_uuid(),
                asset_id: release.asset_id.as_uuid(),
                asset_release_id: release.id.as_uuid(),
                version: release.version.as_str().into(),
            })?,
        ))
    }
}

fn asset_event(
    asset: &Asset,
    event_key: &str,
    correlation_id: Uuid,
    payload: serde_json::Value,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: asset.organization_id.as_uuid(),
        aggregate_id: asset.id.as_uuid(),
        aggregate_version: asset.aggregate_version,
        occurred_at: asset.updated_at,
        correlation_id,
        causation_id: None,
        payload,
    }
}

fn release_event(
    release: &AssetRelease,
    event_key: &str,
    correlation_id: Uuid,
    payload: serde_json::Value,
) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: if event_key == "asset.release.published" && release.provenance.is_some() {
            2
        } else {
            1
        },
        organization_id: release.organization_id.as_uuid(),
        aggregate_id: release.id.as_uuid(),
        aggregate_version: release.aggregate_version,
        occurred_at: release.updated_at,
        correlation_id,
        causation_id: None,
        payload,
    }
}
