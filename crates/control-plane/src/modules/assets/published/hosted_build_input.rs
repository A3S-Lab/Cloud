use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, OrganizationId, Sha256Digest,
};
use crate::modules::sources::published::BuildRecipe;
use a3s_cloud_contracts::{agent_harness_compatibility_v1, AgentReleaseManifest};

/// Published, owner-independent snapshot of one admitted Code release
/// template. Consumers receive canonical bytes and identity, never the Assets
/// domain value that admitted them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedAgentReleaseTemplate {
    identity: Sha256Digest,
    canonical_acl: String,
}

impl HostedAgentReleaseTemplate {
    pub(in crate::modules::assets) fn from_validated_parts(
        identity: String,
        canonical_acl: String,
    ) -> Result<Self, String> {
        let value = Self {
            identity: Sha256Digest::parse(identity)?,
            canonical_acl,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let manifest = AgentReleaseManifest::parse(&self.canonical_acl)
            .map_err(|error| format!("published Agent release template is invalid: {error}"))?;
        manifest
            .verify_compatibility(&agent_harness_compatibility_v1())
            .map_err(|error| {
                format!("published Agent release template is incompatible: {error}")
            })?;
        let provenance = manifest
            .provenance()
            .iter()
            .map(|reference| reference.kind())
            .collect::<Vec<_>>();
        if manifest.canonical_acl() != self.canonical_acl
            || manifest.identity() != self.identity.as_str()
            || provenance != ["builder", "source"]
        {
            return Err("published Agent release template changed its canonical identity".into());
        }
        Ok(())
    }

    pub const fn identity(&self) -> &Sha256Digest {
        &self.identity
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }
}

/// The minimal immutable Assets-owned input required to build one exact
/// hosted Agent or MCP release.
///
/// This snapshot is not an Asset or release aggregate. Assets produces it only
/// after validating the aggregate binding and the pinned hosted Git manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedAssetBuildInputSnapshot {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    commit_sha: GitCommitSha,
    manifest_digest: Sha256Digest,
    recipe: BuildRecipe,
    agent_release_template: Option<HostedAgentReleaseTemplate>,
}

pub(in crate::modules::assets) struct ValidatedHostedAssetBuildInputProjection {
    pub(in crate::modules::assets) organization_id: OrganizationId,
    pub(in crate::modules::assets) asset_id: AssetId,
    pub(in crate::modules::assets) asset_release_id: AssetReleaseId,
    pub(in crate::modules::assets) commit_sha: GitCommitSha,
    pub(in crate::modules::assets) manifest_digest: Sha256Digest,
    pub(in crate::modules::assets) recipe: BuildRecipe,
    pub(in crate::modules::assets) agent_release_template: Option<HostedAgentReleaseTemplate>,
}

impl HostedAssetBuildInputSnapshot {
    pub const SCHEMA: &'static str = "a3s.cloud.hosted-asset-build-input.v1";

    pub(in crate::modules::assets) fn from_validated_release(
        projection: ValidatedHostedAssetBuildInputProjection,
    ) -> Result<Self, String> {
        if projection.organization_id.as_uuid().is_nil()
            || projection.asset_id.as_uuid().is_nil()
            || projection.asset_release_id.as_uuid().is_nil()
            || GitCommitSha::parse(projection.commit_sha.as_str())? != projection.commit_sha
            || Sha256Digest::parse(projection.manifest_digest.as_str())?
                != projection.manifest_digest
            || projection.recipe.clone().validate()? != projection.recipe
        {
            return Err("hosted Asset build input identity or recipe is invalid".into());
        }
        if let Some(template) = &projection.agent_release_template {
            template.validate()?;
        }
        Ok(Self {
            organization_id: projection.organization_id,
            asset_id: projection.asset_id,
            asset_release_id: projection.asset_release_id,
            commit_sha: projection.commit_sha,
            manifest_digest: projection.manifest_digest,
            recipe: projection.recipe,
            agent_release_template: projection.agent_release_template,
        })
    }

    pub const fn schema(&self) -> &'static str {
        Self::SCHEMA
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub fn commit_sha(&self) -> &GitCommitSha {
        &self.commit_sha
    }

    pub fn manifest_digest(&self) -> &Sha256Digest {
        &self.manifest_digest
    }

    pub fn recipe(&self) -> &BuildRecipe {
        &self.recipe
    }

    pub fn agent_release_template(&self) -> Option<&HostedAgentReleaseTemplate> {
        self.agent_release_template.as_ref()
    }
}
