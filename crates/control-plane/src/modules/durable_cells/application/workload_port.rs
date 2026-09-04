use crate::modules::durable_cells::domain::DurableCellProviderWorkloadProjection;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, DurableCellApplicationId, DurableCellApplicationRevisionId,
    EnvironmentId, IdempotencyRequest, NodeId, NodePoolId, OperationId, OrganizationId, ProjectId,
    Sha256Digest, WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
};
use a3s_runtime::contract::{SecretReference, SecretTarget};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_WORKLOAD_TEMPLATE_BYTES: usize = 1024 * 1024;
const MAX_WORKLOAD_ARTIFACT_URI_BYTES: usize = 4096;

/// Opaque, immutable bytes for the already-resolved Workloads Service
/// template. Workloads remains the authority for interpreting this payload;
/// Durable Cells only fences it with the exact template digest while passing
/// it through its consumer-owned port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadTemplate {
    bytes: Vec<u8>,
    digest: Sha256Digest,
}

impl DurableCellWorkloadTemplate {
    /// Encodes one owner contract without importing the owner model into this
    /// application port. The byte representation is intentionally retained so
    /// a replay cannot silently normalize or alter the template.
    pub fn from_serializable<T: Serialize>(
        value: &T,
        digest: Sha256Digest,
    ) -> Result<Self, String> {
        let bytes = serde_json::to_vec(value)
            .map_err(|error| format!("could not encode Workloads template: {error}"))?;
        Self::new(bytes, digest)
    }

    pub fn new(bytes: Vec<u8>, digest: Sha256Digest) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_WORKLOAD_TEMPLATE_BYTES {
            return Err("Durable Cell Workloads template payload is out of bounds".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            format!("Durable Cell Workloads template is not valid JSON: {error}")
        })?;
        if !value.is_object() {
            return Err("Durable Cell Workloads template payload must be an object".into());
        }
        if Sha256Digest::from_bytes(&bytes) != digest {
            return Err("Durable Cell Workloads template digest does not match its payload".into());
        }
        Sha256Digest::parse(digest.as_str())?;
        Ok(Self { bytes, digest })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

/// Exact owner-neutral intent for creating one managed Workloads deployment.
/// All aggregate construction, idempotency replay, and lifecycle writes stay
/// inside the Workloads adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadDeploymentRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub application_revision_number: u64,
    pub application_definition_digest: Sha256Digest,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub workload_generation: u64,
    pub provider_artifact_digest: Sha256Digest,
    pub placement_policy_digest: Sha256Digest,
    pub service_template: DurableCellWorkloadTemplate,
    pub node_pool_id: Option<NodePoolId>,
    pub idempotency: IdempotencyRequest,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellWorkloadDeploymentRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        application_revision_id: DurableCellApplicationRevisionId,
        application_revision_number: u64,
        application_definition_digest: Sha256Digest,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        operation_id: OperationId,
        workload_generation: u64,
        provider_artifact_digest: Sha256Digest,
        placement_policy_digest: Sha256Digest,
        service_template: DurableCellWorkloadTemplate,
        node_pool_id: Option<NodePoolId>,
        idempotency: IdempotencyRequest,
        request_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            application_id,
            application_revision_id,
            application_revision_number,
            application_definition_digest,
            workload_id,
            workload_revision_id,
            deployment_id,
            operation_id,
            workload_generation,
            provider_artifact_digest,
            placement_policy_digest,
            service_template,
            node_pool_id,
            idempotency,
            request_id,
            requested_at,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_revision_id.as_uuid().is_nil()
            || self.application_revision_number == 0
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.deployment_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.workload_generation == 0
            || self.request_id.is_nil()
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Durable Cell Workloads deployment identity is invalid".into());
        }
        Sha256Digest::parse(self.application_definition_digest.as_str())?;
        Sha256Digest::parse(self.provider_artifact_digest.as_str())?;
        Sha256Digest::parse(self.placement_policy_digest.as_str())?;
        if let Some(node_pool_id) = self.node_pool_id {
            if node_pool_id.as_uuid().is_nil() {
                return Err("Durable Cell Workloads node-pool identity is invalid".into());
            }
        }
        self.idempotency.validate()?;
        if self.service_template.bytes().len() > MAX_WORKLOAD_TEMPLATE_BYTES {
            return Err("Durable Cell Workloads template payload is out of bounds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableCellWorkloadDeploymentStatus {
    Queued,
    Resolving,
    Scheduled,
    Applying,
    Verifying,
    Retiring,
    Cancelling,
    CleanupPending,
    Active,
    Failed,
    Orphaned,
    Cancelled,
}

impl DurableCellWorkloadDeploymentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Scheduled => "scheduled",
            Self::Applying => "applying",
            Self::Verifying => "verifying",
            Self::Retiring => "retiring",
            Self::Cancelling => "cancelling",
            Self::CleanupPending => "cleanup_pending",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Orphaned => "orphaned",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Aggregate-free projection returned by the Workloads owner for the Durable
/// Cells response. It deliberately contains no Workloads aggregate or event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub generation: u64,
    pub status: DurableCellWorkloadDeploymentStatus,
    pub deployment_aggregate_version: u64,
    pub artifact_source_uri: String,
    pub expected_artifact_digest: Option<Sha256Digest>,
    pub request_digest: Sha256Digest,
    pub artifact_digest: Option<Sha256Digest>,
    pub template_digest: Option<Sha256Digest>,
    pub requested_at: DateTime<Utc>,
    pub replayed: bool,
}

impl DurableCellWorkloadDeployment {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.deployment_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.generation == 0
            || self.deployment_aggregate_version == 0
            || self.artifact_source_uri.is_empty()
            || self.artifact_source_uri.len() > MAX_WORKLOAD_ARTIFACT_URI_BYTES
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Durable Cell Workloads deployment projection is invalid".into());
        }
        Sha256Digest::parse(self.request_digest.as_str())?;
        for digest in [
            self.expected_artifact_digest.as_ref(),
            self.artifact_digest.as_ref(),
            self.template_digest.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            Sha256Digest::parse(digest.as_str())?;
        }
        Ok(())
    }
}

/// Exact, owner-neutral Workloads facts required by the Durable Cell
/// pre-start publication gate. The Workloads adapter performs every aggregate
/// read and returns only immutable projections plus opaque template bytes;
/// Durable Cells does not receive a Workloads aggregate, repository, or event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadPrestartRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub application_revision_number: u64,
    pub application_definition_digest: Sha256Digest,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_generation: u64,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub node_id: NodeId,
}

impl DurableCellWorkloadPrestartRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        application_revision_id: DurableCellApplicationRevisionId,
        application_revision_number: u64,
        application_definition_digest: Sha256Digest,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        workload_generation: u64,
        deployment_id: DeploymentId,
        operation_id: OperationId,
        node_id: NodeId,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            application_id,
            application_revision_id,
            application_revision_number,
            application_definition_digest,
            workload_id,
            workload_revision_id,
            workload_generation,
            deployment_id,
            operation_id,
            node_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_revision_id.as_uuid().is_nil()
            || self.application_revision_number == 0
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.workload_generation == 0
            || self.deployment_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
        {
            return Err("Durable Cell Workloads pre-start identity is invalid".into());
        }
        Sha256Digest::parse(self.application_definition_digest.as_str())?;
        Ok(())
    }
}

/// Immutable Workloads projection consumed by the publication Execution
/// compiler. Secret references remain opaque Runtime references; their
/// plaintext is never present in this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadPrestartProjection {
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub writer_epoch: u64,
    pub provider_workload: DurableCellProviderWorkloadProjection,
    pub service_template: DurableCellWorkloadTemplate,
    pub runtime_secrets: Vec<SecretReference>,
}

impl DurableCellWorkloadPrestartProjection {
    pub fn validate_against(
        &self,
        request: &DurableCellWorkloadPrestartRequest,
    ) -> Result<(), String> {
        request.validate()?;
        if self.deployment_id != request.deployment_id
            || self.operation_id != request.operation_id
            || self.workload_id != request.workload_id
            || self.workload_revision_id != request.workload_revision_id
            || self.node_id != request.node_id
            || self.writer_epoch == 0
            || self.provider_workload.workload_id != request.workload_id
            || self.provider_workload.workload_revision_id != request.workload_revision_id
            || self.provider_workload.workload_generation != request.workload_generation
            || self.provider_workload.service_template_digest != *self.service_template.digest()
            || self.runtime_secrets.len() > 128
        {
            return Err("Durable Cell Workloads pre-start projection drifted".into());
        }
        self.provider_workload.validate()?;
        let mut names = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for secret in &self.runtime_secrets {
            validate_secret_reference(secret)?;
            let target = match &secret.target {
                SecretTarget::Environment { variable } => format!("environment:{variable}"),
                SecretTarget::File { path, .. } => format!("file:{path}"),
                SecretTarget::RegistryCredential => "registry_credential".into(),
            };
            if !names.insert(secret.name.as_str()) || !targets.insert(target) {
                return Err(
                    "Durable Cell Workloads pre-start Secret references are ambiguous".into(),
                );
            }
        }
        Ok(())
    }
}

/// Exact owner-neutral identity used when a stopped Durable Cell asks
/// Workloads to prepare a single-writer fence. The Workloads adapter checks
/// the mutable control projection and returns `None` for an ordinary
/// retirement, so the Durable Cells application never reads that repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadWriterFenceRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub application_revision_number: u64,
    pub application_definition_digest: Sha256Digest,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_generation: u64,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub replica_ordinal: u32,
}

impl DurableCellWorkloadWriterFenceRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        application_revision_id: DurableCellApplicationRevisionId,
        application_revision_number: u64,
        application_definition_digest: Sha256Digest,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        workload_generation: u64,
        replica_id: WorkloadReplicaId,
        replica_generation: u64,
        replica_ordinal: u32,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            application_id,
            application_revision_id,
            application_revision_number,
            application_definition_digest,
            workload_id,
            workload_revision_id,
            workload_generation,
            replica_id,
            replica_generation,
            replica_ordinal,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_revision_id.as_uuid().is_nil()
            || self.application_revision_number == 0
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.workload_generation == 0
            || self.replica_id.as_uuid().is_nil()
            || self.replica_generation == 0
        {
            return Err("Durable Cell Workloads writer-fence identity is invalid".into());
        }
        Sha256Digest::parse(self.application_definition_digest.as_str())?;
        Ok(())
    }
}

/// Immutable Workloads admission returned for the exact stopped current
/// Durable Cell replica. A missing value means the retirement belongs to a
/// different owner or an older rollout and must follow ordinary Workloads
/// cleanup semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadWriterFenceProjection {
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_generation: u64,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub replica_ordinal: u32,
}

impl DurableCellWorkloadWriterFenceProjection {
    pub fn validate_against(
        &self,
        request: &DurableCellWorkloadWriterFenceRequest,
    ) -> Result<(), String> {
        request.validate()?;
        if self.workload_id != request.workload_id
            || self.workload_revision_id != request.workload_revision_id
            || self.workload_generation != request.workload_generation
            || self.replica_id != request.replica_id
            || self.replica_generation != request.replica_generation
            || self.replica_ordinal != request.replica_ordinal
            || self.replica_generation == 0
        {
            return Err("Durable Cell Workloads writer-fence projection drifted".into());
        }
        Ok(())
    }
}

fn validate_secret_reference(secret: &SecretReference) -> Result<(), String> {
    if secret.name.is_empty()
        || secret.name.len() > 255
        || secret
            .name
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"-_ .".contains(&byte)))
        || secret.name.starts_with(['-', '_', '.', ' '])
        || secret.name.ends_with(['-', '_', '.', ' '])
        || secret.reference.is_empty()
        || secret.reference.len() > 1024
        || secret.name.contains(['\0', '\r', '\n'])
        || secret.reference.contains(['\0', '\r', '\n'])
    {
        return Err("Durable Cell Workloads pre-start Secret reference is invalid".into());
    }
    match &secret.target {
        SecretTarget::Environment { variable } => {
            let mut bytes = variable.bytes();
            if variable.len() > 255
                || !bytes
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err("Durable Cell Workloads Secret environment target is invalid".into());
            }
        }
        SecretTarget::File { path, mode } => {
            if path.is_empty()
                || path.len() > 4096
                || !path.starts_with('/')
                || path.contains('\0')
                || path.split('/').any(|segment| segment == "..")
                || *mode == 0
                || *mode > 0o777
            {
                return Err("Durable Cell Workloads Secret file target is invalid".into());
            }
        }
        SecretTarget::RegistryCredential => {}
    }
    Ok(())
}

/// Exact identity of the Durable Cell application whose managed Workload
/// projection must be reconciled.
///
/// Workloads remains the authority for replica state, scheduling, Runtime
/// fencing, and retirement. Durable Cells sends only this bounded intent
/// through its consumer-owned port; no Workloads aggregate or repository type
/// crosses the application boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCellWorkloadReconciliationRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
}

impl DurableCellWorkloadReconciliationRequest {
    pub const fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            application_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
        {
            return Err("Durable Cell Workload reconciliation identity is invalid".into());
        }
        Ok(())
    }
}

/// Exact Workloads revision identity used while composing a Durable Cell
/// deployment. Workloads owns the monotonic generation; Durable Cells sends
/// only the projected identity and immutable template digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadRevisionGenerationRequest {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub service_template_digest: Sha256Digest,
}

impl DurableCellWorkloadRevisionGenerationRequest {
    pub fn new(
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        service_template_digest: Sha256Digest,
    ) -> Self {
        Self {
            organization_id,
            workload_id,
            workload_revision_id,
            service_template_digest,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.service_template_digest.as_str())?
                != self.service_template_digest
        {
            return Err("Durable Cell Workloads revision identity is invalid".into());
        }
        Ok(())
    }
}

/// Durable Cells' sole application boundary for converging its deterministic
/// managed Workload projection. The owner adapter performs all aggregate
/// loading, authorization/integrity checks, idempotency, and replica-set
/// mutation while Workloads retains lifecycle authority.
#[async_trait]
pub trait IDurableCellWorkloadPort: Send + Sync {
    async fn load_prestart_publication(
        &self,
        request: &DurableCellWorkloadPrestartRequest,
    ) -> ApplicationResult<DurableCellWorkloadPrestartProjection>;

    async fn load_writer_fence_admission(
        &self,
        request: &DurableCellWorkloadWriterFenceRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadWriterFenceProjection>>;

    async fn replay_managed_deployment(
        &self,
        request: &DurableCellWorkloadDeploymentRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadDeployment>>;

    async fn create_managed_deployment(
        &self,
        request: &DurableCellWorkloadDeploymentRequest,
    ) -> ApplicationResult<DurableCellWorkloadDeployment>;

    async fn resolve_revision_generation(
        &self,
        request: &DurableCellWorkloadRevisionGenerationRequest,
    ) -> ApplicationResult<u64>;

    async fn converge_managed_replicas(
        &self,
        request: &DurableCellWorkloadReconciliationRequest,
    ) -> ApplicationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn reconciliation_request_requires_complete_scope() {
        let request = DurableCellWorkloadReconciliationRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            DurableCellApplicationId::new(),
        );
        request.validate().expect("valid reconciliation request");
    }

    #[test]
    fn reconciliation_request_rejects_nil_identity() {
        let request = DurableCellWorkloadReconciliationRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            DurableCellApplicationId::new(),
        );
        let mut invalid = request;
        invalid.application_id = DurableCellApplicationId::from_uuid(Uuid::nil());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn revision_generation_request_requires_a_valid_digest() {
        let request = DurableCellWorkloadRevisionGenerationRequest::new(
            OrganizationId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            Sha256Digest::from_bytes(b"template"),
        );
        request
            .validate()
            .expect("valid revision generation request");
    }

    #[test]
    fn revision_generation_request_rejects_a_nil_revision() {
        let request = DurableCellWorkloadRevisionGenerationRequest::new(
            OrganizationId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::from_uuid(Uuid::nil()),
            Sha256Digest::from_bytes(b"template"),
        );
        assert!(request.validate().is_err());
    }

    #[test]
    fn template_payload_is_locked_to_its_exact_bytes_and_digest() {
        let digest = Sha256Digest::from_bytes(br#"{"artifact":"pinned"}"#);
        let template = DurableCellWorkloadTemplate::from_serializable(
            &serde_json::json!({"artifact": "pinned"}),
            digest.clone(),
        )
        .expect("template payload");
        assert_eq!(template.digest(), &digest);

        let mut tampered = template.bytes().to_vec();
        tampered[2] = b'X';
        assert!(DurableCellWorkloadTemplate::new(tampered, digest).is_err());
    }

    #[test]
    fn template_payload_rejects_non_object_and_unbounded_input() {
        let digest = Sha256Digest::from_bytes(b"[]");
        assert!(DurableCellWorkloadTemplate::new(b"[]".to_vec(), digest).is_err());
        let bytes = vec![b'{'; MAX_WORKLOAD_TEMPLATE_BYTES + 1];
        assert!(
            DurableCellWorkloadTemplate::new(bytes.clone(), Sha256Digest::from_bytes(&bytes),)
                .is_err()
        );
    }

    #[test]
    fn prestart_projection_is_exactly_scoped_and_digest_locked() {
        let request = DurableCellWorkloadPrestartRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            DurableCellApplicationId::new(),
            DurableCellApplicationRevisionId::new(),
            4,
            Sha256Digest::from_bytes(b"application"),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            7,
            DeploymentId::new(),
            OperationId::new(),
            NodeId::new(),
        );
        let bytes =
            serde_json::to_vec(&serde_json::json!({"artifact": "pinned"})).expect("template bytes");
        let template_digest = Sha256Digest::from_bytes(&bytes);
        let projection = DurableCellWorkloadPrestartProjection {
            deployment_id: request.deployment_id,
            operation_id: request.operation_id,
            workload_id: request.workload_id,
            workload_revision_id: request.workload_revision_id,
            node_id: request.node_id,
            writer_epoch: 2,
            provider_workload: DurableCellProviderWorkloadProjection {
                workload_id: request.workload_id,
                workload_revision_id: request.workload_revision_id,
                workload_generation: request.workload_generation,
                service_template_digest: template_digest.clone(),
                provider_artifact_digest: Sha256Digest::from_bytes(b"provider"),
                ports: Vec::new(),
                health: None,
            },
            service_template: DurableCellWorkloadTemplate::new(bytes, template_digest)
                .expect("opaque template"),
            runtime_secrets: Vec::new(),
        };
        projection
            .validate_against(&request)
            .expect("exact pre-start projection");

        let mut drifted = projection;
        drifted.node_id = NodeId::new();
        assert!(drifted.validate_against(&request).is_err());
    }

    #[test]
    fn prestart_projection_rejects_invalid_secret_targets() {
        let secret = SecretReference {
            name: "s0-access-key-id".into(),
            reference: "cloud-secret://revision/secret/1".into(),
            target: SecretTarget::Environment {
                variable: "AWS_ACCESS_KEY_ID".into(),
            },
        };
        assert!(validate_secret_reference(&secret).is_ok());
        let invalid = SecretReference {
            target: SecretTarget::File {
                path: "/run/../secret".into(),
                mode: 0o600,
            },
            ..secret
        };
        assert!(validate_secret_reference(&invalid).is_err());
    }

    #[test]
    fn writer_fence_projection_is_exactly_bound_to_replica_identity() {
        let request = DurableCellWorkloadWriterFenceRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            DurableCellApplicationId::new(),
            DurableCellApplicationRevisionId::new(),
            2,
            Sha256Digest::from_bytes(b"application"),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            5,
            WorkloadReplicaId::new(),
            9,
            0,
        );
        let projection = DurableCellWorkloadWriterFenceProjection {
            workload_id: request.workload_id,
            workload_revision_id: request.workload_revision_id,
            workload_generation: request.workload_generation,
            replica_id: request.replica_id,
            replica_generation: request.replica_generation,
            replica_ordinal: request.replica_ordinal,
        };
        projection
            .validate_against(&request)
            .expect("exact writer-fence projection");

        let mut drifted = projection;
        drifted.replica_generation += 1;
        assert!(drifted.validate_against(&request).is_err());
    }
}
