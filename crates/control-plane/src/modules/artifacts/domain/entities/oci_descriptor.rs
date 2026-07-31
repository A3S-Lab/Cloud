use super::build_artifact::validate_sha256;
use serde::{Deserialize, Serialize};

pub const OCI_IMAGE_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
pub const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

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
            return Err("OCI descriptor must be an image index or image manifest".into());
        }
        let digest = digest.into();
        validate_sha256(&digest, "OCI descriptor digest")?;
        if size == 0 {
            return Err("OCI descriptor size must be positive".into());
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
