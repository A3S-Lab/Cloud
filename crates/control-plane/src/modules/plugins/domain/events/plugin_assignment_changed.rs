use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, PluginAssignmentId, PluginRegistryId, PrincipalId,
    ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_use_core::PluginDesiredState;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginAssignmentChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub assignment_id: PluginAssignmentId,
    pub registry_id: PluginRegistryId,
    pub target_host_id: NodeId,
    pub package_id: String,
    pub catalog_record_digest: String,
    pub desired_state: PluginDesiredState,
    pub assignment_generation: u64,
    pub actor_id: PrincipalId,
}

impl PluginAssignmentChanged {
    pub fn envelope(assignment: &PluginAssignment) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "plugin.assignment.changed".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Environment {
                organization_id: assignment.organization_id.as_uuid(),
                project_id: assignment.project_id.as_uuid(),
                environment_id: assignment.environment_id.as_uuid(),
            },
            aggregate_id: assignment.id.as_uuid(),
            aggregate_version: assignment.aggregate_version,
            occurred_at: assignment.updated_at,
            correlation_id: assignment.last_request_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: assignment.organization_id,
                project_id: assignment.project_id,
                environment_id: assignment.environment_id,
                assignment_id: assignment.id,
                registry_id: assignment.registry_id,
                target_host_id: assignment.target_host_id,
                package_id: assignment.package_id().as_str().to_owned(),
                catalog_record_digest: assignment.selection.catalog_record_digest.as_str().to_owned(),
                desired_state: assignment.desired_state,
                assignment_generation: assignment.assignment_generation,
                actor_id: assignment.last_actor_id,
            })?,
        })
    }
}
