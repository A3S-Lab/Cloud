use crate::modules::shared_kernel::domain::{canonical_timestamp, WorkloadId, WorkloadRevisionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
        let expected_unit_id = format!(
            "workload:{workload_id}:revision:{}",
            self.workload_revision_id
        );
        if self.runtime_generation == 0
            || self.runtime_unit_id != expected_unit_id
            || self.observed_at != canonical_timestamp(self.observed_at)
        {
            return Err("route target is not bound to one canonical Runtime generation".into());
        }
        if RoutePortName::parse(self.port_name.as_str())? != self.port_name
            || UpstreamEndpoint::parse(self.upstream.as_str())? != self.upstream
        {
            return Err("route target contains a non-canonical node-local endpoint".into());
        }
        Ok(())
    }
}
