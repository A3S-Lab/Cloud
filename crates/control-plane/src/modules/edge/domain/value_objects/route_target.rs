use crate::modules::shared_kernel::domain::{canonical_timestamp, WorkloadId, WorkloadRevisionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{RoutePortName, UpstreamEndpoint};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteTarget {
    pub workload_revision_id: WorkloadRevisionId,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub port_name: RoutePortName,
    pub upstream: UpstreamEndpoint,
    pub observed_at: DateTime<Utc>,
}

impl RouteTarget {
    pub fn new(
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        runtime_unit_id: String,
        runtime_generation: u64,
        port_name: RoutePortName,
        upstream: UpstreamEndpoint,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let target = Self {
            workload_revision_id,
            runtime_unit_id,
            runtime_generation,
            port_name,
            upstream,
            observed_at: canonical_timestamp(observed_at),
        };
        target.validate_for(workload_id)?;
        Ok(target)
    }

    pub fn validate_for(&self, workload_id: WorkloadId) -> Result<(), String> {
        if self.runtime_generation == 0
            || !runtime_unit_matches(
                workload_id,
                self.workload_revision_id,
                &self.runtime_unit_id,
            )
            || self.observed_at != canonical_timestamp(self.observed_at)
        {
            return Err("route target is not bound to one exact Runtime generation".into());
        }
        if RoutePortName::parse(self.port_name.as_str())? != self.port_name
            || UpstreamEndpoint::parse(self.upstream.as_str())? != self.upstream
        {
            return Err("route target contains a non-canonical node-local endpoint".into());
        }
        Ok(())
    }

    pub fn has_canonical_runtime_identity(&self, workload_id: WorkloadId) -> bool {
        self.runtime_unit_id
            == format!(
                "workload:{workload_id}:revision:{}",
                self.workload_revision_id
            )
    }
}

fn runtime_unit_matches(
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    runtime_unit_id: &str,
) -> bool {
    if runtime_unit_id == format!("workload:{workload_id}:revision:{revision_id}") {
        return true;
    }
    let prefix = format!("workload:{workload_id}:replica:");
    let suffix = format!(":revision:{revision_id}");
    runtime_unit_id
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .and_then(|value| Uuid::parse_str(value).ok())
        .is_some_and(|replica_id| !replica_id.is_nil() && replica_id != workload_id.as_uuid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_target_accepts_canonical_and_replica_runtime_identities() {
        let workload_id = WorkloadId::new();
        let revision_id = WorkloadRevisionId::new();
        let replica_id = Uuid::now_v7();
        assert!(runtime_unit_matches(
            workload_id,
            revision_id,
            &format!("workload:{workload_id}:revision:{revision_id}"),
        ));
        assert!(runtime_unit_matches(
            workload_id,
            revision_id,
            &format!("workload:{workload_id}:replica:{replica_id}:revision:{revision_id}"),
        ));
        assert!(!runtime_unit_matches(
            workload_id,
            revision_id,
            &format!(
                "workload:{workload_id}:replica:{}:revision:{revision_id}",
                workload_id.as_uuid()
            ),
        ));
        assert!(!runtime_unit_matches(
            workload_id,
            revision_id,
            &format!("workload:{workload_id}:replica:not-a-uuid:revision:{revision_id}"),
        ));
    }
}
