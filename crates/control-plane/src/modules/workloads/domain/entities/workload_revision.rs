use super::SecretBinding;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, SecretId, WorkloadId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OciArtifact {
    pub uri: String,
    pub digest: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OciArtifactReference {
    pub uri: String,
    pub expected_digest: Option<String>,
}

impl OciArtifactReference {
    pub fn validate(&self) -> Result<(), String> {
        let parsed = parse_oci_reference(&self.uri)?;
        if let Some(expected_digest) = &self.expected_digest {
            validate_sha256(expected_digest)?;
            if parsed
                .digest
                .is_some_and(|digest| digest != expected_digest)
            {
                return Err("OCI reference and expected digest do not match".into());
            }
        }
        Ok(())
    }

    pub fn repository(&self) -> Result<&str, String> {
        self.validate()?;
        Ok(parse_oci_reference(&self.uri)?.repository)
    }

    pub fn bound_digest(&self) -> Result<Option<&str>, String> {
        self.validate()?;
        Ok(parse_oci_reference(&self.uri)?.digest)
    }

    pub fn registry_and_repository(&self) -> Result<(&str, &str), String> {
        let repository = self.repository()?;
        repository
            .split_once('/')
            .ok_or_else(|| "OCI repository must include an explicit registry".into())
    }

    pub fn manifest_reference(&self) -> Result<&str, String> {
        self.validate()?;
        Ok(parse_oci_reference(&self.uri)?.reference)
    }
}

impl From<&OciArtifact> for OciArtifactReference {
    fn from(artifact: &OciArtifact) -> Self {
        Self {
            uri: artifact.uri.clone(),
            expected_digest: Some(artifact.digest.clone()),
        }
    }
}

impl OciArtifact {
    pub fn validate(&self) -> Result<(), String> {
        let parsed = parse_oci_reference(&self.uri)?;
        let Some(bound_digest) = parsed.digest else {
            return Err("OCI artifact URI must use oci:// and bind a digest".into());
        };
        if bound_digest != self.digest {
            return Err("OCI artifact URI and digest do not match".into());
        }
        validate_sha256(&self.digest)?;
        if self.media_type.trim().is_empty()
            || self.media_type.len() > 255
            || self.media_type.contains(['\0', '\r', '\n'])
        {
            return Err("OCI artifact media type is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceProcess {
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceResources {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePort {
    pub name: String,
    pub container_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpHealthCheck {
    pub port_name: String,
    pub path: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
    pub stabilization_window_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceTemplate<A = OciArtifact> {
    pub artifact: A,
    pub process: ServiceProcess,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<SecretBinding>,
    pub resources: ServiceResources,
    pub ports: Vec<ServicePort>,
    pub health: HttpHealthCheck,
}

pub type RequestedServiceTemplate = ServiceTemplate<OciArtifactReference>;

impl ServiceTemplate {
    pub fn validate(&self) -> Result<(), String> {
        self.artifact.validate()?;
        validate_template_body(self)
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        digest_json(self, "service template")
    }
}

impl RequestedServiceTemplate {
    pub fn validate_request(&self) -> Result<(), String> {
        self.artifact.validate()?;
        validate_template_body(self)
    }

    pub fn request_digest(&self) -> Result<String, String> {
        self.validate_request()?;
        canonical_digest_json(self, "requested service template")
    }

    pub fn resolve(self, artifact: OciArtifact) -> Result<ServiceTemplate, String> {
        self.validate_request()?;
        artifact.validate()?;
        if self.artifact.repository()? != oci_artifact_repository(&artifact)? {
            return Err("resolved OCI artifact changed the requested repository".into());
        }
        if self
            .artifact
            .expected_digest
            .as_ref()
            .is_some_and(|expected| expected != &artifact.digest)
        {
            return Err("resolved OCI artifact changed the expected digest".into());
        }
        let resolved = ServiceTemplate {
            artifact,
            process: self.process,
            secrets: self.secrets,
            resources: self.resources,
            ports: self.ports,
            health: self.health,
        };
        resolved.validate()?;
        Ok(resolved)
    }
}

fn validate_template_body<A>(template: &ServiceTemplate<A>) -> Result<(), String> {
    let ServiceTemplate {
        process,
        secrets,
        resources,
        ports,
        health,
        ..
    } = template;
    validate_string_list("process command", &process.command, 64, 4096)?;
    validate_string_list("process argument", &process.args, 256, 4096)?;
    if process
        .working_directory
        .as_ref()
        .is_some_and(|value| !valid_single_line(value, 4096))
        || process.environment.len() > 256
        || process.environment.iter().any(|(key, value)| {
            !valid_environment_key(key) || value.len() > 64 * 1024 || value.contains('\0')
        })
    {
        return Err("service process configuration is invalid".into());
    }
    if secrets.len() > 128 {
        return Err("service Secret binding count exceeds 128".into());
    }
    let mut secret_names = std::collections::BTreeSet::new();
    let mut secret_targets = std::collections::BTreeSet::new();
    for secret in secrets {
        secret.validate()?;
        if !secret_names.insert(&secret.name) || !secret_targets.insert(secret.target_key()) {
            return Err("service Secret binding names and targets must be unique".into());
        }
        if matches!(
            &secret.target,
            super::SecretBindingTarget::Environment { variable }
                if process.environment.contains_key(variable)
        ) {
            return Err("service environment and Secret targets must not overlap".into());
        }
    }
    if resources.cpu_millis == 0
        || resources.memory_bytes == 0
        || resources.pids == 0
        || resources.ephemeral_storage_bytes == Some(0)
    {
        return Err("service resource limits are invalid".into());
    }
    if ports.is_empty()
        || ports.len() > 64
        || ports
            .iter()
            .any(|port| !valid_identifier(&port.name, 63) || port.container_port == 0)
    {
        return Err("service ports are invalid".into());
    }
    let mut names = ports.iter().map(|port| &port.name).collect::<Vec<_>>();
    names.sort_unstable();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("service port names must be unique".into());
    }
    if !ports.iter().any(|port| port.name == health.port_name)
        || !health.path.starts_with('/')
        || health.path.len() > 2048
        || health.path.contains(['\0', '\r', '\n'])
        || health.interval_ms == 0
        || health.timeout_ms == 0
        || health.timeout_ms > health.interval_ms
        || health.healthy_threshold == 0
        || health.unhealthy_threshold == 0
        || health.stabilization_window_ms == 0
    {
        return Err("service HTTP health check is invalid".into());
    }
    Ok(())
}

fn digest_json<T>(value: &T, label: &str) -> Result<String, String>
where
    T: Serialize + ?Sized,
{
    let bytes =
        serde_json::to_vec(value).map_err(|error| format!("could not encode {label}: {error}"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn canonical_digest_json<T>(value: &T, label: &str) -> Result<String, String>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(value)
        .map_err(|error| format!("could not encode {label}: {error}"))?;
    digest_json(&value, label)
}

#[derive(Debug, Clone, Copy)]
struct ParsedOciReference<'a> {
    repository: &'a str,
    digest: Option<&'a str>,
    reference: &'a str,
}

fn parse_oci_reference(uri: &str) -> Result<ParsedOciReference<'_>, String> {
    if uri.len() > 4096
        || uri.contains(['\0', '\r', '\n', '\t', ' ', '?', '#', '\\'])
        || !uri.starts_with("oci://")
    {
        return Err("OCI reference is invalid".into());
    }
    let value = uri
        .strip_prefix("oci://")
        .ok_or_else(|| "OCI reference must use oci://".to_owned())?;
    if let Some((repository, digest)) = value.rsplit_once('@') {
        validate_oci_repository(repository)?;
        validate_sha256(digest)?;
        return Ok(ParsedOciReference {
            repository,
            digest: Some(digest),
            reference: digest,
        });
    }

    let last_slash = value.rfind('/').ok_or_else(|| {
        "OCI tag reference must include an explicit registry and repository".to_owned()
    })?;
    let tag_separator = value
        .rfind(':')
        .filter(|index| *index > last_slash)
        .ok_or_else(|| "OCI tag reference must include an explicit tag".to_owned())?;
    let repository = &value[..tag_separator];
    let tag = &value[tag_separator + 1..];
    validate_oci_repository(repository)?;
    if tag.is_empty()
        || tag.len() > 128
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err("OCI tag is invalid".into());
    }
    Ok(ParsedOciReference {
        repository,
        digest: None,
        reference: tag,
    })
}

fn validate_oci_repository(repository: &str) -> Result<(), String> {
    let Some((registry, path)) = repository.split_once('/') else {
        return Err("OCI repository must include an explicit registry".into());
    };
    if registry.is_empty()
        || path.is_empty()
        || registry.starts_with('.')
        || registry.ends_with('.')
        || path.starts_with('/')
        || path.ends_with('/')
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
        return Err("OCI repository is invalid".into());
    }
    Ok(())
}

fn oci_artifact_repository(artifact: &OciArtifact) -> Result<&str, String> {
    artifact.validate()?;
    Ok(parse_oci_reference(&artifact.uri)?.repository)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadRevision {
    pub id: WorkloadRevisionId,
    pub workload_id: WorkloadId,
    pub generation: u64,
    pub request: RequestedServiceTemplate,
    pub request_digest: String,
    pub template: Option<ServiceTemplate>,
    pub template_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl WorkloadRevision {
    pub fn create(
        id: WorkloadRevisionId,
        workload_id: WorkloadId,
        generation: u64,
        template: ServiceTemplate,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if generation == 0 {
            return Err("workload revision generation must be positive".into());
        }
        let template_digest = template.digest()?;
        let request = RequestedServiceTemplate {
            artifact: OciArtifactReference::from(&template.artifact),
            process: template.process.clone(),
            secrets: template.secrets.clone(),
            resources: template.resources.clone(),
            ports: template.ports.clone(),
            health: template.health.clone(),
        };
        let request_digest = request.request_digest()?;
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            workload_id,
            generation,
            request,
            request_digest,
            template: Some(template),
            template_digest: Some(template_digest),
            created_at,
            resolved_at: Some(created_at),
        })
    }

    pub fn request(
        id: WorkloadRevisionId,
        workload_id: WorkloadId,
        generation: u64,
        request: RequestedServiceTemplate,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if generation == 0 {
            return Err("workload revision generation must be positive".into());
        }
        let request_digest = request.request_digest()?;
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            workload_id,
            generation,
            request,
            request_digest,
            template: None,
            template_digest: None,
            created_at,
            resolved_at: None,
        })
    }

    pub fn resolve(
        &mut self,
        artifact: OciArtifact,
        resolved_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let resolved_at = canonical_timestamp(resolved_at);
        if resolved_at < self.created_at {
            return Err("workload revision resolution time regressed".into());
        }
        let template = self.request.clone().resolve(artifact)?;
        let template_digest = template.digest()?;
        if let Some(existing) = &self.template {
            if existing == &template && self.template_digest.as_ref() == Some(&template_digest) {
                return Ok(());
            }
            return Err("resolved workload revision is immutable".into());
        }
        self.template = Some(template);
        self.template_digest = Some(template_digest);
        self.resolved_at = Some(resolved_at);
        Ok(())
    }

    pub fn resolved_template(&self) -> Result<&ServiceTemplate, String> {
        self.template
            .as_ref()
            .ok_or_else(|| "workload revision has not resolved its OCI artifact".into())
    }

    pub fn restart_for_secret_rotation(
        &self,
        id: WorkloadRevisionId,
        generation: u64,
        secret_id: SecretId,
        version: u64,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if id == self.id
            || generation <= self.generation
            || version == 0
            || created_at < self.created_at
        {
            return Err("Secret rotation revision identity or ordering is invalid".into());
        }
        let mut template = self.resolved_template()?.clone();
        let mut advanced = false;
        for binding in template
            .secrets
            .iter_mut()
            .filter(|binding| binding.secret_id == secret_id)
        {
            if binding.version > version {
                return Err("Secret rotation cannot regress a workload binding".into());
            }
            if binding.version < version {
                binding.version = version;
                advanced = true;
            }
        }
        if !advanced {
            return Err("workload revision has no older binding for this Secret".into());
        }
        Self::create(id, self.workload_id, generation, template, created_at)
    }

    pub fn runtime_unit_id(&self) -> String {
        format!("workload:{}:revision:{}", self.workload_id, self.id)
    }
}

fn validate_sha256(value: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err("digest must use sha256".into());
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("digest must contain 64 lowercase hexadecimal characters".into());
    }
    Ok(())
}

fn validate_string_list(
    label: &str,
    values: &[String],
    maximum_items: usize,
    maximum_length: usize,
) -> Result<(), String> {
    if values.len() > maximum_items
        || values
            .iter()
            .any(|value| value.len() > maximum_length || value.contains('\0'))
    {
        return Err(format!("{label} list is invalid"));
    }
    Ok(())
}

fn valid_single_line(value: &str, maximum_length: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum_length && !value.contains(['\0', '\r', '\n'])
}

fn valid_identifier(value: &str, maximum_length: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_length
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn valid_environment_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
}
