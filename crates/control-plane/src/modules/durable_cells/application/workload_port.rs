use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, DurableCellApplicationId, DurableCellApplicationRevisionId,
    EnvironmentId, IdempotencyRequest, NodePoolId, OperationId, OrganizationId, ProjectId,
    Sha256Digest, WorkloadId, WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
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
}
