use super::{IWorkloadRuntimeEvidenceCandidatePort, WorkloadRuntimeEvidenceRequest};
use crate::modules::identity::domain::entities::WorkloadRuntimeEvidenceRecord;
use crate::modules::identity::domain::repositories::{
    workload_runtime_evidence_idempotency, IWorkloadIdentityPolicyRepository,
    IWorkloadRuntimeEvidenceRepository, ReadCurrentWorkloadIdentityPolicyForRuntime,
    RecordWorkloadRuntimeEvidenceWrite, ReplayWorkloadRuntimeEvidenceAdmission,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotencyRequest, IdempotentWrite, InstallationId, OrganizationId,
    RepositoryError, ResourceClaimId, WorkloadId,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordWorkloadRuntimeEvidence {
    admission_id: Uuid,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    resource_claim_id: ResourceClaimId,
    evaluated_at: DateTime<Utc>,
}

impl RecordWorkloadRuntimeEvidence {
    pub fn new(
        admission_id: Uuid,
        installation_id: InstallationId,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        resource_claim_id: ResourceClaimId,
        evaluated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            admission_id,
            installation_id,
            organization_id,
            workload_id,
            resource_claim_id,
            evaluated_at: canonical_timestamp(evaluated_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.admission_id.is_nil()
            || self.installation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.resource_claim_id.as_uuid().is_nil()
            || self.evaluated_at.timestamp_millis() <= 0
            || self.evaluated_at != canonical_timestamp(self.evaluated_at)
        {
            return Err("workload Runtime evidence command identity or time is invalid".into());
        }
        Ok(())
    }

    fn idempotency(&self) -> Result<IdempotencyRequest, RepositoryError> {
        workload_runtime_evidence_idempotency(
            self.admission_id,
            self.installation_id,
            self.organization_id,
            self.workload_id,
            self.resource_claim_id,
            self.evaluated_at,
        )
        .map_err(RepositoryError::Conflict)
    }
}

/// Internal Identity application service for the C3b Runtime component fact.
/// It intentionally has no REST surface and does not issue credentials.
pub struct WorkloadRuntimeEvidenceRecorder {
    policies: Arc<dyn IWorkloadIdentityPolicyRepository>,
    candidates: Arc<dyn IWorkloadRuntimeEvidenceCandidatePort>,
    evidence: Arc<dyn IWorkloadRuntimeEvidenceRepository>,
}

impl WorkloadRuntimeEvidenceRecorder {
    pub fn new(
        policies: Arc<dyn IWorkloadIdentityPolicyRepository>,
        candidates: Arc<dyn IWorkloadRuntimeEvidenceCandidatePort>,
        evidence: Arc<dyn IWorkloadRuntimeEvidenceRepository>,
    ) -> Self {
        Self {
            policies,
            candidates,
            evidence,
        }
    }

    pub async fn record(
        &self,
        command: RecordWorkloadRuntimeEvidence,
    ) -> Result<IdempotentWrite<WorkloadRuntimeEvidenceRecord>, RepositoryError> {
        command.validate().map_err(RepositoryError::Conflict)?;
        let idempotency = command.idempotency()?;
        if let Some(replayed) = self
            .evidence
            .replay_admission(ReplayWorkloadRuntimeEvidenceAdmission {
                installation_id: command.installation_id,
                organization_id: command.organization_id,
                workload_id: command.workload_id,
                resource_claim_id: command.resource_claim_id,
                evaluated_at: command.evaluated_at,
                admission_id: command.admission_id,
                idempotency: idempotency.clone(),
            })
            .await?
        {
            return Ok(replayed);
        }

        let policy = self
            .policies
            .read_current_for_runtime(ReadCurrentWorkloadIdentityPolicyForRuntime {
                organization_id: command.organization_id,
                workload_id: command.workload_id,
            })
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "workload Runtime evidence has no current Identity policy".into(),
                )
            })?;
        let spec = policy.contract.spec();
        if policy.installation_id != command.installation_id
            || spec.installation_id != command.installation_id
            || spec.organization_id != command.organization_id
            || spec.workload_id != command.workload_id
        {
            return Err(RepositoryError::Conflict(
                "workload Runtime evidence policy changed command lineage".into(),
            ));
        }
        let request = WorkloadRuntimeEvidenceRequest::for_policy(
            &policy,
            command.resource_claim_id,
            command.evaluated_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let candidate = self.candidates.read_candidate(request.clone()).await?;
        request
            .validate_candidate(&candidate)
            .map_err(RepositoryError::Conflict)?;
        let record = WorkloadRuntimeEvidenceRecord::admit_runtime_component(
            &policy,
            candidate,
            command.evaluated_at,
        )
        .map_err(RepositoryError::Conflict)?;
        debug_assert!(!record.authorizes_credential_issuance());

        self.evidence
            .record(RecordWorkloadRuntimeEvidenceWrite {
                record,
                expected_policy: policy,
                admission_id: command.admission_id,
                idempotency,
            })
            .await
    }
}
