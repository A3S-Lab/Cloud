use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, OrganizationId, Sha256Digest,
};
use serde::{Deserialize, Serialize};

pub const HOSTED_ASSET_BUILD_REQUESTED_EVENT_KEY: &str = "asset.hosted-build.requested";
pub const HOSTED_ASSET_BUILD_REQUESTED_SCHEMA_VERSION: u32 = 1;

/// Assets-owned immutable request for Artifacts to build one hosted release.
///
/// Assets emits this fact only after it has admitted an active Agent or MCP
/// release. Skill bundle publication never enters this contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedAssetBuildRequestedFact {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    commit_sha: String,
    manifest_digest: String,
}

impl HostedAssetBuildRequestedFact {
    pub(in crate::modules::assets) fn new(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        commit_sha: String,
        manifest_digest: String,
    ) -> Result<Self, String> {
        let fact = Self {
            organization_id,
            asset_id,
            asset_release_id,
            commit_sha,
            manifest_digest,
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
        {
            return Err("hosted Asset build request identity is invalid".into());
        }
        GitCommitSha::parse(&self.commit_sha)?;
        Sha256Digest::parse(&self.manifest_digest)?;
        Ok(())
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

    pub fn commit_sha(&self) -> &str {
        &self.commit_sha
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }
}
