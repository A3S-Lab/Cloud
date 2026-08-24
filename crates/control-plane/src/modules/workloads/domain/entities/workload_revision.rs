use super::{SecretBinding, Workload};
use crate::modules::assets::domain::{
    Asset, AssetKind, AssetRelease, AssetReleaseArtifactKind, AssetReleaseState, AssetState,
    McpServiceProfile, McpServiceProfileBinding, SKILL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, BuildRunId, EnvironmentId, OrganizationId,
    ProjectId, SecretId, Sha256Digest, SourceRevisionId, WorkloadId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use a3s_cloud_contracts::artifact_uri;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ServicePort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HttpHealthCheck>,
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
    if ports.len() > 64
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
    if let Some(health) = health {
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalBuildReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub build_run_id: BuildRunId,
}

impl ExternalBuildReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
        {
            return Err("external build reference identity is invalid".into());
        }
        Ok(())
    }
}

/// Immutable Agent release identity attached to an ordinary Workload revision.
///
/// Runtime consumes the resolved OCI artifact while Cloud retains the exact
/// AssetRelease and successful BuildRun identities for lifecycle and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorkloadRevisionBinding {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    build_run_id: BuildRunId,
}

/// Workloads-owned admission facts for binding one published Agent release.
///
/// The Assets application boundary translates its deployable release read
/// model into this value. Workloads therefore depends on neither an Asset
/// aggregate nor an Artifacts BuildRun aggregate when enforcing its own
/// revision invariants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReleaseAdmission {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    build_run_id: BuildRunId,
    published_at: DateTime<Utc>,
    artifact: OciArtifact,
}

impl AgentReleaseAdmission {
    pub fn new(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        build_run_id: BuildRunId,
        published_at: DateTime<Utc>,
        artifact: OciArtifact,
    ) -> Result<Self, String> {
        let admission = Self {
            organization_id,
            asset_id,
            asset_release_id,
            build_run_id,
            published_at: canonical_timestamp(published_at),
            artifact,
        };
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.published_at != canonical_timestamp(self.published_at)
        {
            return Err("Agent release admission identity is invalid".into());
        }
        self.artifact.validate()
    }

    pub const fn artifact(&self) -> &OciArtifact {
        &self.artifact
    }
}

impl AgentWorkloadRevisionBinding {
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub const fn build_run_id(&self) -> BuildRunId {
        self.build_run_id
    }

    pub(crate) fn restore(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        build_run_id: BuildRunId,
    ) -> Result<Self, String> {
        let binding = Self {
            organization_id,
            asset_id,
            asset_release_id,
            build_run_id,
        };
        binding.validate_identity()?;
        Ok(binding)
    }

    fn validate_identity(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
        {
            return Err("Agent Workload release binding identity is invalid".into());
        }
        Ok(())
    }
}

/// Immutable product identity attached to an ordinary Workload revision.
///
/// Runtime consumes only `profile_digest`; Cloud retains the exact release
/// identity so equal behavior profiles never collapse distinct releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpWorkloadRevisionBinding {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    profile_digest: Sha256Digest,
}

/// One exact published Skill bundle mounted into an Agent Service revision.
///
/// Runtime receives only the content-addressed, read-only Artifact mount. Cloud
/// retains the tenant and release identities for lifecycle, rollback, and
/// audit without scheduling the Skill as another Runtime unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillWorkloadRevisionBinding {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    artifact_digest: Sha256Digest,
    artifact_size_bytes: u64,
}

impl SkillWorkloadRevisionBinding {
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub const fn artifact_digest(&self) -> &Sha256Digest {
        &self.artifact_digest
    }

    pub const fn artifact_size_bytes(&self) -> u64 {
        self.artifact_size_bytes
    }

    pub fn artifact_uri(&self) -> Result<String, String> {
        artifact_uri(self.artifact_digest.as_str())
    }

    pub const fn artifact_media_type(&self) -> &'static str {
        SKILL_BUNDLE_MEDIA_TYPE
    }

    pub fn mount_name(&self) -> String {
        format!("skill-{}", self.asset_id)
    }

    pub fn mount_target(&self) -> String {
        format!("/a3s/skills/{}", self.asset_id)
    }

    pub(crate) fn restore(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        artifact_digest: Sha256Digest,
        artifact_size_bytes: u64,
    ) -> Result<Self, String> {
        let binding = Self {
            organization_id,
            asset_id,
            asset_release_id,
            artifact_digest,
            artifact_size_bytes,
        };
        binding.validate_identity()?;
        Ok(binding)
    }

    fn from_release(
        workload: &Workload,
        asset: &Asset,
        release: &AssetRelease,
    ) -> Result<Self, String> {
        let artifact = release
            .artifact
            .as_ref()
            .ok_or_else(|| "published Skill release omitted its bundle artifact".to_owned())?;
        if artifact.kind() != AssetReleaseArtifactKind::SkillBundle
            || artifact.media_type() != SKILL_BUNDLE_MEDIA_TYPE
        {
            return Err("Skill Workload input requires a Skill bundle release".into());
        }
        Self::restore(
            workload.organization_id,
            asset.id,
            release.id,
            artifact.digest().clone(),
            artifact.size_bytes(),
        )
    }

    fn validate_identity(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.artifact_digest.as_str())? != self.artifact_digest
            || self.artifact_size_bytes == 0
        {
            return Err("Skill Workload release binding identity is invalid".into());
        }
        artifact_uri(self.artifact_digest.as_str())?;
        Ok(())
    }
}

impl McpWorkloadRevisionBinding {
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub const fn profile_digest(&self) -> &Sha256Digest {
        &self.profile_digest
    }

    pub(crate) fn restore(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        profile_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let binding = Self {
            organization_id,
            asset_id,
            asset_release_id,
            profile_digest,
        };
        binding.validate_identity()?;
        Ok(binding)
    }

    fn validate_identity(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.profile_digest.as_str())? != self.profile_digest
        {
            return Err("MCP Workload release binding identity is invalid".into());
        }
        Ok(())
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_build: Option<ExternalBuildReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_binding: Option<AgentWorkloadRevisionBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mcp_binding: Option<McpWorkloadRevisionBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skill_bindings: Vec<SkillWorkloadRevisionBinding>,
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
            external_build: None,
            agent_binding: None,
            mcp_binding: None,
            skill_bindings: Vec::new(),
        })
    }

    pub fn create_from_external_build(
        id: WorkloadRevisionId,
        workload_id: WorkloadId,
        generation: u64,
        template: ServiceTemplate,
        external_build: ExternalBuildReference,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        external_build.validate()?;
        let mut revision = Self::create(id, workload_id, generation, template, created_at)?;
        revision.external_build = Some(external_build);
        Ok(revision)
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
            external_build: None,
            agent_binding: None,
            mcp_binding: None,
            skill_bindings: Vec::new(),
        })
    }

    pub(crate) fn restore_external_build(
        &mut self,
        external_build: ExternalBuildReference,
    ) -> Result<(), String> {
        external_build.validate()?;
        self.resolved_template()?;
        match &self.external_build {
            Some(existing) if existing == &external_build => Ok(()),
            Some(_) => Err("external build reference is immutable".into()),
            None => {
                self.external_build = Some(external_build);
                Ok(())
            }
        }
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

    /// Attach one published Agent release without introducing another Runtime
    /// or deployment specification.
    pub fn bind_agent_release(
        &mut self,
        workload: &Workload,
        admission: &AgentReleaseAdmission,
    ) -> Result<bool, String> {
        admission.validate()?;
        if self.workload_id != workload.id
            || workload.organization_id != admission.organization_id
            || self.created_at < admission.published_at
            || self.external_build.is_some()
            || self.mcp_binding.is_some()
        {
            return Err(
                "Agent Workload revision does not match its tenant or published release".into(),
            );
        }
        let template = self.resolved_template()?;
        if template.artifact != admission.artifact {
            return Err("Agent Workload artifact does not match its exact AssetRelease".into());
        }
        let binding = AgentWorkloadRevisionBinding::restore(
            workload.organization_id,
            admission.asset_id,
            admission.asset_release_id,
            admission.build_run_id,
        )?;
        match &self.agent_binding {
            Some(existing) if existing == &binding => Ok(false),
            Some(_) => Err("Agent Workload release binding is immutable".into()),
            None => {
                self.agent_binding = Some(binding);
                Ok(true)
            }
        }
    }

    pub const fn agent_binding(&self) -> Option<&AgentWorkloadRevisionBinding> {
        self.agent_binding.as_ref()
    }

    pub(crate) fn validate_agent_binding_for_workload(
        &self,
        workload: &Workload,
    ) -> Result<(), String> {
        let Some(binding) = &self.agent_binding else {
            return Ok(());
        };
        binding.validate_identity()?;
        if self.workload_id != workload.id
            || binding.organization_id != workload.organization_id
            || self.template.is_none()
            || self.resolved_at.is_none()
            || self.external_build.is_some()
            || self.mcp_binding.is_some()
        {
            return Err(
                "Agent Workload release binding does not match its Workload revision".into(),
            );
        }
        Ok(())
    }

    pub(crate) fn restore_agent_binding(
        &mut self,
        binding: AgentWorkloadRevisionBinding,
    ) -> Result<(), String> {
        binding.validate_identity()?;
        self.resolved_template()?.validate()?;
        if self.agent_binding.is_some()
            || self.external_build.is_some()
            || self.mcp_binding.is_some()
        {
            return Err("Agent Workload release binding was restored more than once".into());
        }
        self.agent_binding = Some(binding);
        Ok(())
    }

    /// Attach one published MCP release and its immutable semantics profile.
    ///
    /// The binding is idempotent for the same identity and otherwise
    /// immutable. It validates the ordinary Service template instead of
    /// introducing an MCP-specific Runtime specification.
    pub fn bind_mcp_release(
        &mut self,
        workload: &Workload,
        asset: &Asset,
        release: &AssetRelease,
        profile: &McpServiceProfileBinding,
    ) -> Result<bool, String> {
        asset.validate()?;
        release.validate_for(asset)?;
        profile.validate()?;
        if self.workload_id != workload.id
            || workload.organization_id != asset.organization_id
            || asset.kind != AssetKind::Mcp
            || release.organization_id != workload.organization_id
            || release.asset_id != asset.id
            || release.state != AssetReleaseState::Published
            || profile.organization_id != workload.organization_id
            || profile.asset_id != asset.id
            || profile.asset_release_id != release.id
            || self.created_at < release.updated_at
            || self.created_at < profile.created_at
            || self.agent_binding.is_some()
            || self.external_build.is_some()
        {
            return Err(
                "MCP Workload revision does not match its tenant, published release, or profile"
                    .into(),
            );
        }
        let release_artifact = release
            .artifact
            .as_ref()
            .ok_or_else(|| "published MCP release omitted its OCI artifact".to_owned())?;
        if release_artifact.kind() != AssetReleaseArtifactKind::OciService {
            return Err("MCP Workload requires an OCI Service release".into());
        }
        let template = self.resolved_template()?;
        if template.artifact.digest != release_artifact.digest().as_str()
            || template.artifact.media_type != release_artifact.media_type()
        {
            return Err("MCP Workload artifact does not match its exact AssetRelease".into());
        }
        validate_mcp_template(template, &profile.profile)?;
        let binding = McpWorkloadRevisionBinding::restore(
            workload.organization_id,
            asset.id,
            release.id,
            profile.profile.digest().clone(),
        )?;
        match &self.mcp_binding {
            Some(existing) if existing == &binding => Ok(false),
            Some(_) => Err("MCP Workload release binding is immutable".into()),
            None => {
                self.mcp_binding = Some(binding);
                Ok(true)
            }
        }
    }

    pub const fn mcp_binding(&self) -> Option<&McpWorkloadRevisionBinding> {
        self.mcp_binding.as_ref()
    }

    pub(crate) fn validate_mcp_binding_for_workload(
        &self,
        workload: &Workload,
    ) -> Result<(), String> {
        let Some(binding) = &self.mcp_binding else {
            return Ok(());
        };
        binding.validate_identity()?;
        if self.workload_id != workload.id
            || binding.organization_id != workload.organization_id
            || self.template.is_none()
            || self.resolved_at.is_none()
        {
            return Err("MCP Workload release binding does not match its Workload revision".into());
        }
        Ok(())
    }

    pub(crate) fn restore_mcp_binding(
        &mut self,
        binding: McpWorkloadRevisionBinding,
        profile: &McpServiceProfile,
    ) -> Result<(), String> {
        binding.validate_identity()?;
        if binding.profile_digest != *profile.digest() {
            return Err("MCP Workload release binding and Service profile digest differ".into());
        }
        validate_mcp_template(self.resolved_template()?, profile)?;
        if self.mcp_binding.is_some()
            || self.agent_binding.is_some()
            || self.external_build.is_some()
        {
            return Err("MCP Workload release binding was restored more than once".into());
        }
        self.mcp_binding = Some(binding);
        Ok(())
    }

    pub fn skill_bindings(&self) -> &[SkillWorkloadRevisionBinding] {
        &self.skill_bindings
    }

    pub fn skill_binding(&self, asset_id: AssetId) -> Option<&SkillWorkloadRevisionBinding> {
        self.skill_bindings
            .binary_search_by_key(&asset_id, SkillWorkloadRevisionBinding::asset_id)
            .ok()
            .map(|index| &self.skill_bindings[index])
    }

    /// Derive a new immutable Agent revision with one exact Skill release.
    /// Existing bindings from other Skill Assets are retained. Rebinding the
    /// same Asset replaces only that Asset's release in the new revision.
    pub fn with_skill_release_as(
        &self,
        id: WorkloadRevisionId,
        generation: u64,
        created_at: DateTime<Utc>,
        workload: &Workload,
        asset: &Asset,
        release: &AssetRelease,
    ) -> Result<Self, String> {
        self.validate_skill_bindings_for_workload(workload)?;
        asset.validate()?;
        release.validate_for(asset)?;
        if self.workload_id != workload.id
            || workload.organization_id != asset.organization_id
            || asset.kind != AssetKind::Skill
            || asset.state != AssetState::Active
            || release.organization_id != workload.organization_id
            || release.asset_id != asset.id
            || release.state != AssetReleaseState::Published
            || created_at < release.updated_at
        {
            return Err(
                "Skill Workload input does not match its tenant or published release".into(),
            );
        }
        let binding = SkillWorkloadRevisionBinding::from_release(workload, asset, release)?;
        if self.skill_binding(asset.id) == Some(&binding) {
            return Err("Skill release is already bound to the active Workload revision".into());
        }
        let mut revision = self.copy_as(id, generation, created_at)?;
        match revision
            .skill_bindings
            .binary_search_by_key(&asset.id, SkillWorkloadRevisionBinding::asset_id)
        {
            Ok(index) => revision.skill_bindings[index] = binding,
            Err(index) => revision.skill_bindings.insert(index, binding),
        }
        revision.validate_skill_bindings_for_workload(workload)?;
        Ok(revision)
    }

    /// Derive a new immutable Agent revision without one Skill Asset binding.
    pub fn without_skill_release_as(
        &self,
        id: WorkloadRevisionId,
        generation: u64,
        created_at: DateTime<Utc>,
        asset_id: AssetId,
    ) -> Result<Self, String> {
        let index = self
            .skill_bindings
            .binary_search_by_key(&asset_id, SkillWorkloadRevisionBinding::asset_id)
            .map_err(|_| "Skill Asset is not bound to the active Workload revision".to_owned())?;
        let mut revision = self.copy_as(id, generation, created_at)?;
        revision.skill_bindings.remove(index);
        Ok(revision)
    }

    pub(crate) fn validate_skill_bindings_for_workload(
        &self,
        workload: &Workload,
    ) -> Result<(), String> {
        if self.skill_bindings.is_empty() {
            return Ok(());
        }
        if self.workload_id != workload.id
            || self.agent_binding.is_none()
            || self.external_build.is_some()
            || self.mcp_binding.is_some()
            || self.skill_bindings.len() > 128
        {
            return Err("Skill bindings require one resolved Agent Workload revision".into());
        }
        let mut previous = None;
        for binding in &self.skill_bindings {
            binding.validate_identity()?;
            if binding.organization_id != workload.organization_id
                || previous.is_some_and(|asset_id| asset_id >= binding.asset_id)
            {
                return Err("Skill Workload release bindings are not unique and ordered".into());
            }
            previous = Some(binding.asset_id);
        }
        self.resolved_template()?.validate()
    }

    pub(crate) fn restore_skill_binding(
        &mut self,
        binding: SkillWorkloadRevisionBinding,
    ) -> Result<(), String> {
        binding.validate_identity()?;
        if self
            .agent_binding
            .as_ref()
            .is_none_or(|agent| agent.organization_id() != binding.organization_id())
            || self.external_build.is_some()
            || self.mcp_binding.is_some()
            || self.skill_bindings.len() >= 128
        {
            return Err("Skill bindings require one resolved Agent Workload revision".into());
        }
        match self
            .skill_bindings
            .binary_search_by_key(&binding.asset_id, SkillWorkloadRevisionBinding::asset_id)
        {
            Ok(_) => Err("Skill Workload release binding was restored more than once".into()),
            Err(index) => {
                self.skill_bindings.insert(index, binding);
                Ok(())
            }
        }
    }

    fn copy_as(
        &self,
        id: WorkloadRevisionId,
        generation: u64,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if id == self.id
            || generation <= self.generation
            || created_at < self.created_at
            || self.agent_binding.is_none()
            || self.external_build.is_some()
            || self.mcp_binding.is_some()
        {
            return Err("Skill binding revision identity or ordering is invalid".into());
        }
        let mut revision = Self::create(
            id,
            self.workload_id,
            generation,
            self.resolved_template()?.clone(),
            created_at,
        )?;
        revision.agent_binding = self.agent_binding.clone();
        revision.skill_bindings = self.skill_bindings.clone();
        Ok(revision)
    }

    pub fn rollback_as(
        &self,
        id: WorkloadRevisionId,
        generation: u64,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if id == self.id || generation <= self.generation || created_at < self.created_at {
            return Err("rollback revision identity or ordering is invalid".into());
        }
        let mut revision = Self::create(
            id,
            self.workload_id,
            generation,
            self.resolved_template()?.clone(),
            created_at,
        )?;
        revision.external_build = self.external_build.clone();
        revision.agent_binding = self.agent_binding.clone();
        revision.mcp_binding = self.mcp_binding.clone();
        revision.skill_bindings = self.skill_bindings.clone();
        Ok(revision)
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
        let mut revision = Self::create(id, self.workload_id, generation, template, created_at)?;
        revision.external_build = self.external_build.clone();
        revision.agent_binding = self.agent_binding.clone();
        revision.mcp_binding = self.mcp_binding.clone();
        revision.skill_bindings = self.skill_bindings.clone();
        Ok(revision)
    }

    pub fn runtime_unit_id(&self) -> String {
        format!("workload:{}:revision:{}", self.workload_id, self.id)
    }
}

fn validate_mcp_template(
    template: &ServiceTemplate,
    profile: &McpServiceProfile,
) -> Result<(), String> {
    template.validate()?;
    McpServiceProfile::restore(profile.canonical_acl(), profile.digest().as_str())?;
    let profile = profile.spec();
    if !template
        .ports
        .iter()
        .any(|port| port.name == profile.runtime_port)
    {
        return Err("MCP Workload does not declare the profile Runtime port".into());
    }
    let health = template
        .health
        .as_ref()
        .ok_or_else(|| "MCP Workload requires the profile HTTP health check".to_owned())?;
    if health.port_name != profile.runtime_port || health.path != profile.health_path {
        return Err(
            "MCP Workload health check does not match its immutable Service profile".into(),
        );
    }
    Ok(())
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
