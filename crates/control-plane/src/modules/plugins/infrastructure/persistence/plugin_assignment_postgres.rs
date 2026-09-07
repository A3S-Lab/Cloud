use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::plugins::domain::repositories::{
    CreatePluginAssignmentWrite, IPluginAssignmentRepository, UpdatePluginAssignmentWrite,
};
use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, NodeId, OperationId, OrganizationId, PluginAssignmentId,
    PluginRegistryId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use a3s_use_core::{PluginDesiredState, PluginManagedScope, PluginPackageId, PluginSurfaceRef};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct PluginAssignmentRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    registry_id: Uuid,
    target_host_id: Uuid,
    workspace_scope: serde_json::Value,
    package_id: String,
    catalog_record_digest: String,
    version: String,
    package_digest: String,
    manifest_digest: String,
    selected_surfaces: serde_json::Value,
    policy_digest: String,
    desired_state: String,
    assignment_generation: u64,
    aggregate_version: u64,
    current_operation_id: Option<Uuid>,
    last_actor_id: Uuid,
    last_request_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for PluginAssignmentRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            registry_id: decode(row, 4)?,
            target_host_id: decode(row, 5)?,
            workspace_scope: decode(row, 6)?,
            package_id: decode(row, 7)?,
            catalog_record_digest: decode(row, 8)?,
            version: decode(row, 9)?,
            package_digest: decode(row, 10)?,
            manifest_digest: decode(row, 11)?,
            selected_surfaces: decode(row, 12)?,
            policy_digest: decode(row, 13)?,
            desired_state: decode(row, 14)?,
            assignment_generation: decode(row, 15)?,
            aggregate_version: decode(row, 16)?,
            current_operation_id: decode(row, 17)?,
            last_actor_id: decode(row, 18)?,
            last_request_id: decode(row, 19)?,
            created_at: decode(row, 20)?,
            updated_at: decode(row, 21)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresPluginAssignmentRepository {
    executor: PostgresExecutor,
}

impl PostgresPluginAssignmentRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IPluginAssignmentRepository for PostgresPluginAssignmentRepository {
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
        let actor_id = assignment.last_actor_id;
        let request_id = assignment.last_request_id;
        let workspace_scope = serde_json::to_value(&assignment.workspace_scope)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let selected_surfaces = serde_json::to_value(&assignment.selection.selected_surfaces)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let desired_state = desired_state_as_str(assignment.desired_state);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<PluginAssignment>(transaction, &idempotency).await?
                    {
                        CreatePluginAssignmentWrite::validate_replay(&assignment, &replayed.value)?;
                        return Ok(replayed);
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into plugin_assignments (organization_id, project_id, environment_id, id, registry_id, target_host_id, workspace_scope, package_id, catalog_record_digest, version, package_digest, manifest_digest, selected_surfaces, policy_digest, desired_state, assignment_generation, aggregate_version, current_operation_id, last_actor_id, last_request_id, created_at, updated_at) values (",
                        )
                        .bind(assignment.organization_id.as_uuid())
                        .append(", ")
                        .bind(assignment.project_id.as_uuid())
                        .append(", ")
                        .bind(assignment.environment_id.as_uuid())
                        .append(", ")
                        .bind(assignment.id.as_uuid())
                        .append(", ")
                        .bind(assignment.registry_id.as_uuid())
                        .append(", ")
                        .bind(assignment.target_host_id.as_uuid())
                        .append(", ")
                        .bind(workspace_scope)
                        .append(", ")
                        .bind(assignment.package_id().as_str())
                        .append(", ")
                        .bind(assignment.selection.catalog_record_digest.as_str())
                        .append(", ")
                        .bind(assignment.selection.version.as_str())
                        .append(", ")
                        .bind(assignment.selection.package_digest.as_str())
                        .append(", ")
                        .bind(assignment.selection.manifest_digest.as_str())
                        .append(", ")
                        .bind(selected_surfaces)
                        .append(", ")
                        .bind(assignment.policy_digest.as_str())
                        .append(", ")
                        .bind(desired_state)
                        .append(", ")
                        .bind(assignment.assignment_generation)
                        .append(", ")
                        .bind(assignment.aggregate_version)
                        .append(", ")
                        .bind(assignment.current_operation_id.map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(assignment.last_actor_id.as_uuid())
                        .append(", ")
                        .bind(assignment.last_request_id)
                        .append(", ")
                        .bind(assignment.created_at)
                        .append(", ")
                        .bind(assignment.updated_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating plugin assignment affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "plugin assignment already exists for this host package".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            scope: AuditWrite::resource_scope(
                                assignment.organization_id.as_uuid(),
                                assignment.project_id,
                                Some(assignment.environment_id),
                            ),
                            actor_id: Some(actor_id.as_uuid()),
                            action: "plugin.assignment.changed",
                            aggregate_id: assignment.id.as_uuid(),
                            occurred_at: assignment.created_at,
                            request_id,
                            details: serde_json::json!({
                                "packageId": assignment.package_id().as_str(),
                                "desiredState": desired_state,
                                "assignmentGeneration": assignment.assignment_generation,
                                "targetHostId": assignment.target_host_id.as_uuid(),
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &idempotency, &assignment).await?;
                    Ok(IdempotentWrite {
                        value: assignment,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
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
        let actor_id = assignment.last_actor_id;
        let request_id = assignment.last_request_id;
        let workspace_scope = serde_json::to_value(&assignment.workspace_scope)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let selected_surfaces = serde_json::to_value(&assignment.selection.selected_surfaces)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let desired_state = desired_state_as_str(assignment.desired_state);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<PluginAssignment>(transaction, &idempotency).await?
                    {
                        UpdatePluginAssignmentWrite::validate_replay(&assignment, &replayed.value)?;
                        return Ok(replayed);
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>(
                            "update plugin_assignments set workspace_scope = ",
                        )
                        .bind(workspace_scope)
                        .append(", catalog_record_digest = ")
                        .bind(assignment.selection.catalog_record_digest.as_str())
                        .append(", version = ")
                        .bind(assignment.selection.version.as_str())
                        .append(", package_digest = ")
                        .bind(assignment.selection.package_digest.as_str())
                        .append(", manifest_digest = ")
                        .bind(assignment.selection.manifest_digest.as_str())
                        .append(", selected_surfaces = ")
                        .bind(selected_surfaces)
                        .append(", policy_digest = ")
                        .bind(assignment.policy_digest.as_str())
                        .append(", desired_state = ")
                        .bind(desired_state)
                        .append(", assignment_generation = ")
                        .bind(assignment.assignment_generation)
                        .append(", aggregate_version = ")
                        .bind(assignment.aggregate_version)
                        .append(", current_operation_id = ")
                        .bind(assignment.current_operation_id.map(|id| id.as_uuid()))
                        .append(", last_actor_id = ")
                        .bind(assignment.last_actor_id.as_uuid())
                        .append(", last_request_id = ")
                        .bind(assignment.last_request_id)
                        .append(", updated_at = ")
                        .bind(assignment.updated_at)
                        .append(" where organization_id = ")
                        .bind(assignment.organization_id.as_uuid())
                        .append(" and id = ")
                        .bind(assignment.id.as_uuid())
                        .append(" and aggregate_version = ")
                        .bind(expected_aggregate_version)
                        .append(" and target_host_id = ")
                        .bind(assignment.target_host_id.as_uuid())
                        .append(" and package_id = ")
                        .bind(assignment.package_id().as_str())
                        .append(" and project_id = ")
                        .bind(assignment.project_id.as_uuid())
                        .append(" and environment_id = ")
                        .bind(assignment.environment_id.as_uuid())
                        .append(" and registry_id = ")
                        .bind(assignment.registry_id.as_uuid())
                        .append(" and created_at = ")
                        .bind(assignment.created_at),
                    )
                    .await;
                    match updated {
                        Ok(1) => {}
                        Ok(0) => {
                            return Err(RepositoryError::Conflict(
                                "plugin assignment changed while updating desired state".into(),
                            )
                            .into())
                        }
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "updating plugin assignment affected {rows} rows"
                            )))
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            scope: AuditWrite::resource_scope(
                                assignment.organization_id.as_uuid(),
                                assignment.project_id,
                                Some(assignment.environment_id),
                            ),
                            actor_id: Some(actor_id.as_uuid()),
                            action: "plugin.assignment.changed",
                            aggregate_id: assignment.id.as_uuid(),
                            occurred_at: assignment.updated_at,
                            request_id,
                            details: serde_json::json!({
                                "packageId": assignment.package_id().as_str(),
                                "desiredState": desired_state,
                                "assignmentGeneration": assignment.assignment_generation,
                                "targetHostId": assignment.target_host_id.as_uuid(),
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &idempotency, &assignment).await?;
                    Ok(IdempotentWrite {
                        value: assignment,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
    ) -> Result<Option<PluginAssignment>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                plugin_assignment_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(assignment_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(plugin_assignment_from_row)
            .transpose()
    }

    async fn find_live_for_host_package(
        &self,
        organization_id: OrganizationId,
        target_host_id: NodeId,
        package_id: &PluginPackageId,
    ) -> Result<Option<PluginAssignment>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                plugin_assignment_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and target_host_id = ")
                    .bind(target_host_id.as_uuid())
                    .append(" and package_id = ")
                    .bind(package_id.as_str()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(plugin_assignment_from_row)
            .transpose()
    }

    async fn list_for_environment(
        &self,
        organization_id: OrganizationId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<PluginAssignment>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                plugin_assignment_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(plugin_assignment_from_row)
            .collect()
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<PluginAssignment>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<PluginAssignmentRow>(
                    "select a.organization_id, a.project_id, a.environment_id, a.id, a.registry_id, a.target_host_id, a.workspace_scope, a.package_id, a.catalog_record_digest, a.version, a.package_digest, a.manifest_digest, a.selected_surfaces, a.policy_digest, a.desired_state, a.assignment_generation, a.aggregate_version, a.current_operation_id, a.last_actor_id, a.last_request_id, a.created_at, a.updated_at from plugin_assignments a left join operation_requests o on o.operation_id = a.current_operation_id where a.current_operation_id is not null and o.operation_id is null order by a.updated_at asc, a.id asc limit ",
                )
                .bind(limit),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(plugin_assignment_from_row)
            .collect()
    }
}

fn plugin_assignment_select() -> a3s_orm::SqlQuery<PluginAssignmentRow> {
    sql_query::<PluginAssignmentRow>(
        "select organization_id, project_id, environment_id, id, registry_id, target_host_id, workspace_scope, package_id, catalog_record_digest, version, package_digest, manifest_digest, selected_surfaces, policy_digest, desired_state, assignment_generation, aggregate_version, current_operation_id, last_actor_id, last_request_id, created_at, updated_at from plugin_assignments",
    )
}

fn plugin_assignment_from_row(row: PluginAssignmentRow) -> Result<PluginAssignment, RepositoryError> {
    let PluginAssignmentRow {
        organization_id,
        project_id,
        environment_id,
        id,
        registry_id,
        target_host_id,
        workspace_scope,
        package_id,
        catalog_record_digest,
        version,
        package_digest,
        manifest_digest,
        selected_surfaces,
        policy_digest,
        desired_state,
        assignment_generation,
        aggregate_version,
        current_operation_id,
        last_actor_id,
        last_request_id,
        created_at,
        updated_at,
    } = row;
    let workspace_scope: PluginManagedScope = serde_json::from_value(workspace_scope)
        .map_err(|error| stored_error("workspace scope", error))?;
    let selected_surfaces: Vec<PluginSurfaceRef> = serde_json::from_value(selected_surfaces)
        .map_err(|error| stored_error("selected surfaces", error))?;
    let package_id =
        PluginPackageId::parse(package_id).map_err(|error| stored_error("package id", error))?;
    let selection = PluginCatalogSelection {
        package_id,
        catalog_record_digest: Sha256Digest::parse(catalog_record_digest)
            .map_err(|error| stored_error("catalog record digest", error))?,
        version,
        package_digest: Sha256Digest::parse(package_digest)
            .map_err(|error| stored_error("package digest", error))?,
        manifest_digest: Sha256Digest::parse(manifest_digest)
            .map_err(|error| stored_error("manifest digest", error))?,
        selected_surfaces,
    };
    let assignment = PluginAssignment {
        organization_id: OrganizationId::from_uuid(organization_id),
        project_id: ProjectId::from_uuid(project_id),
        environment_id: EnvironmentId::from_uuid(environment_id),
        id: PluginAssignmentId::from_uuid(id),
        registry_id: PluginRegistryId::from_uuid(registry_id),
        target_host_id: NodeId::from_uuid(target_host_id),
        workspace_scope,
        selection,
        policy_digest: Sha256Digest::parse(policy_digest)
            .map_err(|error| stored_error("policy digest", error))?,
        desired_state: parse_desired_state(&desired_state)?,
        assignment_generation,
        aggregate_version,
        current_operation_id: current_operation_id.map(OperationId::from_uuid),
        last_actor_id: PrincipalId::from_uuid(last_actor_id),
        last_request_id,
        created_at,
        updated_at,
    };
    assignment
        .validate()
        .map_err(|error| stored_error("record", error))?;
    Ok(assignment)
}

fn desired_state_as_str(desired_state: PluginDesiredState) -> &'static str {
    match desired_state {
        PluginDesiredState::Enabled => "enabled",
        PluginDesiredState::InstalledDisabled => "installed-disabled",
        PluginDesiredState::Absent => "absent",
    }
}

fn parse_desired_state(value: &str) -> Result<PluginDesiredState, RepositoryError> {
    match value {
        "enabled" => Ok(PluginDesiredState::Enabled),
        "installed-disabled" => Ok(PluginDesiredState::InstalledDisabled),
        "absent" => Ok(PluginDesiredState::Absent),
        _ => Err(stored_error("desired state", "unknown value")),
    }
}

fn stored_error(field: &str, error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(format!(
        "stored plugin assignment {field} is invalid: {error}"
    ))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
