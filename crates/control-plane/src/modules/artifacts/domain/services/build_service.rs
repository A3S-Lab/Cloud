use crate::modules::sources::domain::{BuildPlatform, BuildRecipe};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const OCI_IMAGE_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
pub const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciBuildRequest {
    build_id: Uuid,
    source_directory: PathBuf,
    source_content_digest: String,
    recipe: BuildRecipe,
}

impl OciBuildRequest {
    pub fn new(
        build_id: Uuid,
        source_directory: PathBuf,
        source_content_digest: String,
        recipe: BuildRecipe,
    ) -> Result<Self, String> {
        if build_id.is_nil() {
            return Err("OCI build ID cannot be nil".into());
        }
        validate_absolute_path(&source_directory, "OCI build source directory")?;
        validate_sha256(&source_content_digest)?;
        let recipe = recipe.validate()?;
        Ok(Self {
            build_id,
            source_directory,
            source_content_digest,
            recipe,
        })
    }

    pub const fn build_id(&self) -> Uuid {
        self.build_id
    }

    pub fn source_directory(&self) -> &Path {
        &self.source_directory
    }

    pub fn source_content_digest(&self) -> &str {
        &self.source_content_digest
    }

    pub fn recipe(&self) -> &BuildRecipe {
        &self.recipe
    }

    pub fn recipe_digest(&self) -> Result<String, String> {
        self.recipe.digest()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OciDescriptor {
    media_type: String,
    digest: String,
    size: u64,
}

impl OciDescriptor {
    pub fn new(
        media_type: impl Into<String>,
        digest: impl Into<String>,
        size: u64,
    ) -> Result<Self, String> {
        let media_type = media_type.into();
        if !matches!(
            media_type.as_str(),
            OCI_IMAGE_INDEX_MEDIA_TYPE | OCI_IMAGE_MANIFEST_MEDIA_TYPE
        ) {
            return Err("OCI build descriptor must be an image index or image manifest".into());
        }
        let digest = digest.into();
        validate_sha256(&digest)?;
        if size == 0 {
            return Err("OCI build descriptor size must be positive".into());
        }
        Ok(Self {
            media_type,
            digest,
            size,
        })
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub const fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltOciArtifact {
    pub build_id: Uuid,
    pub source_content_digest: String,
    pub recipe_digest: String,
    pub descriptor: OciDescriptor,
    pub platforms: Vec<BuildPlatform>,
    pub oci_layout_directory: PathBuf,
    pub content_bytes: u64,
    pub blob_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildServiceError {
    #[error("OCI build request is invalid: {0}")]
    Invalid(String),
    #[error("OCI build identity conflicts with an existing build")]
    Conflict,
    #[error("BuildKit is unavailable: {0}")]
    Unavailable(String),
    #[error("BuildKit rejected the build")]
    BuildFailed,
    #[error("OCI build output failed integrity validation: {0}")]
    Integrity(String),
    #[error("OCI build storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait IBuildService: Send + Sync {
    async fn build(&self, request: &OciBuildRequest)
        -> Result<BuiltOciArtifact, BuildServiceError>;

    async fn remove(&self, build_id: Uuid) -> Result<(), BuildServiceError>;
}

fn validate_absolute_path(path: &Path, label: &str) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{label} must be UTF-8"))?;
    if !path.is_absolute()
        || text.is_empty()
        || text.len() > 4096
        || text.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{label} must be a bounded absolute path"));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if !value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err("OCI build digest must be a lowercase SHA-256 digest".into());
    }
    Ok(())
}
