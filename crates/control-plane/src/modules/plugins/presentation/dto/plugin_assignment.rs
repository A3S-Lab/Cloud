use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
use a3s_use_core::{PluginDesiredState, PluginManagedScope, PluginSurfaceRef};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAssignmentResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub id: Uuid,
    pub registry_id: Uuid,
    pub target_host_id: Uuid,
    pub workspace_scope: PluginManagedScope,
    pub package_id: String,
    pub catalog_record_digest: String,
    pub version: String,
    pub package_digest: String,
    pub manifest_digest: String,
    pub selected_surfaces: Vec<PluginSurfaceRef>,
    pub policy_digest: String,
    pub desired_state: PluginDesiredState,
    pub assignment_generation: u64,
    pub aggregate_version: u64,
    pub current_operation_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PluginAssignment> for PluginAssignmentResponse {
    fn from(value: PluginAssignment) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            environment_id: value.environment_id.as_uuid(),
            id: value.id.as_uuid(),
            registry_id: value.registry_id.as_uuid(),
            target_host_id: value.target_host_id.as_uuid(),
            package_id: value.package_id().as_str().to_owned(),
            catalog_record_digest: value.selection.catalog_record_digest.as_str().to_owned(),
            version: value.selection.version.clone(),
            package_digest: value.selection.package_digest.as_str().to_owned(),
            manifest_digest: value.selection.manifest_digest.as_str().to_owned(),
            selected_surfaces: value.selection.selected_surfaces.clone(),
            policy_digest: value.policy_digest.as_str().to_owned(),
            desired_state: value.desired_state,
            assignment_generation: value.assignment_generation,
            aggregate_version: value.aggregate_version,
            current_operation_id: value.current_operation_id.map(|id| id.as_uuid()),
            created_at: value.created_at,
            updated_at: value.updated_at,
            workspace_scope: value.workspace_scope,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAssignmentMutationResponse {
    pub assignment: PluginAssignmentResponse,
    pub replayed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetPluginAssignmentRequest {
    pub registry_id: Uuid,
    pub target_host_id: Uuid,
    pub workspace_scope: PluginManagedScope,
    pub selection: PluginCatalogSelection,
    pub policy_digest: String,
    pub desired_state: PluginDesiredState,
}
