use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, PluginRegistryId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_boot::Command;
use a3s_use_core::{PluginDesiredState, PluginManagedScope};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone)]
pub struct SetPluginAssignment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub registry_id: PluginRegistryId,
    pub target_host_id: NodeId,
    pub workspace_scope: PluginManagedScope,
    pub selection: PluginCatalogSelection,
    pub policy_digest: Sha256Digest,
    pub desired_state: PluginDesiredState,
    pub actor_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for SetPluginAssignment {
    type Output = ApplicationResult<SetPluginAssignmentResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetPluginAssignmentResult {
    pub assignment: PluginAssignment,
    pub replayed: bool,
}
