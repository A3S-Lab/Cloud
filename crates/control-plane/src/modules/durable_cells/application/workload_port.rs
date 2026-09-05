use super::storage_port::{
    DurableCellStorageCredentialRequest, DurableCellStorageProviderProfileProjection,
};
use crate::modules::durable_cells::domain::{
    DurableCellProjectionIdentity, DurableCellProviderWorkloadProjection,
    DurableCellPublisherProfile, DurableCellServiceProfile, DurableCellStorageBinding,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, DurableCellApplicationId, DurableCellApplicationRevisionId,
    EnvironmentId, IdempotencyRequest, NodeId, NodePoolId, OperationId, OrganizationId, ProjectId,
    SecretVersionReference, Sha256Digest, StorageNamespaceId, WorkloadId, WorkloadReplicaId,
    WorkloadRevisionId,
};
use a3s_runtime::contract::{RuntimeUnitSpec, SecretReference, SecretTarget};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_WORKLOAD_TEMPLATE_BYTES: usize = 1024 * 1024;
const MAX_WORKLOAD_ARTIFACT_URI_BYTES: usize = 4096;
const MAX_WORKLOAD_TEMPLATE_SECRET_REFERENCES: usize = 128;

/// The published identity form used by the ordinary Workloads Runtime
/// Service projection. Keeping this tiny identity helper beside the port
/// prevents consumer policy from reconstructing a foreign aggregate.
pub(crate) fn ordinary_runtime_unit_id(
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
) -> String {
    format!("workload:{}:revision:{}", workload_id, workload_revision_id)
}

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

/// Exact owner-neutral metadata projected from one opaque Workloads Service
/// template. Durable Cells may validate replay integrity, tenant scope, and
/// credential inclusion without interpreting the owner template itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadTemplateProjection {
    pub service_template_digest: Sha256Digest,
    pub provider_artifact_digest: Sha256Digest,
    pub secret_references: Vec<SecretVersionReference>,
}

impl DurableCellWorkloadTemplateProjection {
    pub fn new(
        service_template_digest: Sha256Digest,
        provider_artifact_digest: Sha256Digest,
        secret_references: Vec<SecretVersionReference>,
    ) -> Self {
        Self {
            service_template_digest,
            provider_artifact_digest,
            secret_references,
        }
    }

    pub fn validate_against(&self, template: &DurableCellWorkloadTemplate) -> Result<(), String> {
        if self.service_template_digest != *template.digest()
            || self.secret_references.len() > MAX_WORKLOAD_TEMPLATE_SECRET_REFERENCES
        {
            return Err("Durable Cell Workloads template binding projection is invalid".into());
        }
        Sha256Digest::parse(self.service_template_digest.as_str())?;
        Sha256Digest::parse(self.provider_artifact_digest.as_str())?;
        for reference in &self.secret_references {
            reference.validate()?;
        }
        Ok(())
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
    pub service_template_digest: Sha256Digest,
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
        service_template_digest: Sha256Digest,
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
            service_template_digest,
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
            || Sha256Digest::parse(self.service_template_digest.as_str())?
                != self.service_template_digest
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
    pub service_template: DurableCellWorkloadTemplate,
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
            || self.service_template.digest() != &request.service_template_digest
        {
            return Err("Durable Cell Workloads writer-fence projection drifted".into());
        }
        Ok(())
    }
}

/// Exact owner-neutral identity used when Durable Cells validates the
/// continuation seal for a previously fenced writer. Workloads remains the
/// authority for locating and validating the receipt; the consumer sends only
/// the current projection and the next epoch that is about to be admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadPriorWriterFenceRequest {
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
    pub replica_ordinal: u32,
    pub next_writer_epoch: u64,
}

impl DurableCellWorkloadPriorWriterFenceRequest {
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
        replica_ordinal: u32,
        next_writer_epoch: u64,
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
            replica_ordinal,
            next_writer_epoch,
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
            || self.replica_ordinal != 0
            || self.next_writer_epoch == 0
        {
            return Err("Durable Cell prior writer-fence identity is invalid".into());
        }
        Sha256Digest::parse(self.application_definition_digest.as_str())?;
        Ok(())
    }
}

/// Immutable Workloads receipt projection consumed while checking the prior
/// writer's namespace seal. Receipt owner metadata is validated inside the
/// Workloads adapter and is intentionally not exposed across this port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadPriorWriterFenceProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_revision_generation: u64,
    pub replica_id: WorkloadReplicaId,
    pub replica_ordinal: u32,
    pub writer_epoch: u64,
    pub continuation_operation_id: OperationId,
    pub fenced_at: DateTime<Utc>,
    pub receipt_digest: Sha256Digest,
}

impl DurableCellWorkloadPriorWriterFenceProjection {
    pub fn validate_against(
        &self,
        request: &DurableCellWorkloadPriorWriterFenceRequest,
    ) -> Result<(), String> {
        request.validate()?;
        if self.organization_id != request.organization_id
            || self.project_id != request.project_id
            || self.environment_id != request.environment_id
            || self.workload_id != request.workload_id
            || self.workload_revision_id != request.workload_revision_id
            || self.workload_revision_generation == 0
            || self.workload_revision_generation > request.workload_generation
            || self.replica_id != request.replica_id
            || self.replica_ordinal != request.replica_ordinal
            || self.writer_epoch == 0
            || self.writer_epoch >= request.next_writer_epoch
            || self.continuation_operation_id.as_uuid().is_nil()
            || self.fenced_at != canonical_timestamp(self.fenced_at)
        {
            return Err("Durable Cell prior writer-fence projection drifted".into());
        }
        Sha256Digest::parse(self.receipt_digest.as_str())?;
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

/// Exact owner-neutral placement intent used while binding a Durable Cell
/// deployment. Workloads compiles the control value and returns only its
/// immutable digest; no placement vocabulary crosses this port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadPlacementRequest {
    pub projection: DurableCellProjectionIdentity,
    pub workload_generation: u64,
    pub node_pool_id: Option<NodePoolId>,
}

impl DurableCellWorkloadPlacementRequest {
    pub fn new(
        projection: DurableCellProjectionIdentity,
        workload_generation: u64,
        node_pool_id: Option<NodePoolId>,
    ) -> Self {
        Self {
            projection,
            workload_generation,
            node_pool_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.projection.validate()?;
        if self.workload_generation == 0 {
            return Err("Durable Cell Workloads placement generation is invalid".into());
        }
        if let Some(node_pool_id) = self.node_pool_id {
            if node_pool_id.as_uuid().is_nil() {
                return Err(
                    "Durable Cell Workloads placement node-pool identity is invalid".into(),
                );
            }
        }
        Ok(())
    }
}

/// Exact owner-neutral input for projecting the provider Workload revision.
/// The Workloads adapter owns decoding and aggregate construction; Durable
/// Cells receives only its immutable provider projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadProviderProjectionRequest {
    pub projection: DurableCellProjectionIdentity,
    pub workload_generation: u64,
    pub service_template: DurableCellWorkloadTemplate,
}

/// Exact owner-neutral request for compiling one Workloads revision into the
/// generic Runtime Service contract. Workloads remains responsible for
/// loading the revision, decoding its Service template, and invoking the sole
/// Runtime compiler; Durable Cells supplies only the identity and immutable
/// profile digest that must be bound to the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadRuntimeProjectionRequest {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_generation: u64,
    pub service_template_digest: Sha256Digest,
    pub semantics_profile_digest: Sha256Digest,
    /// `None` means the revision's ordinary Service unit identity and
    /// generation. `Some` is reserved for an already-bound replica Runtime
    /// target whose identity is owned by Workloads.
    pub runtime_unit_id: Option<String>,
    pub runtime_generation: Option<u64>,
}

impl DurableCellWorkloadRuntimeProjectionRequest {
    pub fn for_revision(
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        workload_generation: u64,
        service_template_digest: Sha256Digest,
        semantics_profile_digest: Sha256Digest,
    ) -> Self {
        Self {
            organization_id,
            workload_id,
            workload_revision_id,
            workload_generation,
            service_template_digest,
            semantics_profile_digest,
            runtime_unit_id: None,
            runtime_generation: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn for_replica(
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        workload_generation: u64,
        service_template_digest: Sha256Digest,
        semantics_profile_digest: Sha256Digest,
        runtime_unit_id: String,
        runtime_generation: u64,
    ) -> Self {
        Self {
            organization_id,
            workload_id,
            workload_revision_id,
            workload_generation,
            service_template_digest,
            semantics_profile_digest,
            runtime_unit_id: Some(runtime_unit_id),
            runtime_generation: Some(runtime_generation),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.workload_generation == 0
            || Sha256Digest::parse(self.service_template_digest.as_str())?
                != self.service_template_digest
            || Sha256Digest::parse(self.semantics_profile_digest.as_str())?
                != self.semantics_profile_digest
        {
            return Err("Durable Cell Workloads Runtime projection identity is invalid".into());
        }
        match (&self.runtime_unit_id, self.runtime_generation) {
            (None, None) => {}
            (Some(unit_id), Some(generation))
                if !unit_id.trim().is_empty()
                    && unit_id.len() <= 512
                    && !unit_id.contains(['\0', '\r', '\n'])
                    && generation > 0 => {}
            _ => return Err("Durable Cell Workloads Runtime replica target is incomplete".into()),
        }
        Ok(())
    }

    fn expected_runtime_unit_id(&self) -> String {
        self.runtime_unit_id.clone().unwrap_or_else(|| {
            ordinary_runtime_unit_id(self.workload_id, self.workload_revision_id)
        })
    }

    fn expected_runtime_generation(&self) -> u64 {
        self.runtime_generation.unwrap_or(self.workload_generation)
    }
}

/// Immutable Workloads-owned projection returned by the Runtime compiler.
/// The provider projection is included so Durable Cells can bind the generic
/// Runtime spec to the same revision and artifact without importing a
/// Workloads aggregate or Service template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadRuntimeProjection {
    pub provider: DurableCellProviderWorkloadProjection,
    pub spec: RuntimeUnitSpec,
}

impl DurableCellWorkloadRuntimeProjection {
    pub fn new(
        provider: DurableCellProviderWorkloadProjection,
        spec: RuntimeUnitSpec,
    ) -> Result<Self, String> {
        let projection = Self { provider, spec };
        projection.validate()?;
        Ok(projection)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.provider.validate()?;
        self.spec.validate()?;
        if self.spec.generation == 0
            || self.spec.artifact.digest != self.provider.provider_artifact_digest.as_str()
        {
            return Err("Durable Cell Workloads Runtime projection drifted".into());
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        request: &DurableCellWorkloadRuntimeProjectionRequest,
    ) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        if self.provider.workload_id != request.workload_id
            || self.provider.workload_revision_id != request.workload_revision_id
            || self.provider.workload_generation != request.workload_generation
            || self.provider.service_template_digest != request.service_template_digest
            || self.spec.unit_id != request.expected_runtime_unit_id()
            || self.spec.generation != request.expected_runtime_generation()
            || self.spec.semantics_profile_digest.as_deref()
                != Some(request.semantics_profile_digest.as_str())
        {
            return Err(
                "Durable Cell Workloads Runtime projection changed its exact binding".into(),
            );
        }
        Ok(())
    }
}

/// Owner-neutral identity of one Workloads replica Runtime target. The
/// concrete `DeploymentReplicaBinding` stays inside the Workloads adapter;
/// Durable Cells only receives the fields needed to fence one exact receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadReplicaRuntimeBinding {
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub replica_ordinal: u32,
    pub node_id: NodeId,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
}

impl DurableCellWorkloadReplicaRuntimeBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        replica_id: WorkloadReplicaId,
        replica_generation: u64,
        replica_ordinal: u32,
        node_id: NodeId,
        runtime_unit_id: String,
        runtime_generation: u64,
    ) -> Self {
        Self {
            workload_id,
            workload_revision_id,
            replica_id,
            replica_generation,
            replica_ordinal,
            node_id,
            runtime_unit_id,
            runtime_generation,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.replica_id.as_uuid().is_nil()
            || self.replica_generation == 0
            || self.node_id.as_uuid().is_nil()
            || self.runtime_generation != self.replica_generation
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
        {
            return Err("Durable Cell Workloads replica Runtime binding is invalid".into());
        }
        Ok(())
    }
}

impl DurableCellWorkloadProviderProjectionRequest {
    pub fn new(
        projection: DurableCellProjectionIdentity,
        workload_generation: u64,
        service_template: DurableCellWorkloadTemplate,
    ) -> Self {
        Self {
            projection,
            workload_generation,
            service_template,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.projection.validate()?;
        if self.workload_generation == 0 {
            return Err("Durable Cell Workloads provider generation is invalid".into());
        }
        Ok(())
    }
}

/// Exact owner-neutral input for validating the pinned provider Workload at
/// the Workloads boundary. The adapter decodes the opaque template and keeps
/// all Workloads model and provider-shape vocabulary on the owner side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadProviderValidationRequest {
    pub credentials: DurableCellStorageCredentialRequest,
    pub provider_profile: DurableCellStorageProviderProfileProjection,
    pub service_profile: DurableCellServiceProfile,
    pub service_template: DurableCellWorkloadTemplate,
    pub publisher: DurableCellPublisherProfile,
}

impl DurableCellWorkloadProviderValidationRequest {
    pub fn new(
        credentials: DurableCellStorageCredentialRequest,
        provider_profile: DurableCellStorageProviderProfileProjection,
        service_profile: DurableCellServiceProfile,
        service_template: DurableCellWorkloadTemplate,
        publisher: DurableCellPublisherProfile,
    ) -> Self {
        Self {
            credentials,
            provider_profile,
            service_profile,
            service_template,
            publisher,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.credentials.validate()?;
        self.provider_profile.validate()?;
        self.publisher.validate()?;
        Ok(())
    }
}

/// Exact owner-neutral request for validating an already-resolved provider
/// template during publication. The Workloads adapter is the only layer that
/// decodes the opaque template and applies its Service/profile rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadProviderTemplateValidationRequest {
    pub provider_profile: DurableCellStorageProviderProfileProjection,
    pub storage_namespace_id: StorageNamespaceId,
    pub service_profile: DurableCellServiceProfile,
    pub service_template: DurableCellWorkloadTemplate,
    pub publisher: DurableCellPublisherProfile,
}

impl DurableCellWorkloadProviderTemplateValidationRequest {
    pub fn new(
        provider_profile: DurableCellStorageProviderProfileProjection,
        storage_namespace_id: StorageNamespaceId,
        service_profile: DurableCellServiceProfile,
        service_template: DurableCellWorkloadTemplate,
        publisher: DurableCellPublisherProfile,
    ) -> Self {
        Self {
            provider_profile,
            storage_namespace_id,
            service_profile,
            service_template,
            publisher,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.provider_profile.validate()?;
        if self.storage_namespace_id.as_uuid().is_nil() {
            return Err("Durable Cell provider template namespace is invalid".into());
        }
        self.publisher.validate()?;
        Ok(())
    }
}

/// Bounded provider metadata returned after the Workloads owner validates an
/// opaque template. No Workloads Service model crosses the application port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadProviderTemplateProjection {
    pub artifact_media_type: String,
}

impl DurableCellWorkloadProviderTemplateProjection {
    pub fn new(artifact_media_type: impl Into<String>) -> Result<Self, String> {
        let projection = Self {
            artifact_media_type: artifact_media_type.into(),
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.artifact_media_type.is_empty()
            || self.artifact_media_type.len() > 255
            || self.artifact_media_type.contains(['\0', '\r', '\n'])
        {
            return Err("Durable Cell provider artifact media type is invalid".into());
        }
        Ok(())
    }
}

/// Owner-neutral request for deriving the exact S0 Secret references from an
/// opaque Workloads template. The Workloads adapter performs the translation;
/// no Service or Secret-binding model is exposed to Durable Cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellWorkloadProviderCredentialProjectionRequest {
    pub storage: DurableCellStorageBinding,
    pub service_template: DurableCellWorkloadTemplate,
    pub publisher: DurableCellPublisherProfile,
}

impl DurableCellWorkloadProviderCredentialProjectionRequest {
    pub fn new(
        storage: DurableCellStorageBinding,
        service_template: DurableCellWorkloadTemplate,
        publisher: DurableCellPublisherProfile,
    ) -> Self {
        Self {
            storage,
            service_template,
            publisher,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.storage.validate()?;
        self.publisher.validate()?;
        Ok(())
    }
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
    fn project_template(
        &self,
        template: &DurableCellWorkloadTemplate,
    ) -> ApplicationResult<DurableCellWorkloadTemplateProjection>;

    fn compile_placement_policy_digest(
        &self,
        request: &DurableCellWorkloadPlacementRequest,
    ) -> ApplicationResult<Sha256Digest>;

    fn project_provider_workload(
        &self,
        request: &DurableCellWorkloadProviderProjectionRequest,
    ) -> ApplicationResult<DurableCellProviderWorkloadProjection>;

    fn validate_provider_workload(
        &self,
        request: &DurableCellWorkloadProviderValidationRequest,
    ) -> ApplicationResult<()>;

    fn validate_provider_template(
        &self,
        request: &DurableCellWorkloadProviderTemplateValidationRequest,
    ) -> ApplicationResult<DurableCellWorkloadProviderTemplateProjection>;

    fn project_provider_credentials(
        &self,
        request: &DurableCellWorkloadProviderCredentialProjectionRequest,
    ) -> ApplicationResult<DurableCellStorageCredentialRequest>;

    async fn load_runtime_projection(
        &self,
        request: &DurableCellWorkloadRuntimeProjectionRequest,
    ) -> ApplicationResult<DurableCellWorkloadRuntimeProjection>;

    async fn load_prestart_publication(
        &self,
        request: &DurableCellWorkloadPrestartRequest,
    ) -> ApplicationResult<DurableCellWorkloadPrestartProjection>;

    async fn load_writer_fence_admission(
        &self,
        request: &DurableCellWorkloadWriterFenceRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadWriterFenceProjection>>;

    async fn load_prior_writer_fence(
        &self,
        request: &DurableCellWorkloadPriorWriterFenceRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadPriorWriterFenceProjection>>;

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
    use crate::modules::shared_kernel::domain::SecretId;
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

    fn placement_projection() -> DurableCellProjectionIdentity {
        let application_id = DurableCellApplicationId::new();
        let application_revision_id = DurableCellApplicationRevisionId::new();
        DurableCellProjectionIdentity {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            application_id,
            application_revision_id,
            application_revision_number: 1,
            application_definition_digest: Sha256Digest::from_bytes(b"application"),
            storage_namespace_id:
                DurableCellProjectionIdentity::storage_namespace_id_for_application(application_id),
            workload_id: DurableCellProjectionIdentity::workload_id_for_application(application_id),
            workload_revision_id:
                DurableCellProjectionIdentity::workload_revision_id_for_application_revision(
                    application_revision_id,
                ),
            deployment_id: DeploymentId::from_uuid(Uuid::new_v5(
                &application_revision_id.as_uuid(),
                b"a3s-cloud:durable-cell:workload-deployment:v1",
            )),
            operation_id: OperationId::from_uuid(Uuid::new_v5(
                &application_revision_id.as_uuid(),
                b"a3s-cloud:durable-cell:deployment-operation:v1",
            )),
        }
    }

    #[test]
    fn placement_request_requires_a_valid_generation_and_pool() {
        let request = DurableCellWorkloadPlacementRequest::new(
            placement_projection(),
            2,
            Some(NodePoolId::new()),
        );
        request.validate().expect("valid placement request");

        let mut invalid_generation = request.clone();
        invalid_generation.workload_generation = 0;
        assert!(invalid_generation.validate().is_err());

        let mut invalid_pool = request;
        invalid_pool.node_pool_id = Some(NodePoolId::from_uuid(Uuid::nil()));
        assert!(invalid_pool.validate().is_err());
    }

    #[test]
    fn provider_projection_request_requires_a_valid_generation() {
        let bytes =
            serde_json::to_vec(&serde_json::json!({"artifact": "pinned"})).expect("template bytes");
        let template =
            DurableCellWorkloadTemplate::new(bytes.clone(), Sha256Digest::from_bytes(&bytes))
                .expect("opaque template");
        let request =
            DurableCellWorkloadProviderProjectionRequest::new(placement_projection(), 2, template);
        request
            .validate()
            .expect("valid provider projection request");

        let mut invalid = request;
        invalid.workload_generation = 0;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn runtime_projection_request_keeps_revision_and_replica_targets_complete() {
        let template_digest = Sha256Digest::from_bytes(b"template");
        let semantics_digest = Sha256Digest::from_bytes(b"semantics");
        let revision = DurableCellWorkloadRuntimeProjectionRequest::for_revision(
            OrganizationId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            3,
            template_digest.clone(),
            semantics_digest.clone(),
        );
        revision.validate().expect("revision Runtime request");

        let replica = DurableCellWorkloadRuntimeProjectionRequest::for_replica(
            revision.organization_id,
            revision.workload_id,
            revision.workload_revision_id,
            revision.workload_generation,
            template_digest,
            semantics_digest,
            "workload:replica:1".into(),
            9,
        );
        replica.validate().expect("replica Runtime request");

        let mut incomplete = replica;
        incomplete.runtime_generation = None;
        assert!(incomplete.validate().is_err());
    }

    #[test]
    fn replica_runtime_binding_rejects_generation_and_node_drift() {
        let mut binding = DurableCellWorkloadReplicaRuntimeBinding::new(
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            WorkloadReplicaId::new(),
            4,
            0,
            NodeId::new(),
            "workload:replica:1".into(),
            4,
        );
        binding.validate().expect("valid replica Runtime binding");
        binding.runtime_generation = 3;
        assert!(binding.validate().is_err());
        binding.runtime_generation = 4;
        binding.node_id = NodeId::from_uuid(Uuid::nil());
        assert!(binding.validate().is_err());
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
    fn template_projection_is_digest_locked_and_bounded() {
        let bytes =
            serde_json::to_vec(&serde_json::json!({"artifact": "pinned"})).expect("template bytes");
        let template_digest = Sha256Digest::from_bytes(&bytes);
        let template = DurableCellWorkloadTemplate::new(bytes, template_digest.clone())
            .expect("opaque template");
        let provider_artifact_digest = Sha256Digest::from_bytes(b"provider artifact");
        let projection = DurableCellWorkloadTemplateProjection::new(
            template_digest,
            provider_artifact_digest.clone(),
            vec![SecretVersionReference::new(SecretId::new(), 1).expect("Secret reference")],
        );
        projection
            .validate_against(&template)
            .expect("template binding projection");
        assert_eq!(
            projection.provider_artifact_digest,
            provider_artifact_digest
        );

        let mut drifted = projection.clone();
        drifted.service_template_digest = Sha256Digest::from_bytes(b"other template");
        assert!(drifted.validate_against(&template).is_err());

        let mut invalid = projection.clone();
        invalid.secret_references[0].version = 0;
        assert!(invalid.validate_against(&template).is_err());

        let mut unbounded = projection;
        unbounded.secret_references = vec![
            SecretVersionReference::new(SecretId::new(), 1)
                .expect("Secret reference");
            MAX_WORKLOAD_TEMPLATE_SECRET_REFERENCES + 1
        ];
        assert!(unbounded.validate_against(&template).is_err());
    }

    #[test]
    fn provider_template_validation_request_requires_a_namespace_and_bounded_result() {
        let bytes =
            serde_json::to_vec(&serde_json::json!({"artifact": "pinned"})).expect("template bytes");
        let template =
            DurableCellWorkloadTemplate::new(bytes.clone(), Sha256Digest::from_bytes(&bytes))
                .expect("opaque template");
        let provider_profile = DurableCellStorageProviderProfileProjection {
            digest: Sha256Digest::from_bytes(b"provider profile"),
            endpoint: "https://storage.example.test".into(),
            region: "test".into(),
            bucket: "bucket".into(),
            prefix: "prefix".into(),
            virtual_hosted_style: false,
        };
        let request = DurableCellWorkloadProviderTemplateValidationRequest::new(
            provider_profile,
            StorageNamespaceId::new(),
            DurableCellServiceProfile::pinned_celld_v0_2_1().expect("service profile"),
            template,
            DurableCellPublisherProfile::pinned_celld_v0_2_1().expect("publisher"),
        );
        request.validate().expect("valid provider template request");

        let projection = DurableCellWorkloadProviderTemplateProjection::new(
            "application/vnd.oci.image.index.v1+json",
        )
        .expect("media type projection");
        projection.validate().expect("valid media type projection");

        let mut invalid_request = request;
        invalid_request.storage_namespace_id = StorageNamespaceId::from_uuid(Uuid::nil());
        assert!(invalid_request.validate().is_err());

        assert!(DurableCellWorkloadProviderTemplateProjection::new("\n").is_err());
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
        let template_bytes = br#"{"template":"x"}"#;
        let template_digest = Sha256Digest::from_bytes(template_bytes);
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
            template_digest.clone(),
            WorkloadReplicaId::new(),
            9,
            0,
        );
        let template = DurableCellWorkloadTemplate::new(template_bytes.to_vec(), template_digest)
            .expect("opaque template");
        let projection = DurableCellWorkloadWriterFenceProjection {
            workload_id: request.workload_id,
            workload_revision_id: request.workload_revision_id,
            workload_generation: request.workload_generation,
            replica_id: request.replica_id,
            replica_generation: request.replica_generation,
            replica_ordinal: request.replica_ordinal,
            service_template: template,
        };
        projection
            .validate_against(&request)
            .expect("exact writer-fence projection");

        let mut drifted = projection;
        drifted.replica_generation += 1;
        assert!(drifted.validate_against(&request).is_err());
    }

    #[test]
    fn prior_writer_fence_projection_is_locked_to_the_next_epoch() {
        let request = DurableCellWorkloadPriorWriterFenceRequest::new(
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
            0,
            12,
        );
        let projection = DurableCellWorkloadPriorWriterFenceProjection {
            organization_id: request.organization_id,
            project_id: request.project_id,
            environment_id: request.environment_id,
            workload_id: request.workload_id,
            workload_revision_id: request.workload_revision_id,
            workload_revision_generation: request.workload_generation,
            replica_id: request.replica_id,
            replica_ordinal: request.replica_ordinal,
            writer_epoch: 7,
            continuation_operation_id: OperationId::new(),
            fenced_at: canonical_timestamp(Utc::now()),
            receipt_digest: Sha256Digest::from_bytes(b"receipt"),
        };
        projection
            .validate_against(&request)
            .expect("exact prior writer-fence projection");

        let mut drifted = projection;
        drifted.writer_epoch = request.next_writer_epoch;
        assert!(drifted.validate_against(&request).is_err());
    }
}
