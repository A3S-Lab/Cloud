use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus};
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetReleaseArtifact, AssetReleaseProvenance, AssetReleaseVersion, AssetState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, GitCommitSha, OrganizationId, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetReleaseState {
    Draft,
    Published,
    Yanked,
}

impl AssetReleaseState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
            Self::Yanked => "yanked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "draft" => Ok(Self::Draft),
            "published" => Ok(Self::Published),
            "yanked" => Ok(Self::Yanked),
            _ => Err(format!("unsupported Asset release state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRelease {
    pub id: AssetReleaseId,
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub version: AssetReleaseVersion,
    pub state: AssetReleaseState,
    pub commit_sha: GitCommitSha,
    pub manifest_digest: Sha256Digest,
    pub artifact: Option<AssetReleaseArtifact>,
    pub provenance: Option<AssetReleaseProvenance>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub yanked_at: Option<DateTime<Utc>>,
}

impl AssetRelease {
    pub fn draft(
        asset: &Asset,
        id: AssetReleaseId,
        version: AssetReleaseVersion,
        commit_sha: GitCommitSha,
        manifest_digest: Sha256Digest,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        asset.validate()?;
        if asset.state != AssetState::Active {
            return Err("archived Asset cannot create a release".into());
        }
        let created_at = canonical_timestamp(created_at);
        let release = Self {
            id,
            organization_id: asset.organization_id,
            asset_id: asset.id,
            version,
            state: AssetReleaseState::Draft,
            commit_sha,
            manifest_digest,
            artifact: None,
            provenance: None,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            published_at: None,
            yanked_at: None,
        };
        release.validate_for(asset)?;
        Ok(release)
    }

    pub fn publish_skill(
        &mut self,
        asset: &Asset,
        artifact: AssetReleaseArtifact,
        published_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if asset.kind != AssetKind::Skill {
            return Err(
                "Agent and MCP releases must be published by their successful BuildRun".into(),
            );
        }
        self.publish_with(asset, artifact, None, published_at)
    }

    pub fn publish_from_build(&mut self, asset: &Asset, build: &BuildRun) -> Result<(), String> {
        if !matches!(asset.kind, AssetKind::Agent | AssetKind::Mcp) {
            return Err("only Agent and MCP releases use hosted BuildRun publication".into());
        }
        let (artifact, provenance, published_at) = self.build_publication(build)?;
        self.publish_with(asset, artifact, Some(provenance), published_at)
    }

    pub fn validate_build_publication(
        &self,
        asset: &Asset,
        build: &BuildRun,
    ) -> Result<(), String> {
        self.validate_for(asset)?;
        if !matches!(asset.kind, AssetKind::Agent | AssetKind::Mcp)
            || !matches!(
                self.state,
                AssetReleaseState::Published | AssetReleaseState::Yanked
            )
        {
            return Err("Asset release is not a hosted build publication".into());
        }
        let (artifact, provenance, published_at) = self.build_publication(build)?;
        if self.artifact.as_ref() != Some(&artifact)
            || self.provenance.as_ref() != Some(&provenance)
            || self.published_at != Some(published_at)
        {
            return Err("Asset release changed its immutable BuildRun publication".into());
        }
        Ok(())
    }

    fn build_publication(
        &self,
        build: &BuildRun,
    ) -> Result<(AssetReleaseArtifact, AssetReleaseProvenance, DateTime<Utc>), String> {
        if build.organization_id != self.organization_id
            || build.asset_id() != Some(self.asset_id)
            || build.asset_release_id() != Some(self.id)
            || build.status != BuildRunStatus::Succeeded
        {
            return Err("successful BuildRun does not own this Asset release".into());
        }
        let evidence = build
            .evidence
            .as_deref()
            .ok_or_else(|| "successful hosted BuildRun has no verified evidence".to_owned())?;
        if evidence.commit_sha != self.commit_sha.as_str()
            || evidence.manifest_digest.as_deref() != Some(self.manifest_digest.as_str())
        {
            return Err("hosted BuildRun evidence changed the release source identity".into());
        }
        let published = build
            .published_artifact
            .as_ref()
            .ok_or_else(|| "successful hosted BuildRun has no published artifact".to_owned())?;
        let artifact = AssetReleaseArtifact::oci_service(
            Sha256Digest::parse(&published.digest)?,
            published.media_type.clone(),
            published.size_bytes,
        )?;
        let provenance = AssetReleaseProvenance::new(
            build.id,
            Sha256Digest::parse(&evidence.provenance_digest)?,
        )?;
        let published_at = build
            .finished_at
            .ok_or_else(|| "successful hosted BuildRun has no finish time".to_owned())?;
        Ok((artifact, provenance, published_at))
    }

    fn publish_with(
        &mut self,
        asset: &Asset,
        artifact: AssetReleaseArtifact,
        provenance: Option<AssetReleaseProvenance>,
        published_at: DateTime<Utc>,
    ) -> Result<(), String> {
        self.validate_for(asset)?;
        artifact.validate_for(asset.kind)?;
        if let Some(provenance) = &provenance {
            provenance.validate()?;
        }
        match self.state {
            AssetReleaseState::Draft => {}
            AssetReleaseState::Published
                if self.artifact.as_ref() == Some(&artifact) && self.provenance == provenance =>
            {
                return Ok(())
            }
            AssetReleaseState::Published => {
                return Err("published Asset release artifact is immutable".into())
            }
            AssetReleaseState::Yanked => {
                return Err("yanked Asset release cannot be published".into())
            }
        }
        if asset.state != AssetState::Active {
            return Err("archived Asset cannot publish a release".into());
        }
        let published_at = canonical_timestamp(published_at);
        self.ensure_transition_time(published_at)?;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Asset release aggregate version overflowed".to_owned())?;
        self.state = AssetReleaseState::Published;
        self.artifact = Some(artifact);
        self.provenance = provenance;
        self.updated_at = published_at;
        self.published_at = Some(published_at);
        Ok(())
    }

    pub fn yank(&mut self, yanked_at: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        match self.state {
            AssetReleaseState::Draft => return Err("draft Asset release cannot be yanked".into()),
            AssetReleaseState::Published => {}
            AssetReleaseState::Yanked => return Ok(()),
        }
        let yanked_at = canonical_timestamp(yanked_at);
        self.ensure_transition_time(yanked_at)?;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Asset release aggregate version overflowed".to_owned())?;
        self.state = AssetReleaseState::Yanked;
        self.updated_at = yanked_at;
        self.yanked_at = Some(yanked_at);
        Ok(())
    }

    pub fn validate_for(&self, asset: &Asset) -> Result<(), String> {
        asset.validate()?;
        self.validate()?;
        if self.organization_id != asset.organization_id || self.asset_id != asset.id {
            return Err("Asset release does not belong to its Asset".into());
        }
        if let Some(artifact) = &self.artifact {
            artifact.validate_for(asset.kind)?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || AssetReleaseVersion::parse(self.version.as_str())? != self.version
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || Sha256Digest::parse(self.manifest_digest.as_str())? != self.manifest_digest
        {
            return Err(
                "Asset release identity, source, version, or timestamps are invalid".into(),
            );
        }
        if let Some(artifact) = &self.artifact {
            artifact.validate()?;
        }
        if let Some(provenance) = &self.provenance {
            provenance.validate()?;
        }
        let publication_binding_is_valid = self.artifact.as_ref().is_none_or(|artifact| {
            matches!(
                artifact.kind(),
                crate::modules::assets::domain::AssetReleaseArtifactKind::OciService
            ) == self.provenance.is_some()
        });
        let state_is_valid = match self.state {
            AssetReleaseState::Draft => {
                self.artifact.is_none()
                    && self.provenance.is_none()
                    && self.published_at.is_none()
                    && self.yanked_at.is_none()
            }
            AssetReleaseState::Published => {
                self.artifact.is_some()
                    && publication_binding_is_valid
                    && self.published_at == Some(self.updated_at)
                    && self.yanked_at.is_none()
            }
            AssetReleaseState::Yanked => {
                self.artifact.is_some()
                    && publication_binding_is_valid
                    && self.published_at.is_some_and(|published_at| {
                        published_at >= self.created_at
                            && published_at <= self.updated_at
                            && published_at == canonical_timestamp(published_at)
                    })
                    && self.yanked_at == Some(self.updated_at)
            }
        };
        if !state_is_valid {
            return Err("Asset release state transition is inconsistent".into());
        }
        Ok(())
    }

    fn ensure_transition_time(&self, at: DateTime<Utc>) -> Result<(), String> {
        if at < self.updated_at {
            return Err("Asset release transition time regressed".into());
        }
        Ok(())
    }
}

impl AssetReleaseArtifact {
    fn validate_for(&self, asset_kind: AssetKind) -> Result<(), String> {
        if self.supports(asset_kind) {
            Ok(())
        } else {
            Err("Asset kind and release artifact profile do not match".into())
        }
    }
}
