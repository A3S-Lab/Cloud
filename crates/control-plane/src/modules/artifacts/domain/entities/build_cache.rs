use super::build_artifact::{validate_sha256, BuildArtifact};
use crate::modules::artifacts::domain::OciDescriptor;
use serde::{Deserialize, Serialize};

pub const BUILD_CACHE_SCHEMA: &str = "a3s.cloud.build-cache.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidatedBuildCache {
    pub schema: String,
    pub key: String,
    pub artifact: BuildArtifact,
    pub descriptor: OciDescriptor,
    pub content_bytes: u64,
    pub blob_count: usize,
}

impl ValidatedBuildCache {
    pub fn new(
        key: impl Into<String>,
        artifact: BuildArtifact,
        descriptor: OciDescriptor,
        content_bytes: u64,
        blob_count: usize,
    ) -> Result<Self, String> {
        let cache = Self {
            schema: BUILD_CACHE_SCHEMA.into(),
            key: key.into(),
            artifact,
            descriptor,
            content_bytes,
            blob_count,
        };
        cache.validate()?;
        Ok(cache)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUILD_CACHE_SCHEMA {
            return Err("build cache schema is unsupported".into());
        }
        validate_sha256(&self.key, "build cache key")?;
        self.artifact.validate()?;
        OciDescriptor::new(
            self.descriptor.media_type(),
            self.descriptor.digest(),
            self.descriptor.size(),
        )?;
        if self.content_bytes == 0 || self.blob_count == 0 {
            return Err("validated build cache has invalid bounds".into());
        }
        Ok(())
    }
}
