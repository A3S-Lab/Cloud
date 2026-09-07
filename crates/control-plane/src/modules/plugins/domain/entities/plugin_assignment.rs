use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, NodeId, OperationId, OrganizationId, PluginAssignmentId,
    PluginRegistryId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_use_core::{PluginDesiredState, PluginManagedScope, PluginPackageId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPluginAssignment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: PluginAssignmentId,
    pub registry_id: PluginRegistryId,
    pub target_host_id: NodeId,
    pub workspace_scope: PluginManagedScope,
    pub selection: PluginCatalogSelection,
    pub policy_digest: Sha256Digest,
    pub desired_state: PluginDesiredState,
    pub actor_id: PrincipalId,
    pub request_id: Uuid,
    pub operation_id: OperationId,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginAssignment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: PluginAssignmentId,
    pub registry_id: PluginRegistryId,
    pub target_host_id: NodeId,
    pub workspace_scope: PluginManagedScope,
    pub selection: PluginCatalogSelection,
    pub policy_digest: Sha256Digest,
    pub desired_state: PluginDesiredState,
    pub assignment_generation: u64,
    pub aggregate_version: u64,
    pub current_operation_id: Option<OperationId>,
    pub last_actor_id: PrincipalId,
    pub last_request_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PluginAssignment {
    pub fn create(input: NewPluginAssignment) -> Result<Self, String> {
        let created_at = canonical_timestamp(input.created_at);
        let assignment = Self {
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id: input.environment_id,
            id: input.id,
            registry_id: input.registry_id,
            target_host_id: input.target_host_id,
            workspace_scope: input.workspace_scope,
            selection: input.selection,
            policy_digest: input.policy_digest,
            desired_state: input.desired_state,
            assignment_generation: 1,
            aggregate_version: 1,
            current_operation_id: Some(input.operation_id),
            last_actor_id: input.actor_id,
            last_request_id: input.request_id,
            created_at,
            updated_at: created_at,
        };
        assignment.validate()?;
        Ok(assignment)
    }

    /// Advance desired intent for the same live assignment identity.
    ///
    /// Organization, package, and target host stay fixed. A material change
    /// bumps the Cloud assignment generation and clears applied convergence by
    /// recording the new Operation.
    pub fn set_desired(
        &self,
        selection: PluginCatalogSelection,
        policy_digest: Sha256Digest,
        desired_state: PluginDesiredState,
        workspace_scope: PluginManagedScope,
        actor_id: PrincipalId,
        request_id: Uuid,
        operation_id: OperationId,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if selection.package_id != self.selection.package_id {
            return Err("plugin assignment package identity is immutable".into());
        }
        if workspace_scope.host_id != self.workspace_scope.host_id {
            return Err("plugin assignment host scope identity is immutable".into());
        }
        let unchanged = selection == self.selection
            && policy_digest == self.policy_digest
            && desired_state == self.desired_state
            && workspace_scope == self.workspace_scope;
        if unchanged {
            return Err("plugin assignment desired state is unchanged".into());
        }
        let updated_at = canonical_timestamp(updated_at);
        if updated_at < self.updated_at {
            return Err("plugin assignment update timestamp must not move backward".into());
        }
        let next = Self {
            selection,
            policy_digest,
            desired_state,
            workspace_scope,
            assignment_generation: self
                .assignment_generation
                .checked_add(1)
                .ok_or_else(|| "plugin assignment generation overflow".to_owned())?,
            aggregate_version: self
                .aggregate_version
                .checked_add(1)
                .ok_or_else(|| "plugin assignment aggregate version overflow".to_owned())?,
            current_operation_id: Some(operation_id),
            last_actor_id: actor_id,
            last_request_id: request_id,
            updated_at,
            ..self.clone()
        };
        next.validate()?;
        Ok(next)
    }

    pub fn package_id(&self) -> &PluginPackageId {
        &self.selection.package_id
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.registry_id.as_uuid().is_nil()
            || self.target_host_id.as_uuid().is_nil()
            || self.last_actor_id.as_uuid().is_nil()
            || self.last_request_id.is_nil()
            || self.assignment_generation == 0
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
        {
            return Err("plugin assignment identity, version, or timestamps are invalid".into());
        }
        if let Some(operation_id) = self.current_operation_id {
            if operation_id.as_uuid().is_nil() {
                return Err("plugin assignment operation identity is invalid".into());
            }
        }
        self.workspace_scope
            .validate()
            .map_err(|_| "plugin assignment workspace scope is invalid".to_owned())?;
        self.selection.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NewPluginAssignment, PluginAssignment};
    use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OperationId, OrganizationId, PluginAssignmentId, PluginRegistryId,
        PrincipalId, ProjectId, Sha256Digest,
    };
    use a3s_use_core::{
        PlanScopeKind, PluginDesiredState, PluginManagedScope, PluginPackageId, PluginSurfaceKind,
        PluginSurfaceRef, PLUGIN_MANAGED_SCOPE_SCHEMA_V2,
    };
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn scope() -> PluginManagedScope {
        PluginManagedScope {
            schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
            host_id: "host:node-01".into(),
            scope_kind: PlanScopeKind::Workspace,
            scope_id: "workspace:research".into(),
            authority_id: "cloud:organization-01".into(),
            fence_generation: 7,
            fence_digest: digest('d').as_str().into(),
        }
    }

    fn selection() -> PluginCatalogSelection {
        PluginCatalogSelection {
            package_id: PluginPackageId::parse("a3s/registry-selftest").expect("package"),
            catalog_record_digest: digest('a'),
            version: "0.1.0".into(),
            package_digest: digest('b'),
            manifest_digest: digest('c'),
            selected_surfaces: vec![PluginSurfaceRef {
                kind: PluginSurfaceKind::Skill,
                id: "selftest".into(),
            }],
        }
    }

    fn new_assignment() -> NewPluginAssignment {
        NewPluginAssignment {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: PluginAssignmentId::new(),
            registry_id: PluginRegistryId::new(),
            target_host_id: NodeId::new(),
            workspace_scope: scope(),
            selection: selection(),
            policy_digest: digest('e'),
            desired_state: PluginDesiredState::Enabled,
            actor_id: PrincipalId::new(),
            request_id: Uuid::now_v7(),
            operation_id: OperationId::new(),
            created_at: Utc
                .with_ymd_and_hms(2026, 9, 7, 8, 0, 0)
                .single()
                .expect("timestamp"),
        }
    }

    #[test]
    fn create_starts_generation_one_with_canonical_desired_state() {
        let assignment = PluginAssignment::create(new_assignment()).expect("assignment");
        assert_eq!(assignment.assignment_generation, 1);
        assert_eq!(assignment.aggregate_version, 1);
        assert_eq!(assignment.desired_state, PluginDesiredState::Enabled);
        assert_eq!(
            assignment.package_id().as_str(),
            "a3s/registry-selftest"
        );
        assert!(assignment.current_operation_id.is_some());
    }

    #[test]
    fn set_desired_bumps_assignment_generation_for_material_changes() {
        let created = PluginAssignment::create(new_assignment()).expect("assignment");
        let mut next_selection = selection();
        next_selection.version = "0.2.0".into();
        next_selection.catalog_record_digest = digest('f');
        let revised = created
            .set_desired(
                next_selection,
                digest('e'),
                PluginDesiredState::InstalledDisabled,
                scope(),
                PrincipalId::new(),
                Uuid::now_v7(),
                OperationId::new(),
                Utc.with_ymd_and_hms(2026, 9, 7, 8, 1, 0)
                    .single()
                    .expect("timestamp"),
            )
            .expect("revise");
        assert_eq!(revised.assignment_generation, 2);
        assert_eq!(revised.aggregate_version, 2);
        assert_eq!(revised.desired_state, PluginDesiredState::InstalledDisabled);
        assert_eq!(revised.selection.version, "0.2.0");
    }

    #[test]
    fn set_desired_rejects_package_drift_and_unchanged_intent() {
        let created = PluginAssignment::create(new_assignment()).expect("assignment");
        let mut other_package = selection();
        other_package.package_id = PluginPackageId::parse("a3s/other").expect("package");
        assert!(created
            .set_desired(
                other_package,
                digest('e'),
                PluginDesiredState::Enabled,
                scope(),
                PrincipalId::new(),
                Uuid::now_v7(),
                OperationId::new(),
                Utc.with_ymd_and_hms(2026, 9, 7, 8, 1, 0)
                    .single()
                    .expect("timestamp"),
            )
            .is_err());
        assert!(created
            .set_desired(
                selection(),
                digest('e'),
                PluginDesiredState::Enabled,
                scope(),
                PrincipalId::new(),
                Uuid::now_v7(),
                OperationId::new(),
                Utc.with_ymd_and_hms(2026, 9, 7, 8, 1, 0)
                    .single()
                    .expect("timestamp"),
            )
            .is_err());
    }
}
