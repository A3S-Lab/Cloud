use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{validate_single_line, validate_uuid};

pub const NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE: &str = "application/vnd.a3s.directory.v1+tar";
const ARTIFACT_URI_PREFIX: &str = "a3s-cloud-artifact://sha256/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeArtifactDownloadRequest {
    pub schema: String,
    pub node_id: Uuid,
    pub command_id: Uuid,
    pub spec_digest: String,
    pub mount_name: String,
    pub artifact_uri: String,
    pub artifact_digest: String,
    pub artifact_media_type: String,
}

impl NodeArtifactDownloadRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.node-artifact-download-request.v1";

    pub fn new(
        node_id: Uuid,
        command_id: Uuid,
        spec_digest: impl Into<String>,
        mount_name: impl Into<String>,
        artifact: &ArtifactRef,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            node_id,
            command_id,
            spec_digest: spec_digest.into(),
            mount_name: mount_name.into(),
            artifact_uri: artifact.uri.clone(),
            artifact_digest: artifact.digest.clone(),
            artifact_media_type: artifact.media_type.clone(),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node artifact download request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("command_id", self.command_id)?;
        validate_lower_sha256("Runtime specification digest", &self.spec_digest)?;
        validate_single_line("artifact mount name", &self.mount_name, 255)?;
        self.artifact()?.validate()?;
        validate_directory_artifact(
            &self.artifact_uri,
            &self.artifact_digest,
            &self.artifact_media_type,
        )
    }

    pub fn artifact(&self) -> Result<ArtifactRef, String> {
        let artifact = ArtifactRef {
            uri: self.artifact_uri.clone(),
            digest: self.artifact_digest.clone(),
            media_type: self.artifact_media_type.clone(),
        };
        artifact.validate()?;
        Ok(artifact)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeArtifactUploadRequest {
    pub schema: String,
    pub node_id: Uuid,
    pub command_id: Uuid,
    pub spec_digest: String,
    pub output_name: String,
    pub digest: String,
    pub media_type: String,
    pub size_bytes: u64,
}

impl NodeArtifactUploadRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.node-artifact-upload-request.v1";

    pub fn new(
        node_id: Uuid,
        command_id: Uuid,
        spec_digest: impl Into<String>,
        output_name: impl Into<String>,
        digest: impl Into<String>,
        media_type: impl Into<String>,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            node_id,
            command_id,
            spec_digest: spec_digest.into(),
            output_name: output_name.into(),
            digest: digest.into(),
            media_type: media_type.into(),
            size_bytes,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node artifact upload request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("command_id", self.command_id)?;
        validate_lower_sha256("Runtime specification digest", &self.spec_digest)?;
        validate_single_line("artifact output name", &self.output_name, 255)?;
        validate_lower_sha256("artifact digest", &self.digest)?;
        validate_supported_media_type(&self.media_type)?;
        if self.size_bytes == 0 {
            return Err("artifact upload size must be positive".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeArtifactUploadReceipt {
    pub schema: String,
    pub node_id: Uuid,
    pub command_id: Uuid,
    pub spec_digest: String,
    pub artifact: RuntimeOutputArtifact,
    pub replayed: bool,
}

impl NodeArtifactUploadReceipt {
    pub const SCHEMA: &'static str = "a3s.cloud.node-artifact-upload-receipt.v1";

    pub fn validate_against(&self, request: &NodeArtifactUploadRequest) -> Result<(), String> {
        request.validate()?;
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node artifact upload receipt schema {:?}",
                self.schema
            ));
        }
        if self.node_id != request.node_id
            || self.command_id != request.command_id
            || self.spec_digest != request.spec_digest
            || self.artifact.name != request.output_name
            || self.artifact.artifact.digest != request.digest
            || self.artifact.artifact.media_type != request.media_type
            || self.artifact.size_bytes != request.size_bytes
        {
            return Err("artifact upload receipt changed the transfer identity".into());
        }
        validate_cloud_artifact(&self.artifact.artifact)
    }
}

pub fn artifact_uri(digest: &str) -> Result<String, String> {
    validate_lower_sha256("artifact digest", digest)?;
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| "artifact digest must use sha256".to_owned())?;
    Ok(format!("{ARTIFACT_URI_PREFIX}{hex}"))
}

pub fn validate_cloud_artifact(artifact: &ArtifactRef) -> Result<(), String> {
    artifact.validate()?;
    validate_lower_sha256("artifact digest", &artifact.digest)?;
    if artifact.uri != artifact_uri(&artifact.digest)? {
        return Err("artifact URI does not match its content digest".into());
    }
    validate_supported_media_type(&artifact.media_type)
}

fn validate_directory_artifact(uri: &str, digest: &str, media_type: &str) -> Result<(), String> {
    let artifact = ArtifactRef {
        uri: uri.into(),
        digest: digest.into(),
        media_type: media_type.into(),
    };
    validate_cloud_artifact(&artifact)?;
    if media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE {
        return Err("artifact mount requires the supported directory archive media type".into());
    }
    Ok(())
}

fn validate_supported_media_type(value: &str) -> Result<(), String> {
    validate_single_line("artifact media type", value, 255)?;
    if value != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE {
        return Err("node artifact transport does not support this media type".into());
    }
    Ok(())
}

fn validate_lower_sha256(label: &str, value: &str) -> Result<(), String> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(format!("{label} must be a lowercase SHA-256 digest"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn artifact(fill: char) -> ArtifactRef {
        let digest = digest(fill);
        ArtifactRef {
            uri: artifact_uri(&digest).expect("artifact URI"),
            digest,
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
        }
    }

    #[test]
    fn download_identity_is_canonical_and_closed() {
        let artifact = artifact('a');
        let mut request = NodeArtifactDownloadRequest::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            digest('b'),
            "source",
            &artifact,
        )
        .expect("download request");
        request.validate().expect("valid request");

        request.artifact_uri = artifact_uri(&digest('c')).expect("different URI");
        assert_eq!(
            request.validate().expect_err("URI mismatch"),
            "artifact URI does not match its content digest"
        );
    }

    #[test]
    fn upload_receipt_cannot_change_the_output_identity() {
        let request = NodeArtifactUploadRequest::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            digest('b'),
            "oci-layout",
            digest('a'),
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            4096,
        )
        .expect("upload request");
        let mut receipt = NodeArtifactUploadReceipt {
            schema: NodeArtifactUploadReceipt::SCHEMA.into(),
            node_id: request.node_id,
            command_id: request.command_id,
            spec_digest: request.spec_digest.clone(),
            artifact: RuntimeOutputArtifact {
                name: request.output_name.clone(),
                artifact: artifact('a'),
                size_bytes: request.size_bytes,
            },
            replayed: false,
        };
        receipt
            .validate_against(&request)
            .expect("matching receipt");

        receipt.artifact.size_bytes += 1;
        assert_eq!(
            receipt
                .validate_against(&request)
                .expect_err("size mismatch"),
            "artifact upload receipt changed the transfer identity"
        );
    }

    #[test]
    fn transport_rejects_noncanonical_digests_and_untyped_archives() {
        assert!(artifact_uri(&format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(NodeArtifactUploadRequest::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            digest('b'),
            "output",
            digest('a'),
            "application/octet-stream",
            1,
        )
        .is_err());
    }
}
