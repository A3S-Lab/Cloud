use crate::modules::edge::domain::repositories::EdgeRoutePublicationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, WorkloadRevisionId,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PublishRoute {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_revision_id: WorkloadRevisionId,
    pub hostname: String,
    pub path_prefix: String,
    pub port_name: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for PublishRoute {
    type Output = ApplicationResult<PublishRouteResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublishRouteResult {
    pub publication: EdgeRoutePublicationResult,
    pub command_replayed: bool,
}
