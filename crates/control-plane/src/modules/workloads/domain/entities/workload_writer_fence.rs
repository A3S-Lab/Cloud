use super::ManagedOwnerReference;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, EnvironmentId, NodeCommandId, NodeId, OperationId,
    OrganizationId, ProjectId, Sha256Digest, WorkloadId, WorkloadReplicaId,
    WorkloadReplicaMemberId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKLOAD_WRITER_FENCE_RECEIPT_SCHEMA: &str = "cloud.workload.writer-fence-receipt.v1";
const MAX_RUNTIME_UNIT_ID_BYTES: usize = 512;
const MAX_RECEIPT_BYTES: usize = 16 * 1024;
const MAX_SAFE_SERIALIZED_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_WORKLOAD_REPLICAS: u32 = 100;

/// Workloads-owned proof that one exact managed Runtime generation can no
/// longer write. The owner-specific adapter decides whether this fence closes
/// an external state namespace; Workloads owns only the Runtime fact and the
/// atomic handoff to the returned continuation Operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadWriterFenceReceiptSpec {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_revision_generation: u64,
    pub replica_id: WorkloadReplicaId,
    pub replica_ordinal: u32,
    pub writer_epoch: u64,
    pub member_id: WorkloadReplicaMemberId,
    pub placement_generation: u64,
    pub managed_owner: ManagedOwnerReference,
    pub node_id: NodeId,
    pub runtime_unit_id: String,
    pub command_id: NodeCommandId,
    pub command_payload_digest: Sha256Digest,
    pub acknowledgement_digest: Sha256Digest,
    pub continuation_operation_id: OperationId,
    pub fenced_at: DateTime<Utc>,
}

impl WorkloadWriterFenceReceiptSpec {
    pub fn validate(&self) -> Result<(), String> {
        self.managed_owner.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.workload_revision_generation == 0
            || self.workload_revision_generation > MAX_SAFE_SERIALIZED_INTEGER
            || self.replica_id.as_uuid().is_nil()
            || self.replica_ordinal >= MAX_WORKLOAD_REPLICAS
            || self.writer_epoch == 0
            || self.writer_epoch > MAX_SAFE_SERIALIZED_INTEGER
            || self.member_id.as_uuid().is_nil()
            || self.placement_generation == 0
            || self.placement_generation > MAX_SAFE_SERIALIZED_INTEGER
            || self.node_id.as_uuid().is_nil()
            || !valid_runtime_unit_id(&self.runtime_unit_id)
            || self.command_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.command_payload_digest.as_str())?
                != self.command_payload_digest
            || Sha256Digest::parse(self.acknowledgement_digest.as_str())?
                != self.acknowledgement_digest
            || self.continuation_operation_id.as_uuid().is_nil()
            || self.fenced_at != canonical_timestamp(self.fenced_at)
        {
            return Err("Workload writer-fence receipt spec is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadWriterFenceReceipt {
    spec: WorkloadWriterFenceReceiptSpec,
    digest: Sha256Digest,
}

impl WorkloadWriterFenceReceipt {
    pub fn issue(mut spec: WorkloadWriterFenceReceiptSpec) -> Result<Self, String> {
        spec.fenced_at = canonical_timestamp(spec.fenced_at);
        spec.validate()?;
        let digest = receipt_digest(&spec)?;
        Ok(Self { spec, digest })
    }

    pub fn restore(
        spec: WorkloadWriterFenceReceiptSpec,
        stored_digest: &str,
    ) -> Result<Self, String> {
        let receipt = Self::issue(spec)?;
        if receipt.digest.as_str() != stored_digest {
            return Err("stored Workload writer-fence receipt digest does not match".into());
        }
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.spec.validate()?;
        if receipt_digest(&self.spec)? != self.digest {
            return Err("Workload writer-fence receipt drifted".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &WorkloadWriterFenceReceiptSpec {
        &self.spec
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalReceipt<'a> {
    schema: &'static str,
    spec: &'a WorkloadWriterFenceReceiptSpec,
}

fn receipt_digest(spec: &WorkloadWriterFenceReceiptSpec) -> Result<Sha256Digest, String> {
    let bytes = canonical_json_bounded(
        &CanonicalReceipt {
            schema: WORKLOAD_WRITER_FENCE_RECEIPT_SCHEMA,
            spec,
        },
        MAX_RECEIPT_BYTES,
        "Workload writer-fence receipt",
    )?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

fn valid_runtime_unit_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RUNTIME_UNIT_ID_BYTES
        && !value.contains(['\0', '\r', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workloads::{ManagedOwnerKind, ManagedOwnerReference};
    use chrono::Duration;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn spec() -> WorkloadWriterFenceReceiptSpec {
        WorkloadWriterFenceReceiptSpec {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            workload_id: WorkloadId::new(),
            workload_revision_id: WorkloadRevisionId::new(),
            workload_revision_generation: 3,
            replica_id: WorkloadReplicaId::new(),
            replica_ordinal: 0,
            writer_epoch: 7,
            member_id: WorkloadReplicaMemberId::new(),
            placement_generation: 2,
            managed_owner: ManagedOwnerReference::new(
                ManagedOwnerKind::parse("durable-cell.application").expect("owner kind"),
                uuid::Uuid::now_v7(),
                3,
                digest('a').as_str(),
            )
            .expect("managed owner"),
            node_id: NodeId::new(),
            runtime_unit_id: "workload:fixture:revision:3".into(),
            command_id: NodeCommandId::new(),
            command_payload_digest: digest('b'),
            acknowledgement_digest: digest('c'),
            continuation_operation_id: OperationId::new(),
            fenced_at: Utc::now(),
        }
    }

    #[test]
    fn receipt_is_canonical_digest_bound_and_timestamp_normalized() {
        let receipt = WorkloadWriterFenceReceipt::issue(spec()).expect("receipt");
        receipt.validate().expect("valid receipt");
        let restored =
            WorkloadWriterFenceReceipt::restore(receipt.spec().clone(), receipt.digest().as_str())
                .expect("restored receipt");
        assert_eq!(restored, receipt);

        let mut drifted = receipt.spec().clone();
        drifted.writer_epoch += 1;
        assert!(WorkloadWriterFenceReceipt::restore(drifted, receipt.digest().as_str()).is_err());
    }

    #[test]
    fn receipt_rejects_unbounded_runtime_or_noncanonical_evidence() {
        let mut invalid = spec();
        invalid.runtime_unit_id = "x".repeat(MAX_RUNTIME_UNIT_ID_BYTES + 1);
        assert!(WorkloadWriterFenceReceipt::issue(invalid).is_err());

        let mut noncanonical = spec();
        noncanonical.fenced_at += Duration::nanoseconds(1);
        let receipt = WorkloadWriterFenceReceipt::issue(noncanonical).expect("normalized receipt");
        assert_eq!(receipt.spec().fenced_at.timestamp_subsec_nanos() % 1_000, 0);
    }
}
