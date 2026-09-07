use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::repositories::{
    CreatePluginAssignmentWrite, IPluginAssignmentRepository, UpdatePluginAssignmentWrite,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, NodeId, OrganizationId, PluginAssignmentId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_use_core::PluginPackageId;
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryPluginAssignmentRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    assignments: BTreeMap<(OrganizationId, PluginAssignmentId), PluginAssignment>,
    host_packages: BTreeMap<(OrganizationId, NodeId, String), PluginAssignmentId>,
    idempotency: BTreeMap<(String, String), (String, PluginAssignment)>,
    started_operations: BTreeSet<crate::modules::shared_kernel::domain::OperationId>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryPluginAssignmentRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    pub async fn mark_operation_started(
        &self,
        operation_id: crate::modules::shared_kernel::domain::OperationId,
    ) {
        self.state
            .write()
            .await
            .started_operations
            .insert(operation_id);
    }
}

#[async_trait]
impl IPluginAssignmentRepository for InMemoryPluginAssignmentRepository {
    async fn create(
        &self,
        write: CreatePluginAssignmentWrite,
    ) -> Result<IdempotentWrite<PluginAssignment>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let CreatePluginAssignmentWrite {
            assignment,
            event,
            idempotency,
        } = write;
        let mut state = self.state.write().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, existing)) = state.idempotency.get(&key) {
            if digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            CreatePluginAssignmentWrite::validate_replay(&assignment, existing)?;
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        let host_package_key = (
            assignment.organization_id,
            assignment.target_host_id,
            assignment.package_id().as_str().to_owned(),
        );
        if state.host_packages.contains_key(&host_package_key) {
            return Err(RepositoryError::Conflict(
                "plugin assignment already exists for this host package".into(),
            ));
        }
        state
            .host_packages
            .insert(host_package_key, assignment.id);
        state.assignments.insert(
            (assignment.organization_id, assignment.id),
            assignment.clone(),
        );
        state
            .idempotency
            .insert(key, (idempotency.request_digest, assignment.clone()));
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: assignment,
            replayed: false,
        })
    }

    async fn update(
        &self,
        write: UpdatePluginAssignmentWrite,
    ) -> Result<IdempotentWrite<PluginAssignment>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let UpdatePluginAssignmentWrite {
            assignment,
            expected_aggregate_version,
            event,
            idempotency,
        } = write;
        let mut state = self.state.write().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, existing)) = state.idempotency.get(&key) {
            if digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            UpdatePluginAssignmentWrite::validate_replay(&assignment, existing)?;
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        let Some(current) = state
            .assignments
            .get(&(assignment.organization_id, assignment.id))
            .cloned()
        else {
            return Err(RepositoryError::NotFound);
        };
        if current.aggregate_version != expected_aggregate_version
            || current.target_host_id != assignment.target_host_id
            || current.package_id() != assignment.package_id()
            || current.environment_id != assignment.environment_id
            || current.project_id != assignment.project_id
            || current.registry_id != assignment.registry_id
            || current.created_at != assignment.created_at
        {
            return Err(RepositoryError::Conflict(
                "plugin assignment changed while updating desired state".into(),
            ));
        }
        state.assignments.insert(
            (assignment.organization_id, assignment.id),
            assignment.clone(),
        );
        state
            .idempotency
            .insert(key, (idempotency.request_digest, assignment.clone()));
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: assignment,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
    ) -> Result<Option<PluginAssignment>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .assignments
            .get(&(organization_id, assignment_id))
            .cloned())
    }

    async fn find_live_for_host_package(
        &self,
        organization_id: OrganizationId,
        target_host_id: NodeId,
        package_id: &PluginPackageId,
    ) -> Result<Option<PluginAssignment>, RepositoryError> {
        let state = self.state.read().await;
        let Some(assignment_id) = state
            .host_packages
            .get(&(
                organization_id,
                target_host_id,
                package_id.as_str().to_owned(),
            ))
            .copied()
        else {
            return Ok(None);
        };
        Ok(state
            .assignments
            .get(&(organization_id, assignment_id))
            .cloned())
    }

    async fn list_for_environment(
        &self,
        organization_id: OrganizationId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<PluginAssignment>, RepositoryError> {
        let mut assignments = self
            .state
            .read()
            .await
            .assignments
            .values()
            .filter(|assignment| {
                assignment.organization_id == organization_id
                    && assignment.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        assignments.sort_by_key(|assignment| (assignment.created_at, assignment.id));
        Ok(assignments)
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<PluginAssignment>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = self.state.read().await;
        let mut assignments = state
            .assignments
            .values()
            .filter(|assignment| {
                assignment
                    .current_operation_id
                    .is_some_and(|operation_id| !state.started_operations.contains(&operation_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        assignments.sort_by_key(|assignment| (assignment.updated_at, assignment.id));
        assignments.truncate(limit);
        Ok(assignments)
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryPluginAssignmentRepository;
    use crate::modules::plugins::domain::entities::{NewPluginAssignment, PluginAssignment};
    use crate::modules::plugins::domain::events::PluginAssignmentChanged;
    use crate::modules::plugins::domain::repositories::{
        CreatePluginAssignmentWrite, IPluginAssignmentRepository,
    };
    use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OperationId, OrganizationId, PluginAssignmentId, PluginRegistryId,
        PrincipalId, ProjectId, RepositoryError, Sha256Digest,
    };
    use a3s_use_core::{
        PlanScopeKind, PluginDesiredState, PluginManagedScope, PluginPackageId, PluginSurfaceKind,
        PluginSurfaceRef, PLUGIN_MANAGED_SCOPE_SCHEMA_V2,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn assignment(
        organization_id: OrganizationId,
        target_host_id: NodeId,
        package_id: &str,
    ) -> PluginAssignment {
        PluginAssignment::create(NewPluginAssignment {
            organization_id,
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: PluginAssignmentId::new(),
            registry_id: PluginRegistryId::new(),
            target_host_id,
            workspace_scope: PluginManagedScope {
                schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
                host_id: "host:node-01".into(),
                scope_kind: PlanScopeKind::Workspace,
                scope_id: "workspace:research".into(),
                authority_id: "cloud:organization-01".into(),
                fence_generation: 7,
                fence_digest: digest('d').as_str().into(),
            },
            selection: PluginCatalogSelection {
                package_id: PluginPackageId::parse(package_id).expect("package"),
                catalog_record_digest: digest('a'),
                version: "0.1.0".into(),
                package_digest: digest('b'),
                manifest_digest: digest('c'),
                selected_surfaces: vec![PluginSurfaceRef {
                    kind: PluginSurfaceKind::Skill,
                    id: "selftest".into(),
                }],
            },
            policy_digest: digest('e'),
            desired_state: PluginDesiredState::Enabled,
            actor_id: PrincipalId::new(),
            request_id: Uuid::now_v7(),
            operation_id: OperationId::new(),
            created_at: Utc::now(),
        })
        .expect("assignment")
    }

    fn write(assignment: PluginAssignment, key: &str) -> CreatePluginAssignmentWrite {
        let event = PluginAssignmentChanged::envelope(&assignment).expect("event");
        let idempotency =
            CreatePluginAssignmentWrite::idempotency_for(&assignment, key).expect("idempotency");
        CreatePluginAssignmentWrite {
            assignment,
            event,
            idempotency,
        }
    }

    #[tokio::test]
    async fn create_replays_once_and_enforces_one_live_host_package() {
        let repository = InMemoryPluginAssignmentRepository::new();
        let organization_id = OrganizationId::new();
        let host_id = NodeId::new();
        let created_assignment = assignment(organization_id, host_id, "a3s/registry-selftest");
        let environment_id = created_assignment.environment_id;

        let created = repository
            .create(write(created_assignment.clone(), "assign-1"))
            .await
            .expect("create");
        let replayed = repository
            .create(write(created_assignment.clone(), "assign-1"))
            .await
            .expect("replay");
        let conflict = repository
            .create(write(
                assignment(organization_id, host_id, "a3s/registry-selftest"),
                "assign-2",
            ))
            .await;

        assert!(!created.replayed);
        assert!(replayed.replayed);
        assert_eq!(replayed.value.id, created_assignment.id);
        assert!(matches!(conflict, Err(RepositoryError::Conflict(_))));
        assert_eq!(
            repository
                .list_for_environment(organization_id, environment_id)
                .await
                .expect("list")
                .len(),
            1
        );
        assert_eq!(
            repository
                .find_live_for_host_package(
                    organization_id,
                    host_id,
                    &PluginPackageId::parse("a3s/registry-selftest").expect("package"),
                )
                .await
                .expect("lookup")
                .expect("present")
                .id,
            created_assignment.id
        );
        assert_eq!(repository.outbox_events().await.len(), 1);
    }
}
