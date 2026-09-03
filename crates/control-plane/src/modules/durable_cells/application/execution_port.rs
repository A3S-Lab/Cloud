use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, EnvironmentId, ExecutionId, NodeId,
    OrganizationId, ProjectId, Sha256Digest,
};
use a3s_runtime::contract::{
    ArtifactRef, ResourceLimits, RuntimeProcessSpec, SecretReference, SecretTarget,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_AUTHORITY_KIND_BYTES: usize = 96;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 512;
const MAX_SECRET_REFERENCE_BYTES: usize = 1024;
const MAX_STRING_BYTES: usize = 32 * 1024;

/// Cloud-owned authority for one internal, node-bound finite Task.
///
/// Executions owns the durable Task lifecycle. Durable Cells supplies only
/// this immutable authority identity and the generic Runtime inputs needed by
/// the selected provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecutionAuthority {
    pub kind: String,
    pub subject_id: Uuid,
    pub digest: Sha256Digest,
}

/// One immutable artifact mounted read-only into a finite Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecutionArtifactMount {
    pub name: String,
    pub artifact: ArtifactRef,
    pub target: String,
}

/// Cloud-owned projection of the generic inputs required by an internal Task.
/// No Execution aggregate, repository, event, or Operation state crosses this
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecutionTaskPolicy {
    pub authority: DurableCellExecutionAuthority,
    pub mounts: Vec<DurableCellExecutionArtifactMount>,
    pub secrets: Vec<SecretReference>,
    pub semantics_profile_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecutionTemplate {
    pub artifact: ArtifactRef,
    pub process: RuntimeProcessSpec,
    pub input: serde_json::Value,
    pub resources: ResourceLimits,
}

/// Exact immutable request for one Durable Cells publication Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecutionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub execution_id: ExecutionId,
    pub template: DurableCellExecutionTemplate,
    pub target_node_id: NodeId,
    pub task_policy: DurableCellExecutionTaskPolicy,
    pub authority_subject_id: Uuid,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellExecutionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.target_node_id.as_uuid().is_nil()
            || self.request_id.is_nil()
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Durable Cell Execution request identity is invalid".into());
        }
        validate_text(
            &self.idempotency_key,
            MAX_IDEMPOTENCY_KEY_BYTES,
            "Durable Cell Execution idempotency key",
        )?;
        validate_template(&self.template)?;
        validate_task_policy(&self.task_policy)?;
        if self.task_policy.authority.subject_id != self.authority_subject_id
            || self.authority_subject_id.is_nil()
        {
            return Err(
                "Durable Cell Execution Task authority subject does not match its request".into(),
            );
        }
        Ok(())
    }
}

/// Minimal cancellation command; the adapter reloads the owner aggregate and
/// applies its existing cancellation transition atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecutionCancellationRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub execution_id: ExecutionId,
    pub authority_kind: String,
    pub authority_subject_id: Uuid,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellExecutionCancellationRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.authority_subject_id.is_nil()
            || self.request_id.is_nil()
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Durable Cell Execution cancellation identity is invalid".into());
        }
        validate_authority_kind(&self.authority_kind)?;
        validate_text(
            &self.idempotency_key,
            MAX_IDEMPOTENCY_KEY_BYTES,
            "Durable Cell Execution cancellation idempotency key",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableCellExecutionStatus {
    Queued,
    Scheduled,
    Running,
    Cancelling,
    CleanupPending,
    Succeeded,
    Failed,
    Cancelled,
}

impl DurableCellExecutionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::CleanupPending => "cleanup_pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Aggregate-free Execution evidence returned to Durable Cells. Executions
/// remains the authority for all other lifecycle and Runtime fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellExecution {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: ExecutionId,
    pub target_node_id: NodeId,
    pub authority_kind: String,
    pub authority_subject_id: Uuid,
    pub authority_digest: Sha256Digest,
    pub status: DurableCellExecutionStatus,
    pub aggregate_version: u64,
    pub finished_at: Option<DateTime<Utc>>,
}

impl DurableCellExecution {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.target_node_id.as_uuid().is_nil()
            || self.authority_kind.trim().is_empty()
            || self.authority_subject_id.is_nil()
            || Sha256Digest::parse(self.authority_digest.as_str())? != self.authority_digest
            || self.aggregate_version == 0
            || self
                .finished_at
                .is_some_and(|value| value != canonical_timestamp(value))
        {
            return Err("Durable Cell Execution projection identity is invalid".into());
        }
        Ok(())
    }
}

/// Durable Cells' sole application boundary for the existing Execution
/// lifecycle. Implementations live in an outer anti-corruption adapter.
#[async_trait]
pub trait IDurableCellExecutionPort: Send + Sync {
    async fn find_bound_task(
        &self,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
    ) -> ApplicationResult<Option<DurableCellExecution>>;

    async fn ensure_bound_task(
        &self,
        request: &DurableCellExecutionRequest,
    ) -> ApplicationResult<DurableCellExecution>;

    async fn cancel_bound_task(
        &self,
        request: &DurableCellExecutionCancellationRequest,
    ) -> ApplicationResult<DurableCellExecution>;
}

fn validate_template(template: &DurableCellExecutionTemplate) -> Result<(), String> {
    validate_artifact(&template.artifact, "Durable Cell Execution artifact")?;
    if template.process.command.len() > 64 || template.process.args.len() > 256 {
        return Err("Durable Cell Execution process bounds are invalid".into());
    }
    for value in template
        .process
        .command
        .iter()
        .chain(&template.process.args)
    {
        validate_text(
            value,
            MAX_STRING_BYTES,
            "Durable Cell Execution process value",
        )?;
    }
    if let Some(path) = &template.process.working_directory {
        if !path.starts_with('/') || path.contains("..") {
            return Err("Durable Cell Execution working directory is invalid".into());
        }
    }
    if template.process.environment.len() > 256
        || template
            .process
            .environment
            .iter()
            .any(|(name, value)| name.trim().is_empty() || value.contains('\0'))
    {
        return Err("Durable Cell Execution environment is invalid".into());
    }
    canonical_json_bounded(&template.input, 16 * 1024, "Durable Cell Execution input")?;
    if template.resources.cpu_millis == 0
        || template.resources.memory_bytes == 0
        || template.resources.pids == 0
        || template
            .resources
            .execution_timeout_ms
            .is_none_or(|timeout| timeout == 0)
    {
        return Err("Durable Cell finite Task resources are invalid".into());
    }
    Ok(())
}

fn validate_task_policy(policy: &DurableCellExecutionTaskPolicy) -> Result<(), String> {
    let authority = &policy.authority;
    validate_authority_kind(&authority.kind)?;
    if authority.subject_id.is_nil() {
        return Err("Durable Cell Execution authority is invalid".into());
    }
    Sha256Digest::parse(authority.digest.as_str())?;
    Sha256Digest::parse(policy.semantics_profile_digest.as_str())?;
    if policy.mounts.is_empty()
        || policy.mounts.len() > 128
        || policy.secrets.is_empty()
        || policy.secrets.len() > 128
    {
        return Err("Durable Cell Execution Task policy cardinality is invalid".into());
    }
    let mut mount_names = BTreeSet::new();
    let mut mount_targets = BTreeSet::new();
    for mount in &policy.mounts {
        validate_text(&mount.name, 255, "Durable Cell Execution mount name")?;
        validate_artifact(&mount.artifact, "Durable Cell Execution mount artifact")?;
        if !mount.target.starts_with('/')
            || mount.target.contains("..")
            || !mount_names.insert(mount.name.as_str())
            || !mount_targets.insert(mount.target.as_str())
        {
            return Err("Durable Cell Execution artifact mounts are invalid".into());
        }
    }
    let mut secret_names = BTreeSet::new();
    let mut secret_targets = BTreeSet::new();
    for secret in &policy.secrets {
        validate_text(&secret.name, 255, "Durable Cell Execution Secret name")?;
        validate_text(
            &secret.reference,
            MAX_SECRET_REFERENCE_BYTES,
            "Durable Cell Execution Secret reference",
        )?;
        let target_key = match &secret.target {
            SecretTarget::Environment { variable } => {
                validate_text(variable, 255, "Durable Cell Execution environment target")?;
                format!("environment:{variable}")
            }
            SecretTarget::File { path, mode } => {
                if !path.starts_with('/') || path.contains("..") || *mode == 0 || *mode > 0o777 {
                    return Err("Durable Cell Execution file Secret target is invalid".into());
                }
                format!("file:{path}")
            }
            SecretTarget::RegistryCredential => "registry_credential".to_owned(),
        };
        if !secret_names.insert(secret.name.as_str()) || !secret_targets.insert(target_key) {
            return Err("Durable Cell Execution Secrets must be unique".into());
        }
    }
    Ok(())
}

fn validate_authority_kind(kind: &str) -> Result<(), String> {
    validate_text(
        kind,
        MAX_AUTHORITY_KIND_BYTES,
        "Durable Cell Execution authority kind",
    )?;
    let mut bytes = kind.bytes();
    if !bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        || !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
    {
        return Err("Durable Cell Execution authority kind is invalid".into());
    }
    Ok(())
}

fn validate_artifact(artifact: &ArtifactRef, label: &str) -> Result<(), String> {
    validate_text(&artifact.uri, 4096, &format!("{label} URI"))?;
    if !artifact.uri.contains("://") {
        return Err(format!("{label} URI is invalid"));
    }
    Sha256Digest::parse(&artifact.digest)?;
    validate_text(&artifact.media_type, 255, &format!("{label} media type"))
}

fn validate_text(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{artifact_uri, CloudSecretReference, DURABLE_CELL_BUNDLE_MEDIA_TYPE};
    use std::collections::BTreeMap;

    fn request() -> DurableCellExecutionRequest {
        let subject_id = Uuid::now_v7();
        let bundle_digest = Sha256Digest::from_bytes(b"durable-cell-bundle");
        let authority_digest = Sha256Digest::from_bytes(b"durable-cell-publication-authority");
        DurableCellExecutionRequest {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            execution_id: ExecutionId::new(),
            template: DurableCellExecutionTemplate {
                artifact: ArtifactRef {
                    uri: format!(
                        "oci://registry.example/a3s/celld@{}",
                        Sha256Digest::from_bytes(b"celld-image")
                    ),
                    digest: Sha256Digest::from_bytes(b"celld-image").to_string(),
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: RuntimeProcessSpec {
                    command: vec!["/usr/local/bin/celld".into()],
                    args: vec!["deploy".into(), "/workspace/bundle".into()],
                    working_directory: Some("/workspace".into()),
                    environment: BTreeMap::new(),
                },
                input: serde_json::json!({"schema": "cloud.durable-cell.bundle-publication.v1"}),
                resources: ResourceLimits {
                    cpu_millis: 250,
                    memory_bytes: 128 * 1024 * 1024,
                    pids: 64,
                    ephemeral_storage_bytes: Some(512 * 1024 * 1024),
                    execution_timeout_ms: Some(30_000),
                },
            },
            target_node_id: NodeId::new(),
            task_policy: DurableCellExecutionTaskPolicy {
                authority: DurableCellExecutionAuthority {
                    kind: "durable-cell.bundle-publication".into(),
                    subject_id,
                    digest: authority_digest,
                },
                mounts: vec![DurableCellExecutionArtifactMount {
                    name: "durable-cell-application".into(),
                    artifact: ArtifactRef {
                        uri: artifact_uri(bundle_digest.as_str()).expect("artifact URI"),
                        digest: bundle_digest.to_string(),
                        media_type: DURABLE_CELL_BUNDLE_MEDIA_TYPE.into(),
                    },
                    target: "/workspace/bundle".into(),
                }],
                secrets: vec![SecretReference {
                    name: "s0-access-key-id".into(),
                    reference: CloudSecretReference::new(subject_id, Uuid::now_v7(), 1)
                        .expect("Secret reference")
                        .to_string(),
                    target: SecretTarget::Environment {
                        variable: "AWS_ACCESS_KEY_ID".into(),
                    },
                }],
                semantics_profile_digest: Sha256Digest::from_bytes(b"publisher-profile"),
            },
            authority_subject_id: subject_id,
            idempotency_key: "durable-cell-publication:test".into(),
            request_id: Uuid::now_v7(),
            requested_at: canonical_timestamp(Utc::now()),
        }
    }

    #[test]
    fn accepts_one_exact_finite_bound_task_request() {
        request().validate().expect("valid Execution request");
    }

    #[test]
    fn rejects_authority_subject_drift() {
        let mut request = request();
        request.authority_subject_id = Uuid::now_v7();
        assert!(request.validate().is_err());
    }

    #[test]
    fn rejects_unbounded_task_resources() {
        let mut request = request();
        request.template.resources.execution_timeout_ms = None;
        assert!(request.validate().is_err());
    }

    #[test]
    fn cancellation_requires_exact_owner_scope_and_authority() {
        let execution = request();
        let mut cancellation = DurableCellExecutionCancellationRequest {
            organization_id: execution.organization_id,
            project_id: execution.project_id,
            environment_id: execution.environment_id,
            execution_id: execution.execution_id,
            authority_kind: execution.task_policy.authority.kind,
            authority_subject_id: execution.authority_subject_id,
            idempotency_key: "durable-cell-publication-cancel:test".into(),
            request_id: Uuid::now_v7(),
            requested_at: canonical_timestamp(Utc::now()),
        };
        cancellation.validate().expect("valid cancellation request");
        cancellation.authority_kind = "Execution".into();
        assert!(cancellation.validate().is_err());
    }
}
