use super::build_artifact::{validate_sha256, ValidatedOciBuildOutput};
use crate::modules::artifacts::domain::OciDescriptor;
use serde::{Deserialize, Serialize};

const MAX_REPOSITORY_LENGTH: usize = 1024;
const PUBLICATION_SCOPE_SUFFIX_LENGTH: usize = 4 * (1 + 36);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OciPublicationTarget {
    pub registry: String,
    pub repository: String,
    pub descriptor: OciDescriptor,
}

impl OciPublicationTarget {
    pub fn new(
        registry: impl Into<String>,
        repository: impl Into<String>,
        descriptor: OciDescriptor,
    ) -> Result<Self, String> {
        let target = Self {
            registry: registry.into(),
            repository: repository.into(),
            descriptor,
        };
        target.validate()?;
        Ok(target)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_registry(&self.registry)?;
        validate_repository(&self.repository)?;
        let descriptor = OciDescriptor::new(
            self.descriptor.media_type(),
            self.descriptor.digest(),
            self.descriptor.size(),
        )?;
        if descriptor != self.descriptor {
            return Err("OCI publication target descriptor is not canonical".into());
        }
        Ok(())
    }

    pub fn uri(&self) -> String {
        format!(
            "oci://{}/{}@{}",
            self.registry,
            self.repository,
            self.descriptor.digest()
        )
    }

    pub fn matches_output(&self, output: &ValidatedOciBuildOutput) -> bool {
        self.descriptor == output.descriptor
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedOciArtifact {
    pub uri: String,
    pub digest: String,
    pub media_type: String,
    pub size_bytes: u64,
}

impl PublishedOciArtifact {
    pub fn from_target(target: &OciPublicationTarget) -> Self {
        Self {
            uri: target.uri(),
            digest: target.descriptor.digest().into(),
            media_type: target.descriptor.media_type().into(),
            size_bytes: target.descriptor.size(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_sha256(&self.digest, "published OCI artifact digest")?;
        if self.uri.len() > 4096
            || self.uri.contains(['\0', '\r', '\n'])
            || self.media_type.trim().is_empty()
            || self.media_type.len() > 255
            || self.media_type.contains(['\0', '\r', '\n'])
            || self.size_bytes == 0
        {
            return Err("published OCI artifact is invalid".into());
        }
        Ok(())
    }

    pub fn matches_target(&self, target: &OciPublicationTarget) -> bool {
        self.uri == target.uri()
            && self.digest == target.descriptor.digest()
            && self.media_type == target.descriptor.media_type()
            && self.size_bytes == target.descriptor.size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciPublicationRequest {
    pub target: OciPublicationTarget,
    pub output: ValidatedOciBuildOutput,
}

impl OciPublicationRequest {
    pub fn new(
        target: OciPublicationTarget,
        output: ValidatedOciBuildOutput,
    ) -> Result<Self, String> {
        target.validate()?;
        output.validate()?;
        if !target.matches_output(&output) {
            return Err("OCI publication target changed the validated output descriptor".into());
        }
        Ok(Self { target, output })
    }
}

pub(crate) fn validate_registry(registry: &str) -> Result<(), String> {
    if registry.is_empty()
        || registry.len() > 255
        || registry.ends_with(':')
        || registry.contains(['/', '@', '\\', '\0', '\r', '\n', ' ', '\t'])
        || !url::Url::parse(&format!("https://{registry}/")).is_ok_and(|origin| {
            origin.host_str().is_some()
                && origin.path() == "/"
                && origin.query().is_none()
                && origin.fragment().is_none()
                && origin.username().is_empty()
                && origin.password().is_none()
        })
    {
        return Err("OCI publication registry must be an explicit host[:port]".into());
    }
    Ok(())
}

pub(crate) fn validate_repository(repository: &str) -> Result<(), String> {
    if repository.is_empty() || repository.len() > MAX_REPOSITORY_LENGTH {
        return Err("OCI publication repository is invalid".into());
    }
    for segment in repository.split('/') {
        let valid_edge = segment
            .as_bytes()
            .first()
            .zip(segment.as_bytes().last())
            .is_some_and(|(first, last)| {
                first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric()
            });
        if segment.is_empty()
            || segment.len() > 128
            || !valid_edge
            || segment.bytes().any(|byte| {
                !(byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-'))
            })
        {
            return Err("OCI publication repository is invalid".into());
        }
    }
    Ok(())
}

pub(crate) fn validate_repository_prefix(repository: &str) -> Result<(), String> {
    validate_repository(repository)?;
    if repository.len() > MAX_REPOSITORY_LENGTH - PUBLICATION_SCOPE_SUFFIX_LENGTH {
        return Err(
            "OCI publication repository prefix leaves no room for its scoped build identity".into(),
        );
    }
    Ok(())
}
