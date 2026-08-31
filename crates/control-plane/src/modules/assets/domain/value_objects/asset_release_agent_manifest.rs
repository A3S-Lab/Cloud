use super::{AssetReleaseArtifact, AssetReleaseProvenance};
use crate::modules::artifacts::published::{HostedAgentReleaseManifest, HostedBuildOutcome};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_cloud_contracts::{
    agent_harness_compatibility_v1, agent_release_builder_uri, agent_release_manifest_archive,
    agent_release_source_uri, artifact_uri, AgentReleaseManifest,
};
use serde::{Deserialize, Serialize};

/// Durable Assets-owned copy of one exact final A3S Code release manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetReleaseAgentManifest {
    identity: Sha256Digest,
    canonical_acl: String,
    archive_digest: Sha256Digest,
    archive_size_bytes: u64,
    source_content_digest: Sha256Digest,
}

impl AssetReleaseAgentManifest {
    pub fn from_hosted_outcome(outcome: &HostedBuildOutcome) -> Result<Self, String> {
        outcome.validate()?;
        let manifest = outcome
            .agent_release_manifest()
            .ok_or_else(|| "hosted Agent build omitted its final release manifest".to_owned())?;
        let source_content_digest = outcome
            .source_content_digest()
            .ok_or_else(|| "hosted Agent build omitted its source content digest".to_owned())?;
        Self::from_hosted_manifest(manifest, source_content_digest.clone())
    }

    fn from_hosted_manifest(
        manifest: &HostedAgentReleaseManifest,
        source_content_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            identity: manifest.identity().clone(),
            canonical_acl: manifest.canonical_acl().into(),
            archive_digest: manifest.archive_digest().clone(),
            archive_size_bytes: manifest.archive_size_bytes(),
            source_content_digest,
        };
        value.validate_shape()?;
        Ok(value)
    }

    pub fn restore(
        identity: Sha256Digest,
        canonical_acl: String,
        archive_digest: Sha256Digest,
        archive_size_bytes: u64,
        source_content_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            identity,
            canonical_acl,
            archive_digest,
            archive_size_bytes,
            source_content_digest,
        };
        value.validate_shape()?;
        Ok(value)
    }

    pub fn validate_for(
        &self,
        artifact: &AssetReleaseArtifact,
        provenance: &AssetReleaseProvenance,
    ) -> Result<(), String> {
        artifact.validate()?;
        provenance.validate()?;
        let manifest = self.validate_shape()?;
        let source_uri = agent_release_source_uri(self.source_content_digest.as_str())?;
        let builder_uri = agent_release_builder_uri(provenance.build_run_id().as_uuid())?;
        if manifest.artifact().digest() != artifact.digest().as_str()
            || manifest.artifact().media_type() != artifact.media_type()
            || manifest.provenance().len() != 2
            || !manifest.provenance().iter().any(|reference| {
                reference.kind() == "source"
                    && reference.uri() == source_uri
                    && reference.digest() == self.source_content_digest.as_str()
            })
            || !manifest.provenance().iter().any(|reference| {
                reference.kind() == "builder"
                    && reference.uri() == builder_uri
                    && reference.digest() == provenance.provenance_digest().as_str()
            })
        {
            return Err("Agent release manifest changed its immutable publication".into());
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<AgentReleaseManifest, String> {
        let manifest = AgentReleaseManifest::parse(&self.canonical_acl)
            .map_err(|error| format!("stored Agent release manifest is invalid: {error}"))?;
        manifest
            .verify_compatibility(&agent_harness_compatibility_v1())
            .map_err(|error| format!("stored Agent release manifest is incompatible: {error}"))?;
        let archive = agent_release_manifest_archive(self.canonical_acl.as_bytes())?;
        if manifest.canonical_acl() != self.canonical_acl
            || manifest.identity() != self.identity.as_str()
            || Sha256Digest::from_bytes(&archive) != self.archive_digest
            || archive.len() as u64 != self.archive_size_bytes
            || Sha256Digest::parse(self.source_content_digest.as_str())?
                != self.source_content_digest
        {
            return Err("stored Agent release manifest changed its exact bytes".into());
        }
        Ok(manifest)
    }

    pub const fn identity(&self) -> &Sha256Digest {
        &self.identity
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn archive_digest(&self) -> &Sha256Digest {
        &self.archive_digest
    }

    pub const fn archive_size_bytes(&self) -> u64 {
        self.archive_size_bytes
    }

    pub const fn source_content_digest(&self) -> &Sha256Digest {
        &self.source_content_digest
    }

    pub fn archive_uri(&self) -> Result<String, String> {
        artifact_uri(self.archive_digest.as_str())
    }
}
