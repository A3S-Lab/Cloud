use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use super::validation::{valid_absolute_path, valid_environment_name, valid_sha256};

const MAX_INPUT_BYTES: usize = 16 * 1024;
pub const MAX_EXECUTION_TIMEOUT_MS: u64 = 900_000;
const OCI_IMAGE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const DOCKER_IMAGE_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
const DOCKER_IMAGE_INDEX: &str = "application/vnd.docker.distribution.manifest.list.v2+json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionArtifact {
    pub uri: String,
    pub digest: String,
    pub media_type: String,
}

impl ExecutionArtifact {
    pub fn validate(&self) -> Result<(), String> {
        let expected = format!("@{}", self.digest);
        if !valid_sha256(&self.digest)
            || self.uri.is_empty()
            || self.uri.len() > 4096
            || self
                .uri
                .contains(['\0', '\r', '\n', '\t', ' ', '?', '#', '\\'])
            || !self.uri.starts_with("oci://")
            || !self.uri.ends_with(&expected)
            || !matches!(
                self.media_type.as_str(),
                OCI_IMAGE_MANIFEST | OCI_IMAGE_INDEX | DOCKER_IMAGE_MANIFEST | DOCKER_IMAGE_INDEX
            )
        {
            return Err(
                "execution artifact must be a credential-free digest-pinned OCI image".into(),
            );
        }
        let repository = self
            .uri
            .strip_prefix("oci://")
            .and_then(|value| value.strip_suffix(&expected))
            .ok_or_else(|| "execution OCI artifact identity is invalid".to_owned())?;
        let Some((registry, path)) = repository.split_once('/') else {
            return Err("execution OCI artifact must include an explicit registry".into());
        };
        if registry.is_empty()
            || path.is_empty()
            || repository.contains("//")
            || repository.split('/').any(|segment| {
                segment.is_empty()
                    || segment == "."
                    || segment == ".."
                    || !segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
                    })
            })
        {
            return Err("execution OCI artifact repository is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProcess {
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl ExecutionProcess {
    fn validate(&self) -> Result<(), String> {
        if self.command.len() > 64
            || self.args.len() > 256
            || self
                .command
                .iter()
                .chain(&self.args)
                .any(|value| !valid_process_value(value))
            || self
                .working_directory
                .as_ref()
                .is_some_and(|path| !valid_absolute_path(path))
            || self.environment.len() > 256
            || self.environment.iter().any(|(name, value)| {
                !valid_environment_name(name)
                    || name.starts_with("A3S_EXECUTION_")
                    || value.len() > 32 * 1024
                    || value.contains('\0')
            })
        {
            return Err("execution process configuration is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionResources {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_storage_bytes: Option<u64>,
    pub timeout_ms: u64,
}

impl ExecutionResources {
    fn validate(&self) -> Result<(), String> {
        if self.cpu_millis == 0
            || self.cpu_millis > 1_000_000
            || self.memory_bytes == 0
            || self.memory_bytes > 1024 * 1024 * 1024 * 1024
            || self.pids == 0
            || self.pids > 1_000_000
            || self.ephemeral_storage_bytes == Some(0)
            || self
                .ephemeral_storage_bytes
                .is_some_and(|bytes| bytes > 1024 * 1024 * 1024 * 1024)
            || self.timeout_ms == 0
            || self.timeout_ms > MAX_EXECUTION_TIMEOUT_MS
        {
            return Err(
                "execution resource limits must be positive and within Cloud bounds".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTemplate {
    pub artifact: ExecutionArtifact,
    pub process: ExecutionProcess,
    #[serde(default)]
    pub input: serde_json::Value,
    pub resources: ExecutionResources,
}

impl ExecutionTemplate {
    pub fn validate(&self) -> Result<(), String> {
        self.artifact.validate()?;
        self.process.validate()?;
        self.resources.validate()?;
        let input = serde_json::to_vec(&self.input)
            .map_err(|error| format!("could not encode execution input: {error}"))?;
        if input.len() > MAX_INPUT_BYTES {
            return Err(format!(
                "execution input exceeds the {MAX_INPUT_BYTES}-byte limit"
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode execution template: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
}

fn valid_process_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 * 1024 && !value.contains('\0')
}
