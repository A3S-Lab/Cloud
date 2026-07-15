use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, ResourceName, WorkloadId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadDesiredState {
    Running,
    Stopped,
}

impl WorkloadDesiredState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            _ => Err(format!("unsupported workload desired state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workload {
    pub id: WorkloadId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: ResourceName,
    pub desired_state: WorkloadDesiredState,
    pub active_revision_id: Option<WorkloadRevisionId>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Workload {
    pub fn create(
        id: WorkloadId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        name: ResourceName,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            organization_id,
            project_id,
            environment_id,
            name,
            desired_state: WorkloadDesiredState::Running,
            active_revision_id: None,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
        }
    }

    pub fn activate(
        &mut self,
        revision_id: WorkloadRevisionId,
        activated_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.desired_state != WorkloadDesiredState::Running {
            return Err("stopped workload cannot activate a revision".into());
        }
        if activated_at < self.updated_at {
            return Err("workload activation time regressed".into());
        }
        if self.active_revision_id != Some(revision_id) {
            self.active_revision_id = Some(revision_id);
            self.aggregate_version += 1;
        }
        self.updated_at = activated_at;
        Ok(())
    }

    pub fn request_stop(&mut self, requested_at: DateTime<Utc>) -> Result<(), String> {
        if requested_at < self.updated_at {
            return Err("workload stop request time regressed".into());
        }
        if self.desired_state == WorkloadDesiredState::Stopped {
            return Ok(());
        }
        self.desired_state = WorkloadDesiredState::Stopped;
        self.aggregate_version += 1;
        self.updated_at = requested_at;
        Ok(())
    }

    pub fn complete_stop(&mut self, stopped_at: DateTime<Utc>) -> Result<(), String> {
        if self.desired_state != WorkloadDesiredState::Stopped {
            return Err("running workload cannot complete a stop".into());
        }
        if stopped_at < self.updated_at {
            return Err("workload stop completion time regressed".into());
        }
        if self.active_revision_id.is_none() {
            return Ok(());
        }
        self.active_revision_id = None;
        self.aggregate_version += 1;
        self.updated_at = stopped_at;
        Ok(())
    }
}
