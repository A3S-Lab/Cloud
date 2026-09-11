use crate::modules::shared_kernel::domain::{
    NodeCommandId, NodeId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use a3s_runtime::contract::RuntimeUnitSpec;
use serde::{Deserialize, Serialize};

pub const WORKLOAD_HEALTHY_ROUTE_TARGET_CANDIDATE_SET_SCHEMA: &str =
    "a3s.cloud.workload-healthy-route-target-candidate-set.v1";

/// One Workloads-owned candidate that Edge may observe for healthy route targeting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadHealthyRouteTargetCandidate {
    node_id: NodeId,
    command_id: NodeCommandId,
    runtime_unit_id: String,
    runtime_generation: u64,
    runtime_spec: RuntimeUnitSpec,
}

/// Workloads-owned set of active revision deployments eligible for healthy route targeting.
///
/// Placement aggregates, Deployment lifecycle rows, and replica members remain
/// inside Workloads. Consumers receive only the Runtime binding slice required to
/// observe health and build Edge-owned route targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadHealthyRouteTargetCandidateSet {
    schema: String,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    port_name: String,
    candidates: Vec<WorkloadHealthyRouteTargetCandidate>,
}

pub(in crate::modules::workloads) struct ValidatedWorkloadHealthyRouteTargetCandidate {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub runtime_spec: RuntimeUnitSpec,
}

pub(in crate::modules::workloads) struct ValidatedWorkloadHealthyRouteTargetCandidateSet {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub port_name: String,
    pub candidates: Vec<ValidatedWorkloadHealthyRouteTargetCandidate>,
}

impl WorkloadHealthyRouteTargetCandidate {
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn command_id(&self) -> NodeCommandId {
        self.command_id
    }

    pub fn runtime_unit_id(&self) -> &str {
        &self.runtime_unit_id
    }

    pub const fn runtime_generation(&self) -> u64 {
        self.runtime_generation
    }

    pub fn runtime_spec(&self) -> &RuntimeUnitSpec {
        &self.runtime_spec
    }
}

impl WorkloadHealthyRouteTargetCandidateSet {
    pub(in crate::modules::workloads) fn from_validated(
        projection: ValidatedWorkloadHealthyRouteTargetCandidateSet,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKLOAD_HEALTHY_ROUTE_TARGET_CANDIDATE_SET_SCHEMA.into(),
            organization_id: projection.organization_id,
            workload_id: projection.workload_id,
            revision_id: projection.revision_id,
            port_name: projection.port_name,
            candidates: projection
                .candidates
                .into_iter()
                .map(|candidate| WorkloadHealthyRouteTargetCandidate {
                    node_id: candidate.node_id,
                    command_id: candidate.command_id,
                    runtime_unit_id: candidate.runtime_unit_id,
                    runtime_generation: candidate.runtime_generation,
                    runtime_spec: candidate.runtime_spec,
                })
                .collect(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKLOAD_HEALTHY_ROUTE_TARGET_CANDIDATE_SET_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.port_name.is_empty()
        {
            return Err("Workload healthy route-target candidate set is invalid".into());
        }
        for candidate in &self.candidates {
            candidate.runtime_spec.validate()?;
            if candidate.node_id.as_uuid().is_nil()
                || candidate.command_id.as_uuid().is_nil()
                || candidate.runtime_unit_id.is_empty()
                || candidate.runtime_generation == 0
            {
                return Err("Workload healthy route-target candidate is invalid".into());
            }
        }
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn revision_id(&self) -> WorkloadRevisionId {
        self.revision_id
    }

    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    pub fn candidates(&self) -> &[WorkloadHealthyRouteTargetCandidate] {
        &self.candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_set_rejects_empty_port_name() {
        assert!(WorkloadHealthyRouteTargetCandidateSet::from_validated(
            ValidatedWorkloadHealthyRouteTargetCandidateSet {
                organization_id: OrganizationId::new(),
                workload_id: WorkloadId::new(),
                revision_id: WorkloadRevisionId::new(),
                port_name: String::new(),
                candidates: Vec::new(),
            },
        )
        .is_err());

        let set = WorkloadHealthyRouteTargetCandidateSet::from_validated(
            ValidatedWorkloadHealthyRouteTargetCandidateSet {
                organization_id: OrganizationId::new(),
                workload_id: WorkloadId::new(),
                revision_id: WorkloadRevisionId::new(),
                port_name: "mcp".into(),
                candidates: Vec::new(),
            },
        )
        .expect("valid empty candidate set");
        assert_eq!(
            set.schema(),
            WORKLOAD_HEALTHY_ROUTE_TARGET_CANDIDATE_SET_SCHEMA
        );
        assert!(set.candidates().is_empty());
    }
}
