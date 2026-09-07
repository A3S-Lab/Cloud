use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::events::PluginAssignmentChanged;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, NodeId, OrganizationId,
    PluginAssignmentId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_use_core::PluginPackageId;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct CreatePluginAssignmentWrite {
    pub assignment: PluginAssignment,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct UpdatePluginAssignmentWrite {
    pub assignment: PluginAssignment,
    pub expected_aggregate_version: u64,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl CreatePluginAssignmentWrite {
    pub fn idempotency_for(
        assignment: &PluginAssignment,
        key: impl Into<String>,
    ) -> Result<IdempotencyRequest, String> {
        assignment_idempotency(assignment, key)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.idempotency.validate()?;
        let assignment = &self.assignment;
        let event = &self.event;
        let expected_idempotency = Self::idempotency_for(assignment, self.idempotency.key.clone())?;
        if assignment.assignment_generation != 1
            || assignment.aggregate_version != 1
            || assignment.created_at != assignment.updated_at
            || assignment.current_operation_id.is_none()
            || event.event_id.is_nil()
            || event.event_key != "plugin.assignment.changed"
            || event.schema_version != 1
            || event.organization_id() != Some(assignment.organization_id.as_uuid())
            || event.scope.project_id() != Some(assignment.project_id.as_uuid())
            || event.scope.environment_id() != Some(assignment.environment_id.as_uuid())
            || event.aggregate_id != assignment.id.as_uuid()
            || event.aggregate_version != assignment.aggregate_version
            || event.occurred_at != assignment.created_at
            || event.correlation_id != assignment.last_request_id
            || event.causation_id.is_some_and(|id| id.is_nil())
            || self.idempotency != expected_idempotency
        {
            return Err("plugin assignment write evidence is inconsistent".into());
        }
        validate_event_payload(assignment, event)?;
        Ok(())
    }

    pub(crate) fn validate_replay(
        requested: &PluginAssignment,
        replayed: &PluginAssignment,
    ) -> Result<(), RepositoryError> {
        replayed.validate().map_err(RepositoryError::Storage)?;
        if replayed.last_actor_id != requested.last_actor_id {
            return Err(RepositoryError::IdempotencyConflict);
        }
        if replayed.organization_id != requested.organization_id
            || replayed.project_id != requested.project_id
            || replayed.environment_id != requested.environment_id
            || replayed.registry_id != requested.registry_id
            || replayed.target_host_id != requested.target_host_id
            || replayed.workspace_scope != requested.workspace_scope
            || replayed.selection != requested.selection
            || replayed.policy_digest != requested.policy_digest
            || replayed.desired_state != requested.desired_state
            || replayed.assignment_generation != 1
            || replayed.aggregate_version != 1
            || replayed.created_at != replayed.updated_at
        {
            return Err(RepositoryError::Storage(
                "plugin assignment idempotency replay is inconsistent".into(),
            ));
        }
        Ok(())
    }
}

impl UpdatePluginAssignmentWrite {
    pub fn idempotency_for(
        assignment: &PluginAssignment,
        key: impl Into<String>,
    ) -> Result<IdempotencyRequest, String> {
        assignment_idempotency(assignment, key)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.idempotency.validate()?;
        let assignment = &self.assignment;
        let event = &self.event;
        let expected_idempotency = Self::idempotency_for(assignment, self.idempotency.key.clone())?;
        if assignment.assignment_generation < 2
            || assignment.aggregate_version < 2
            || self.expected_aggregate_version.checked_add(1) != Some(assignment.aggregate_version)
            || assignment.current_operation_id.is_none()
            || assignment.updated_at < assignment.created_at
            || event.event_id.is_nil()
            || event.event_key != "plugin.assignment.changed"
            || event.schema_version != 1
            || event.organization_id() != Some(assignment.organization_id.as_uuid())
            || event.scope.project_id() != Some(assignment.project_id.as_uuid())
            || event.scope.environment_id() != Some(assignment.environment_id.as_uuid())
            || event.aggregate_id != assignment.id.as_uuid()
            || event.aggregate_version != assignment.aggregate_version
            || event.occurred_at != assignment.updated_at
            || event.correlation_id != assignment.last_request_id
            || event.causation_id.is_some_and(|id| id.is_nil())
            || self.idempotency != expected_idempotency
        {
            return Err("plugin assignment update evidence is inconsistent".into());
        }
        validate_event_payload(assignment, event)?;
        Ok(())
    }

    pub(crate) fn validate_replay(
        requested: &PluginAssignment,
        replayed: &PluginAssignment,
    ) -> Result<(), RepositoryError> {
        replayed.validate().map_err(RepositoryError::Storage)?;
        if replayed.last_actor_id != requested.last_actor_id {
            return Err(RepositoryError::IdempotencyConflict);
        }
        if replayed.organization_id != requested.organization_id
            || replayed.project_id != requested.project_id
            || replayed.environment_id != requested.environment_id
            || replayed.id != requested.id
            || replayed.registry_id != requested.registry_id
            || replayed.target_host_id != requested.target_host_id
            || replayed.workspace_scope != requested.workspace_scope
            || replayed.selection != requested.selection
            || replayed.policy_digest != requested.policy_digest
            || replayed.desired_state != requested.desired_state
            || replayed.assignment_generation != requested.assignment_generation
            || replayed.aggregate_version != requested.aggregate_version
        {
            return Err(RepositoryError::Storage(
                "plugin assignment update idempotency replay is inconsistent".into(),
            ));
        }
        Ok(())
    }
}

fn assignment_idempotency(
    assignment: &PluginAssignment,
    key: impl Into<String>,
) -> Result<IdempotencyRequest, String> {
    assignment.validate()?;
    let surfaces: Vec<_> = assignment
        .selection
        .selected_surfaces
        .iter()
        .map(|surface| {
            serde_json::json!({
                "kind": surface.kind,
                "id": surface.id,
            })
        })
        .collect();
    let canonical_request = serde_json::to_vec(&serde_json::json!({
        "organizationId": assignment.organization_id,
        "projectId": assignment.project_id,
        "environmentId": assignment.environment_id,
        "registryId": assignment.registry_id,
        "targetHostId": assignment.target_host_id,
        "workspaceScope": assignment.workspace_scope,
        "packageId": assignment.package_id().as_str(),
        "catalogRecordDigest": assignment.selection.catalog_record_digest.as_str(),
        "version": assignment.selection.version,
        "packageDigest": assignment.selection.package_digest.as_str(),
        "manifestDigest": assignment.selection.manifest_digest.as_str(),
        "selectedSurfaces": surfaces,
        "policyDigest": assignment.policy_digest.as_str(),
        "desiredState": assignment.desired_state,
        "assignmentGeneration": assignment.assignment_generation,
    }))
    .map_err(|error| format!("serialize plugin assignment: {error}"))?;
    IdempotencyRequest::new(
        format!(
            "organizations/{}/environments/{}/plugin-assignments",
            assignment.organization_id, assignment.environment_id
        ),
        key,
        &canonical_request,
    )
}

fn validate_event_payload(
    assignment: &PluginAssignment,
    event: &DomainEventEnvelope,
) -> Result<(), String> {
    let payload: PluginAssignmentChanged = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("plugin assignment event payload is invalid: {error}"))?;
    if payload.organization_id != assignment.organization_id
        || payload.project_id != assignment.project_id
        || payload.environment_id != assignment.environment_id
        || payload.assignment_id != assignment.id
        || payload.registry_id != assignment.registry_id
        || payload.target_host_id != assignment.target_host_id
        || payload.package_id != assignment.package_id().as_str()
        || payload.catalog_record_digest != assignment.selection.catalog_record_digest.as_str()
        || payload.desired_state != assignment.desired_state
        || payload.assignment_generation != assignment.assignment_generation
        || payload.actor_id != assignment.last_actor_id
    {
        return Err("plugin assignment event payload is inconsistent".into());
    }
    Ok(())
}

#[async_trait]
pub trait IPluginAssignmentRepository: Send + Sync {
    async fn create(
        &self,
        write: CreatePluginAssignmentWrite,
    ) -> Result<IdempotentWrite<PluginAssignment>, RepositoryError>;

    async fn update(
        &self,
        write: UpdatePluginAssignmentWrite,
    ) -> Result<IdempotentWrite<PluginAssignment>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
    ) -> Result<Option<PluginAssignment>, RepositoryError>;

    async fn find_live_for_host_package(
        &self,
        organization_id: OrganizationId,
        target_host_id: NodeId,
        package_id: &PluginPackageId,
    ) -> Result<Option<PluginAssignment>, RepositoryError>;

    async fn list_for_environment(
        &self,
        organization_id: OrganizationId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<PluginAssignment>, RepositoryError>;

    /// Assignments whose current Operation has not been enqueued yet.
    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<PluginAssignment>, RepositoryError>;
}
