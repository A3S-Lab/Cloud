use crate::modules::artifacts::domain::OciDescriptor;
use crate::modules::sources::domain::BuildPlatform;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildArtifact {
    pub uri: String,
    pub digest: String,
    pub media_type: String,
    pub size_bytes: u64,
}

impl BuildArtifact {
    pub fn new(
        uri: impl Into<String>,
        digest: impl Into<String>,
        media_type: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let artifact = Self {
            uri: uri.into(),
            digest: digest.into(),
            media_type: media_type.into(),
            size_bytes,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        let Some((scheme, remainder)) = self.uri.split_once("://") else {
            return Err("build artifact URI must contain a scheme".into());
        };
        if self.uri.len() > 4096
            || self.uri.contains(['\0', '\r', '\n'])
            || scheme.is_empty()
            || remainder.is_empty()
            || !scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        {
            return Err("build artifact URI is invalid".into());
        }
        validate_sha256(&self.digest, "build artifact digest")?;
        if self.media_type.trim().is_empty()
            || self.media_type.len() > 255
            || self.media_type.contains(['\0', '\r', '\n'])
            || self.size_bytes == 0
        {
            return Err("build artifact media type or size is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidatedOciBuildOutput {
    pub artifact: BuildArtifact,
    pub descriptor: OciDescriptor,
    pub platforms: Vec<BuildPlatform>,
    pub content_bytes: u64,
    pub blob_count: usize,
}

impl ValidatedOciBuildOutput {
    pub fn validate(&self) -> Result<(), String> {
        self.artifact.validate()?;
        OciDescriptor::new(
            self.descriptor.media_type(),
            self.descriptor.digest(),
            self.descriptor.size(),
        )?;
        if self.platforms.is_empty()
            || self.platforms.len() > 8
            || self.content_bytes == 0
            || self.blob_count == 0
        {
            return Err("validated OCI build output has invalid bounds".into());
        }
        let mut unique = std::collections::BTreeSet::new();
        for platform in &self.platforms {
            let parsed = BuildPlatform::parse(platform.as_str())?;
            if !unique.insert(parsed) {
                return Err("validated OCI build output platforms must be unique".into());
            }
        }
        Ok(())
    }
}

pub(super) fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if !value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(format!("{label} must be a lowercase SHA-256 digest"));
    }
    Ok(())
}
