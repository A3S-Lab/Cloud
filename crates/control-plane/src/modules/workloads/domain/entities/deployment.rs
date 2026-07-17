use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, NodeCommandId, NodeId, OperationId, OrganizationId,
    WorkloadId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Queued,
    Resolving,
    Scheduled,
    Applying,
    Verifying,
    Cancelling,
    CleanupPending,
    Active,
    Failed,
    Orphaned,
    Cancelled,
}

impl DeploymentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Scheduled => "scheduled",
            Self::Applying => "applying",
            Self::Verifying => "verifying",
            Self::Cancelling => "cancelling",
            Self::CleanupPending => "cleanup_pending",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Orphaned => "orphaned",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "queued" => Ok(Self::Queued),
            "resolving" => Ok(Self::Resolving),
            "scheduled" => Ok(Self::Scheduled),
            "applying" => Ok(Self::Applying),
            "verifying" => Ok(Self::Verifying),
            "cancelling" => Ok(Self::Cancelling),
            "cleanup_pending" => Ok(Self::CleanupPending),
            "active" => Ok(Self::Active),
            "failed" => Ok(Self::Failed),
            "orphaned" => Ok(Self::Orphaned),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unsupported deployment status {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Failed | Self::Orphaned | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub id: DeploymentId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub operation_id: OperationId,
    pub node_id: Option<NodeId>,
    pub command_id: Option<NodeCommandId>,
    pub cleanup_command_id: Option<NodeCommandId>,
    pub status: DeploymentStatus,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl Deployment {
    pub fn create(
        id: DeploymentId,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        revision_id: WorkloadRevisionId,
        operation_id: OperationId,
        requested_at: DateTime<Utc>,
    ) -> Self {
        let requested_at = canonical_timestamp(requested_at);
        Self {
            id,
            organization_id,
            workload_id,
            revision_id,
            operation_id,
            node_id: None,
            command_id: None,
            cleanup_command_id: None,
            status: DeploymentStatus::Queued,
            failure: None,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            activated_at: None,
            cancellation_requested_at: None,
            cancelled_at: None,
        }
    }

    pub fn resolve(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        self.transition(DeploymentStatus::Queued, DeploymentStatus::Resolving, at)
    }

    pub fn schedule(&mut self, node_id: NodeId, at: DateTime<Utc>) -> Result<(), String> {
        if self.status == DeploymentStatus::Scheduled {
            return if self.node_id == Some(node_id) {
                self.canonical_time(at).map(|_| ())
            } else {
                Err("scheduled deployment cannot change node".into())
            };
        }
        self.transition(DeploymentStatus::Resolving, DeploymentStatus::Scheduled, at)?;
        self.node_id = Some(node_id);
        Ok(())
    }

    pub fn dispatch(&mut self, command_id: NodeCommandId, at: DateTime<Utc>) -> Result<(), String> {
        if self.status == DeploymentStatus::Applying {
            return if self.command_id == Some(command_id) {
                self.canonical_time(at).map(|_| ())
            } else {
                Err("dispatched deployment cannot change command".into())
            };
        }
        if self.node_id.is_none() {
            return Err("deployment cannot dispatch before scheduling".into());
        }
        self.transition(DeploymentStatus::Scheduled, DeploymentStatus::Applying, at)?;
        self.command_id = Some(command_id);
        Ok(())
    }

    pub fn verify(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        self.transition(DeploymentStatus::Applying, DeploymentStatus::Verifying, at)
    }

    pub fn activate(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.status == DeploymentStatus::Active {
            return Ok(());
        }
        self.transition(DeploymentStatus::Verifying, DeploymentStatus::Active, at)?;
        self.activated_at = Some(at);
        Ok(())
    }

    pub fn fail(&mut self, reason: String, at: DateTime<Utc>) -> Result<(), String> {
        if matches!(
            self.status,
            DeploymentStatus::Active | DeploymentStatus::Cancelled
        ) {
            return Err("terminal deployment cannot fail".into());
        }
        if reason.trim().is_empty()
            || reason.len() > 16 * 1024
            || reason.contains(['\0', '\r', '\n'])
        {
            return Err("deployment failure is invalid".into());
        }
        let at = self.canonical_time(at)?;
        if matches!(
            self.status,
            DeploymentStatus::Failed | DeploymentStatus::Orphaned
        ) {
            if self.failure.as_ref() == Some(&reason) {
                return Ok(());
            }
            return Err("failed deployment cannot change its failure reason".into());
        }
        self.status = if self.cleanup_command_id.is_some()
            || self.command_id.is_some()
                && matches!(
                    self.status,
                    DeploymentStatus::Cancelling | DeploymentStatus::CleanupPending
                ) {
            DeploymentStatus::Orphaned
        } else {
            DeploymentStatus::Failed
        };
        self.failure = Some(reason);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn cancel(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.status == DeploymentStatus::Cancelled {
            return Ok(());
        }
        if !matches!(
            self.status,
            DeploymentStatus::Cancelling | DeploymentStatus::CleanupPending
        ) {
            self.request_cancellation(at)?;
        }
        self.status = DeploymentStatus::Cancelled;
        self.cancelled_at = Some(at);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn request_cancellation(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if matches!(
            self.status,
            DeploymentStatus::Cancelling | DeploymentStatus::CleanupPending
        ) {
            return Ok(());
        }
        if self.status.is_terminal() {
            return Err("terminal deployment cannot request cancellation".into());
        }
        self.status = DeploymentStatus::Cancelling;
        self.cancellation_requested_at = Some(at);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn begin_cleanup(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.status == DeploymentStatus::CleanupPending {
            return if self.cleanup_command_id == Some(command_id) {
                Ok(())
            } else {
                Err("cleanup command cannot change without an explicit retry".into())
            };
        }
        if self.status != DeploymentStatus::Cancelling {
            return Err("deployment cleanup requires a cancellation request".into());
        }
        if self.node_id.is_none() || self.command_id.is_none() {
            return Err("deployment cleanup requires a dispatched Runtime child".into());
        }
        self.status = DeploymentStatus::CleanupPending;
        self.cleanup_command_id = Some(command_id);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn retry_cleanup(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.status != DeploymentStatus::CleanupPending || self.cleanup_command_id.is_none() {
            return Err("deployment has no pending cleanup to retry".into());
        }
        if self.cleanup_command_id != Some(command_id) {
            self.cleanup_command_id = Some(command_id);
            self.aggregate_version += 1;
        }
        self.updated_at = at;
        Ok(())
    }

    fn transition(
        &mut self,
        expected: DeploymentStatus,
        next: DeploymentStatus,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.status == next {
            return Ok(());
        }
        if self.status != expected {
            return Err(format!(
                "deployment cannot transition from {} to {}",
                self.status.as_str(),
                next.as_str()
            ));
        }
        self.status = next;
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    fn canonical_time(&self, at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let at = canonical_timestamp(at);
        if at < self.updated_at {
            return Err("deployment update time regressed".into());
        }
        Ok(at)
    }
}
