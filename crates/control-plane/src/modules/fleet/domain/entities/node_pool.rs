use crate::modules::shared_kernel::domain::{
    canonical_timestamp, sha256_digest, NodeId, NodePoolId, OrganizationId, ResourceName,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const MAX_NODE_POOL_MEMBERS: usize = 10_000;
pub const MAX_MAINTENANCE_TARGETS: usize = 10_000;
pub const MAX_MAINTENANCE_REASON_CHARS: usize = 1_024;
pub const MAX_MAINTENANCE_DURATION: Duration = Duration::days(30);
pub const MAX_MAINTENANCE_HORIZON: Duration = Duration::days(365);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePoolMaintenanceStatus {
    Scheduled,
    Active,
    Completed,
    Cancelled,
}

impl NodePoolMaintenanceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePoolMaintenanceWindow {
    pub generation: u64,
    pub target_node_ids: Vec<NodeId>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: String,
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl NodePoolMaintenanceWindow {
    pub fn status_at(&self, now: DateTime<Utc>) -> NodePoolMaintenanceStatus {
        if self.cancelled_at.is_some() {
            NodePoolMaintenanceStatus::Cancelled
        } else if now < self.starts_at {
            NodePoolMaintenanceStatus::Scheduled
        } else if now < self.ends_at {
            NodePoolMaintenanceStatus::Active
        } else {
            NodePoolMaintenanceStatus::Completed
        }
    }

    pub fn is_active_for(&self, node_id: NodeId, now: DateTime<Utc>) -> bool {
        self.status_at(now) == NodePoolMaintenanceStatus::Active
            && self.target_node_ids.binary_search(&node_id).is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePool {
    pub id: NodePoolId,
    pub organization_id: OrganizationId,
    pub name: ResourceName,
    pub member_node_ids: Vec<NodeId>,
    pub maintenance: Option<NodePoolMaintenanceWindow>,
    pub spec_digest: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodePoolSpec<'a> {
    name: &'a str,
    member_node_ids: &'a [NodeId],
    maintenance: &'a Option<NodePoolMaintenanceWindow>,
}

impl NodePool {
    pub fn create(
        id: NodePoolId,
        organization_id: OrganizationId,
        name: ResourceName,
        member_node_ids: Vec<NodeId>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let mut pool = Self {
            id,
            organization_id,
            name,
            member_node_ids: canonical_node_ids(member_node_ids, MAX_NODE_POOL_MEMBERS, "member")?,
            maintenance: None,
            spec_digest: String::new(),
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
        };
        pool.refresh_spec_digest()?;
        pool.validate()?;
        Ok(pool)
    }

    pub fn add_members(
        &mut self,
        member_node_ids: Vec<NodeId>,
        changed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let additions = canonical_node_ids(member_node_ids, MAX_NODE_POOL_MEMBERS, "member")?;
        let mut projected = self.member_node_ids.clone();
        projected.extend(additions);
        projected.sort_unstable();
        projected.dedup();
        if projected.len() > MAX_NODE_POOL_MEMBERS {
            return Err(format!(
                "node pool cannot contain more than {MAX_NODE_POOL_MEMBERS} members"
            ));
        }
        if projected == self.member_node_ids {
            return Ok(());
        }
        self.advance(changed_at)?;
        self.member_node_ids = projected;
        self.refresh_spec_digest()?;
        self.validate()
    }

    pub fn schedule_maintenance(
        &mut self,
        target_node_ids: Vec<NodeId>,
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        reason: impl Into<String>,
        requested_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let requested_at = canonical_timestamp(requested_at);
        let starts_at = canonical_timestamp(starts_at);
        let ends_at = canonical_timestamp(ends_at);
        if starts_at < requested_at {
            return Err("maintenance window cannot start before it is requested".into());
        }
        if starts_at > requested_at + MAX_MAINTENANCE_HORIZON {
            return Err("maintenance window starts beyond the supported horizon".into());
        }
        if ends_at <= starts_at || ends_at - starts_at > MAX_MAINTENANCE_DURATION {
            return Err(
                "maintenance window duration must be positive and no longer than 30 days".into(),
            );
        }
        let targets = canonical_node_ids(
            target_node_ids,
            MAX_MAINTENANCE_TARGETS,
            "maintenance target",
        )?;
        if targets
            .iter()
            .any(|node_id| self.member_node_ids.binary_search(node_id).is_err())
        {
            return Err("maintenance targets must be members of the node pool".into());
        }
        let reason = bounded_reason(reason.into())?;
        let generation = match &self.maintenance {
            Some(window) => window
                .generation
                .checked_add(1)
                .ok_or_else(|| "maintenance generation is exhausted".to_owned())?,
            None => 1,
        };
        self.advance(requested_at)?;
        self.maintenance = Some(NodePoolMaintenanceWindow {
            generation,
            target_node_ids: targets,
            starts_at,
            ends_at,
            reason,
            cancelled_at: None,
        });
        self.refresh_spec_digest()?;
        self.validate()
    }

    pub fn cancel_maintenance(
        &mut self,
        generation: u64,
        cancelled_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let window = self
            .maintenance
            .as_ref()
            .ok_or_else(|| "node pool has no maintenance window".to_owned())?;
        if generation == 0 || window.generation != generation {
            return Err("maintenance generation changed".into());
        }
        if window.cancelled_at.is_some() {
            return Ok(());
        }
        let cancelled_at = canonical_timestamp(cancelled_at);
        if cancelled_at >= window.ends_at {
            return Err("completed maintenance window cannot be cancelled".into());
        }
        self.advance(cancelled_at)?;
        self.maintenance
            .as_mut()
            .expect("maintenance was checked above")
            .cancelled_at = Some(cancelled_at);
        self.refresh_spec_digest()?;
        self.validate()
    }

    pub fn node_is_in_active_maintenance(&self, node_id: NodeId, now: DateTime<Utc>) -> bool {
        self.maintenance
            .as_ref()
            .is_some_and(|window| window.is_active_for(node_id, now))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || ResourceName::parse(self.name.as_str())? != self.name
        {
            return Err("node pool identity, name, version, or timestamps are invalid".into());
        }
        validate_canonical_node_ids(&self.member_node_ids, MAX_NODE_POOL_MEMBERS, "member")?;
        if let Some(window) = &self.maintenance {
            if window.generation == 0
                || window.starts_at != canonical_timestamp(window.starts_at)
                || window.ends_at != canonical_timestamp(window.ends_at)
                || window.ends_at <= window.starts_at
                || window.ends_at - window.starts_at > MAX_MAINTENANCE_DURATION
                || window
                    .cancelled_at
                    .is_some_and(|at| at != canonical_timestamp(at) || at >= window.ends_at)
            {
                return Err("node pool maintenance identity or timestamps are invalid".into());
            }
            validate_canonical_node_ids(
                &window.target_node_ids,
                MAX_MAINTENANCE_TARGETS,
                "maintenance target",
            )?;
            if window
                .target_node_ids
                .iter()
                .any(|node_id| self.member_node_ids.binary_search(node_id).is_err())
            {
                return Err("maintenance targets must be members of the node pool".into());
            }
            if bounded_reason(window.reason.clone())? != window.reason {
                return Err("maintenance reason is not canonical".into());
            }
        }
        if self.spec_digest != self.computed_spec_digest()? {
            return Err("node pool specification digest does not match its content".into());
        }
        Ok(())
    }

    pub fn computed_spec_digest(&self) -> Result<String, String> {
        serde_json::to_vec(&NodePoolSpec {
            name: self.name.as_str(),
            member_node_ids: &self.member_node_ids,
            maintenance: &self.maintenance,
        })
        .map(|bytes| sha256_digest(&bytes))
        .map_err(|error| format!("could not encode node pool specification: {error}"))
    }

    fn advance(&mut self, changed_at: DateTime<Utc>) -> Result<(), String> {
        let changed_at = canonical_timestamp(changed_at);
        if changed_at < self.updated_at {
            return Err("node pool change timestamp moved backwards".into());
        }
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "node pool aggregate version is exhausted".to_owned())?;
        self.updated_at = changed_at;
        Ok(())
    }

    fn refresh_spec_digest(&mut self) -> Result<(), String> {
        self.spec_digest = self.computed_spec_digest()?;
        Ok(())
    }
}

fn canonical_node_ids(
    mut node_ids: Vec<NodeId>,
    maximum: usize,
    kind: &str,
) -> Result<Vec<NodeId>, String> {
    node_ids.sort_unstable();
    node_ids.dedup();
    validate_canonical_node_ids(&node_ids, maximum, kind)?;
    Ok(node_ids)
}

fn validate_canonical_node_ids(
    node_ids: &[NodeId],
    maximum: usize,
    kind: &str,
) -> Result<(), String> {
    if node_ids.is_empty()
        || node_ids.len() > maximum
        || node_ids.iter().any(|node_id| node_id.as_uuid().is_nil())
        || node_ids.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(format!(
            "node pool {kind} IDs must be a non-empty canonical set of at most {maximum} nodes"
        ));
    }
    Ok(())
}

fn bounded_reason(reason: String) -> Result<String, String> {
    let reason = reason.trim();
    if reason.is_empty()
        || reason.chars().count() > MAX_MAINTENANCE_REASON_CHARS
        || reason.contains(['\0', '\r', '\n'])
    {
        return Err(format!(
            "maintenance reason must contain 1 to {MAX_MAINTENANCE_REASON_CHARS} visible characters"
        ));
    }
    Ok(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 12, hour, 0, 0)
            .single()
            .expect("time")
    }

    #[test]
    fn membership_is_canonical_additive_and_digest_bound() {
        let first = NodeId::new();
        let second = NodeId::new();
        let mut pool = NodePool::create(
            NodePoolId::new(),
            OrganizationId::new(),
            ResourceName::parse("GPU Pool").expect("name"),
            vec![second, first, second],
            at(1),
        )
        .expect("pool");
        assert_eq!(pool.member_node_ids.len(), 2);
        let previous_digest = pool.spec_digest.clone();
        let third = NodeId::new();
        pool.add_members(vec![third], at(2)).expect("add member");
        assert_eq!(pool.aggregate_version, 2);
        assert_ne!(pool.spec_digest, previous_digest);
        pool.validate().expect("valid pool");
    }

    #[test]
    fn maintenance_is_bounded_versioned_and_exactly_targeted() {
        let first = NodeId::new();
        let second = NodeId::new();
        let mut pool = NodePool::create(
            NodePoolId::new(),
            OrganizationId::new(),
            ResourceName::parse("workers").expect("name"),
            vec![first, second],
            at(1),
        )
        .expect("pool");
        pool.schedule_maintenance(vec![first], at(3), at(5), "kernel upgrade", at(2))
            .expect("schedule");
        let window = pool.maintenance.as_ref().expect("window");
        assert_eq!(window.generation, 1);
        assert_eq!(
            window.status_at(at(2)),
            NodePoolMaintenanceStatus::Scheduled
        );
        assert!(pool.node_is_in_active_maintenance(first, at(4)));
        assert!(!pool.node_is_in_active_maintenance(second, at(4)));
        pool.cancel_maintenance(1, at(4)).expect("cancel");
        assert_eq!(
            pool.maintenance.as_ref().expect("window").status_at(at(4)),
            NodePoolMaintenanceStatus::Cancelled
        );
        assert_eq!(pool.aggregate_version, 3);
    }

    #[test]
    fn maintenance_rejects_non_members_and_unbounded_windows() {
        let member = NodeId::new();
        let mut pool = NodePool::create(
            NodePoolId::new(),
            OrganizationId::new(),
            ResourceName::parse("workers").expect("name"),
            vec![member],
            at(1),
        )
        .expect("pool");
        assert!(pool
            .schedule_maintenance(vec![NodeId::new()], at(3), at(4), "upgrade", at(2))
            .is_err());
        assert!(pool
            .schedule_maintenance(
                vec![member],
                at(3),
                at(3) + Duration::days(31),
                "upgrade",
                at(2),
            )
            .is_err());
    }
}
