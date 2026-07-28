use crate::modules::artifacts::domain::{
    OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE,
};
use crate::modules::assets::domain::AssetKind;
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};

pub const SKILL_BUNDLE_MEDIA_TYPE: &str = "application/vnd.a3s.skill.bundle.v1+tar";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetReleaseArtifactKind {
    OciService,
    SkillBundle,
}

impl AssetReleaseArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OciService => "oci_service",
            Self::SkillBundle => "skill_bundle",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "oci_service" => Ok(Self::OciService),
            "skill_bundle" => Ok(Self::SkillBundle),
            _ => Err(format!("unsupported Asset release artifact kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetReleaseArtifact {
    kind: AssetReleaseArtifactKind,
    digest: Sha256Digest,
    media_type: String,
    size_bytes: u64,
}

impl AssetReleaseArtifact {
    pub fn oci_service(
        digest: Sha256Digest,
        media_type: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let value = Self {
            kind: AssetReleaseArtifactKind::OciService,
            digest,
            media_type: media_type.into(),
            size_bytes,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn skill_bundle(digest: Sha256Digest, size_bytes: u64) -> Result<Self, String> {
        let value = Self {
            kind: AssetReleaseArtifactKind::SkillBundle,
            digest,
            media_type: SKILL_BUNDLE_MEDIA_TYPE.into(),
            size_bytes,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(
        kind: AssetReleaseArtifactKind,
        digest: Sha256Digest,
        media_type: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let value = Self {
            kind,
            digest,
            media_type: media_type.into(),
            size_bytes,
        };
        value.validate()?;
        Ok(value)
    }

    pub const fn kind(&self) -> AssetReleaseArtifactKind {
        self.kind
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn supports(&self, asset_kind: AssetKind) -> bool {
        matches!(
            (asset_kind, self.kind),
            (
                AssetKind::Agent | AssetKind::Mcp,
                AssetReleaseArtifactKind::OciService
            ) | (AssetKind::Skill, AssetReleaseArtifactKind::SkillBundle)
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.digest.as_str())? != self.digest || self.size_bytes == 0 {
            return Err("Asset release artifact identity or size is invalid".into());
        }
        let media_type_is_valid = match self.kind {
            AssetReleaseArtifactKind::OciService => matches!(
                self.media_type.as_str(),
                OCI_IMAGE_INDEX_MEDIA_TYPE | OCI_IMAGE_MANIFEST_MEDIA_TYPE
            ),
            AssetReleaseArtifactKind::SkillBundle => self.media_type == SKILL_BUNDLE_MEDIA_TYPE,
        };
        if !media_type_is_valid {
            return Err("Asset release artifact media type does not match its profile".into());
        }
        Ok(())
    }
}
