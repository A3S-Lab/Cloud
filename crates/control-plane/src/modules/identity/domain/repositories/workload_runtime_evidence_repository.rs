use crate::modules::identity::domain::entities::{
    AcceptedWorkloadIdentityPolicyRevision, WorkloadRuntimeEvidenceBindingId,
    WorkloadRuntimeEvidenceRecord,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotencyRequest, IdempotentWrite, InstallationId, OrganizationId,
    RepositoryError, ResourceClaimId, WorkloadId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const MAX_WORKLOAD_RUNTIME_EVIDENCE_HISTORY_PAGE: usize = 100;
pub const DEFAULT_WORKLOAD_RUNTIME_EVIDENCE_HISTORY_PAGE: usize = 50;
pub const WORKLOAD_RUNTIME_EVIDENCE_IDEMPOTENCY_SCOPE: &str = "identity.workload-runtime-evidence";
pub const WORKLOAD_RUNTIME_EVIDENCE_ADMISSION_SCHEMA: &str =
    "cloud.identity.workload-runtime-evidence-admission.v1";

pub fn workload_runtime_evidence_idempotency_scope(organization_id: OrganizationId) -> String {
    format!(
        "{WORKLOAD_RUNTIME_EVIDENCE_IDEMPOTENCY_SCOPE}:{}",
        organization_id.as_uuid()
    )
}

pub fn workload_runtime_evidence_idempotency(
    admission_id: Uuid,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    resource_claim_id: ResourceClaimId,
    evaluated_at: DateTime<Utc>,
) -> Result<IdempotencyRequest, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CanonicalAdmission {
        schema: &'static str,
        admission_id: Uuid,
        installation_id: InstallationId,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        resource_claim_id: ResourceClaimId,
        evaluated_at: DateTime<Utc>,
    }

    let canonical = CanonicalAdmission {
        schema: WORKLOAD_RUNTIME_EVIDENCE_ADMISSION_SCHEMA,
        admission_id,
        installation_id,
        organization_id,
        workload_id,
        resource_claim_id,
        evaluated_at: canonical_timestamp(evaluated_at),
    };
    if admission_id.is_nil()
        || installation_id.as_uuid().is_nil()
        || organization_id.as_uuid().is_nil()
        || workload_id.as_uuid().is_nil()
        || resource_claim_id.as_uuid().is_nil()
        || canonical.evaluated_at.timestamp_millis() <= 0
        || evaluated_at != canonical.evaluated_at
    {
        return Err("workload Runtime evidence idempotency identity is invalid".into());
    }
    let body = serde_json::to_vec(&canonical)
        .map_err(|error| format!("could not canonicalize workload Runtime evidence: {error}"))?;
    IdempotencyRequest::new(
        workload_runtime_evidence_idempotency_scope(organization_id),
        admission_id.to_string(),
        &body,
    )
}

#[derive(Debug, Clone)]
pub struct RecordWorkloadRuntimeEvidenceWrite {
    pub record: WorkloadRuntimeEvidenceRecord,
    pub expected_policy: AcceptedWorkloadIdentityPolicyRevision,
    pub admission_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ReplayWorkloadRuntimeEvidenceAdmission {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub resource_claim_id: ResourceClaimId,
    pub evaluated_at: DateTime<Utc>,
    pub admission_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl ReplayWorkloadRuntimeEvidenceAdmission {
    pub fn validate(&self) -> Result<(), String> {
        let expected = workload_runtime_evidence_idempotency(
            self.admission_id,
            self.installation_id,
            self.organization_id,
            self.workload_id,
            self.resource_claim_id,
            self.evaluated_at,
        )?;
        if self.idempotency != expected {
            return Err("workload Runtime evidence replay identity is invalid".into());
        }
        Ok(())
    }
}

impl RecordWorkloadRuntimeEvidenceWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate_against_policy(&self.expected_policy)?;
        let candidate = &self.record.binding().candidate;
        let expected = workload_runtime_evidence_idempotency(
            self.admission_id,
            candidate.installation_id,
            candidate.organization_id,
            candidate.workload_id,
            candidate.resource_claim_id,
            self.record.admitted_at(),
        )?;
        if self.idempotency != expected {
            return Err("workload Runtime evidence admission identity is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadWorkloadRuntimeEvidence {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub binding_id: WorkloadRuntimeEvidenceBindingId,
}

impl ReadWorkloadRuntimeEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if self.installation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.binding_id.as_uuid().is_nil()
        {
            return Err("workload Runtime evidence read identity is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListWorkloadRuntimeEvidenceHistory {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub limit: usize,
}

impl ListWorkloadRuntimeEvidenceHistory {
    pub fn validate(&self) -> Result<(), String> {
        if self.installation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || !(1..=MAX_WORKLOAD_RUNTIME_EVIDENCE_HISTORY_PAGE).contains(&self.limit)
        {
            return Err("workload Runtime evidence history query is invalid".into());
        }
        Ok(())
    }
}

/// The sole Identity persistence authority for normalized Runtime evidence.
/// Records are historic, immutable and deliberately non-authorizing.
#[async_trait]
pub trait IWorkloadRuntimeEvidenceRepository: Send + Sync {
    /// Resolves only an exact historic idempotency replay. A miss carries no
    /// admission authority and must continue through current owner reads.
    async fn replay_admission(
        &self,
        replay: ReplayWorkloadRuntimeEvidenceAdmission,
    ) -> Result<Option<IdempotentWrite<WorkloadRuntimeEvidenceRecord>>, RepositoryError>;

    async fn record(
        &self,
        write: RecordWorkloadRuntimeEvidenceWrite,
    ) -> Result<IdempotentWrite<WorkloadRuntimeEvidenceRecord>, RepositoryError>;

    async fn read(
        &self,
        read: ReadWorkloadRuntimeEvidence,
    ) -> Result<Option<WorkloadRuntimeEvidenceRecord>, RepositoryError>;

    async fn list_history(
        &self,
        read: ListWorkloadRuntimeEvidenceHistory,
    ) -> Result<Vec<WorkloadRuntimeEvidenceRecord>, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_idempotency_digest_is_canonical_and_binds_every_input() {
        let admission_id = Uuid::now_v7();
        let installation_id = InstallationId::new();
        let organization_id = OrganizationId::new();
        let workload_id = WorkloadId::new();
        let claim_id = ResourceClaimId::new();
        let evaluated_at = canonical_timestamp(Utc::now());
        let first = workload_runtime_evidence_idempotency(
            admission_id,
            installation_id,
            organization_id,
            workload_id,
            claim_id,
            evaluated_at,
        )
        .expect("canonical admission");
        let replay = workload_runtime_evidence_idempotency(
            admission_id,
            installation_id,
            organization_id,
            workload_id,
            claim_id,
            evaluated_at,
        )
        .expect("canonical replay");
        let drift = workload_runtime_evidence_idempotency(
            admission_id,
            installation_id,
            organization_id,
            workload_id,
            ResourceClaimId::new(),
            evaluated_at,
        )
        .expect("drifted admission");
        assert_eq!(first, replay);
        assert_ne!(first.request_digest, drift.request_digest);
    }
}
