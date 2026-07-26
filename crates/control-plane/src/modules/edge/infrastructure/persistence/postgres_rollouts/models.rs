use super::super::postgres_schema::{
    GatewayRolloutReplicas, GatewayRolloutRollbacks, GatewayRollouts,
};
use crate::modules::edge::domain::{
    GatewayReplicaRollout, GatewayReplicaRolloutState, GatewayRollout, GatewayRolloutPolicy,
    GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState,
};
use crate::modules::shared_kernel::domain::{
    GatewayCertificateId, GatewayRolloutId, GatewayScopeId, NodeCommandId, NodeId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{DecodeError, Expression, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RolloutRow {
    pub(super) id: Uuid,
    pub(super) organization_id: Uuid,
    pub(super) gateway_scope_id: Uuid,
    pub(super) membership_generation: u64,
    pub(super) generation: u64,
    pub(super) correlation_id: Uuid,
    pub(super) min_ready: u32,
    pub(super) max_unavailable: u32,
    pub(super) desired_replicas: u32,
    pub(super) state: String,
    pub(super) ready_replicas: u32,
    pub(super) unavailable_replicas: u32,
    pub(super) aggregate_version: u64,
    pub(super) started_at: DateTime<Utc>,
    pub(super) completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RollbackRow {
    pub(super) failed_rollout_id: Uuid,
    pub(super) gateway_scope_id: Uuid,
    pub(super) membership_generation: u64,
    pub(super) failed_generation: u64,
    pub(super) rollback_rollout_id: Uuid,
    pub(super) rollback_generation: u64,
    pub(super) state: String,
    pub(super) aggregate_version: u64,
    pub(super) required_at: DateTime<Utc>,
    pub(super) staged_at: Option<DateTime<Utc>>,
    pub(super) completed_at: Option<DateTime<Utc>>,
    pub(super) failure: Option<String>,
}

impl FromRow for RollbackRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            failed_rollout_id: decode(row, 0)?,
            gateway_scope_id: decode(row, 1)?,
            membership_generation: decode(row, 2)?,
            failed_generation: decode(row, 3)?,
            rollback_rollout_id: decode(row, 4)?,
            rollback_generation: decode(row, 5)?,
            state: decode(row, 6)?,
            aggregate_version: decode(row, 7)?,
            required_at: decode(row, 8)?,
            staged_at: decode(row, 9)?,
            completed_at: decode(row, 10)?,
            failure: decode(row, 11)?,
        })
    }
}

impl RollbackRow {
    pub(super) fn rollback(self) -> Result<GatewayRolloutRollback, RepositoryError> {
        let rollback = GatewayRolloutRollback {
            failed_rollout_id: GatewayRolloutId::from_uuid(self.failed_rollout_id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            membership_generation: self.membership_generation,
            failed_generation: self.failed_generation,
            rollback_rollout_id: GatewayRolloutId::from_uuid(self.rollback_rollout_id),
            rollback_generation: self.rollback_generation,
            state: GatewayRolloutRollbackState::parse(&self.state)
                .map_err(RepositoryError::Storage)?,
            aggregate_version: self.aggregate_version,
            required_at: self.required_at,
            staged_at: self.staged_at,
            completed_at: self.completed_at,
            failure: self.failure,
        };
        rollback.validate().map_err(RepositoryError::Storage)?;
        Ok(rollback)
    }
}

impl FromRow for RolloutRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Self::from_row_at(row, 0)
    }
}

impl RolloutRow {
    pub(super) fn from_row_at(row: &impl Row, offset: usize) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, offset)?,
            organization_id: decode(row, offset + 1)?,
            gateway_scope_id: decode(row, offset + 2)?,
            membership_generation: decode(row, offset + 3)?,
            generation: decode(row, offset + 4)?,
            correlation_id: decode(row, offset + 5)?,
            min_ready: decode(row, offset + 6)?,
            max_unavailable: decode(row, offset + 7)?,
            desired_replicas: decode(row, offset + 8)?,
            state: decode(row, offset + 9)?,
            ready_replicas: decode(row, offset + 10)?,
            unavailable_replicas: decode(row, offset + 11)?,
            aggregate_version: decode(row, offset + 12)?,
            started_at: decode(row, offset + 13)?,
            completed_at: decode(row, offset + 14)?,
        })
    }

    pub(super) fn rollout(
        self,
        mut replicas: Vec<GatewayReplicaRollout>,
    ) -> Result<GatewayRollout, RepositoryError> {
        replicas.sort_by_key(|replica| replica.node_id);
        if usize::try_from(self.desired_replicas).ok() != Some(replicas.len()) {
            return Err(RepositoryError::Storage(
                "stored Gateway rollout desired replica count is inconsistent".into(),
            ));
        }
        let rollout = GatewayRollout {
            id: GatewayRolloutId::from_uuid(self.id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            membership_generation: self.membership_generation,
            generation: self.generation,
            correlation_id: self.correlation_id,
            policy: GatewayRolloutPolicy {
                min_ready: self.min_ready,
                max_unavailable: self.max_unavailable,
            },
            replicas,
            state: GatewayRolloutState::parse(&self.state).map_err(RepositoryError::Storage)?,
            ready_replicas: self.ready_replicas,
            unavailable_replicas: self.unavailable_replicas,
            aggregate_version: self.aggregate_version,
            started_at: self.started_at,
            completed_at: self.completed_at,
        };
        rollout.validate().map_err(RepositoryError::Storage)?;
        Ok(rollout)
    }
}

#[derive(Debug)]
pub(super) struct ReplicaRow {
    pub(super) node_id: Uuid,
    pub(super) revision: u64,
    pub(super) command_id: Uuid,
    pub(super) snapshot_digest: String,
    pub(super) snapshot_expires_at: DateTime<Utc>,
    pub(super) gateway_certificate_id: Option<Uuid>,
    pub(super) state: String,
    pub(super) failure: Option<String>,
    pub(super) acknowledged_at: Option<DateTime<Utc>>,
    pub(super) recovery: Option<serde_json::Value>,
}

pub(super) struct RolloutSelection;

impl Selection for RolloutSelection {
    type Output = RolloutRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRollouts::id().expression(),
            GatewayRollouts::organization_id().expression(),
            GatewayRollouts::gateway_scope_id().expression(),
            GatewayRollouts::membership_generation().expression(),
            GatewayRollouts::generation().expression(),
            GatewayRollouts::correlation_id().expression(),
            GatewayRollouts::min_ready().expression(),
            GatewayRollouts::max_unavailable().expression(),
            GatewayRollouts::desired_replicas().expression(),
            GatewayRollouts::state().expression(),
            GatewayRollouts::ready_replicas().expression(),
            GatewayRollouts::unavailable_replicas().expression(),
            GatewayRollouts::aggregate_version().expression(),
            GatewayRollouts::started_at().expression(),
            GatewayRollouts::completed_at().expression(),
        ]
    }
}

pub(super) struct RollbackSelection;

impl Selection for RollbackSelection {
    type Output = RollbackRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRolloutRollbacks::failed_rollout_id().expression(),
            GatewayRolloutRollbacks::gateway_scope_id().expression(),
            GatewayRolloutRollbacks::membership_generation().expression(),
            GatewayRolloutRollbacks::failed_generation().expression(),
            GatewayRolloutRollbacks::rollback_rollout_id().expression(),
            GatewayRolloutRollbacks::rollback_generation().expression(),
            GatewayRolloutRollbacks::state().expression(),
            GatewayRolloutRollbacks::aggregate_version().expression(),
            GatewayRolloutRollbacks::required_at().expression(),
            GatewayRolloutRollbacks::staged_at().expression(),
            GatewayRolloutRollbacks::completed_at().expression(),
            GatewayRolloutRollbacks::failure().expression(),
        ]
    }
}

pub(super) struct ReplicaSelection;

impl Selection for ReplicaSelection {
    type Output = ReplicaRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRolloutReplicas::node_id().expression(),
            GatewayRolloutReplicas::revision().expression(),
            GatewayRolloutReplicas::command_id().expression(),
            GatewayRolloutReplicas::snapshot_digest().expression(),
            GatewayRolloutReplicas::snapshot_expires_at().expression(),
            GatewayRolloutReplicas::gateway_certificate_id().expression(),
            GatewayRolloutReplicas::state().expression(),
            GatewayRolloutReplicas::failure().expression(),
            GatewayRolloutReplicas::acknowledged_at().expression(),
            GatewayRolloutReplicas::recovery().expression(),
        ]
    }
}

pub(super) struct RolloutReplicaSelection;

impl Selection for RolloutReplicaSelection {
    type Output = RolloutReplicaRow;

    fn expressions(self) -> Vec<Expression> {
        let mut expressions = RolloutSelection.expressions();
        expressions.extend(ReplicaSelection.expressions());
        expressions
    }
}

#[derive(Debug)]
pub(super) struct RolloutReplicaRow {
    pub(super) rollout: RolloutRow,
    pub(super) replica: ReplicaRow,
}

impl FromRow for RolloutReplicaRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            rollout: RolloutRow::from_row_at(row, 0)?,
            replica: ReplicaRow::from_row_at(row, 15)?,
        })
    }
}

impl FromRow for ReplicaRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Self::from_row_at(row, 0)
    }
}

impl ReplicaRow {
    pub(super) fn from_row_at(row: &impl Row, offset: usize) -> Result<Self, DecodeError> {
        Ok(Self {
            node_id: decode(row, offset)?,
            revision: decode(row, offset + 1)?,
            command_id: decode(row, offset + 2)?,
            snapshot_digest: decode(row, offset + 3)?,
            snapshot_expires_at: decode(row, offset + 4)?,
            gateway_certificate_id: decode(row, offset + 5)?,
            state: decode(row, offset + 6)?,
            failure: decode(row, offset + 7)?,
            acknowledged_at: decode(row, offset + 8)?,
            recovery: decode(row, offset + 9)?,
        })
    }

    pub(super) fn replica(self) -> Result<GatewayReplicaRollout, RepositoryError> {
        Ok(GatewayReplicaRollout {
            node_id: NodeId::from_uuid(self.node_id),
            revision: self.revision,
            command_id: NodeCommandId::from_uuid(self.command_id),
            snapshot_digest: self.snapshot_digest,
            snapshot_expires_at: self.snapshot_expires_at,
            gateway_certificate_id: self
                .gateway_certificate_id
                .map(GatewayCertificateId::from_uuid),
            state: GatewayReplicaRolloutState::parse(&self.state)
                .map_err(RepositoryError::Storage)?,
            failure: self.failure,
            acknowledged_at: self.acknowledged_at,
            recovery: self
                .recovery
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| {
                    RepositoryError::Storage(format!(
                        "stored Gateway replica recovery is invalid: {error}"
                    ))
                })?,
        })
    }
}

pub(super) fn rebuild_rollout(
    mut rows: impl Iterator<Item = RolloutReplicaRow>,
) -> Result<GatewayRollout, RepositoryError> {
    let first = rows.next().ok_or(RepositoryError::NotFound)?;
    let rollout = first.rollout;
    let mut replicas = vec![first.replica.replica()?];
    for row in rows {
        if row.rollout != rollout {
            return Err(RepositoryError::Storage(
                "joined Gateway rollout rows contain inconsistent aggregate data".into(),
            ));
        }
        replicas.push(row.replica.replica()?);
    }
    rollout.rollout(replicas)
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
