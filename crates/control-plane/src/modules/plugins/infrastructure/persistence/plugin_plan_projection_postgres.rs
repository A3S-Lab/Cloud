use crate::infrastructure::{
    execute, is_foreign_key_violation, is_unique_violation, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::plugins::domain::repositories::IPluginPlanProjectionRepository;
use crate::modules::shared_kernel::domain::{
    OperationId, OrganizationId, PluginAssignmentId, PluginPlanProjectionId, RepositoryError,
    Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use a3s_use_core::{PlanPolicyDecision, PluginOperationAction, PluginOperationConfirmation};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct PluginPlanProjectionRow {
    organization_id: Uuid,
    id: Uuid,
    assignment_id: Uuid,
    operation_id: Uuid,
    assignment_generation: u64,
    use_operation_id: String,
    plan_schema: String,
    plan_digest: String,
    expires_at: DateTime<Utc>,
    action: String,
    root_package_id: String,
    root_package_digest: Option<String>,
    root_manifest_digest: Option<String>,
    authority_decision: String,
    authority_policy_digest: String,
    impact_digest: String,
    permission_evidence_digest: String,
    provider_evidence_digest: String,
    confirmation_digest: Option<String>,
    confirmation: Option<serde_json::Value>,
    terminal_reason: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for PluginPlanProjectionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            assignment_id: decode(row, 2)?,
            operation_id: decode(row, 3)?,
            assignment_generation: decode(row, 4)?,
            use_operation_id: decode(row, 5)?,
            plan_schema: decode(row, 6)?,
            plan_digest: decode(row, 7)?,
            expires_at: decode(row, 8)?,
            action: decode(row, 9)?,
            root_package_id: decode(row, 10)?,
            root_package_digest: decode(row, 11)?,
            root_manifest_digest: decode(row, 12)?,
            authority_decision: decode(row, 13)?,
            authority_policy_digest: decode(row, 14)?,
            impact_digest: decode(row, 15)?,
            permission_evidence_digest: decode(row, 16)?,
            provider_evidence_digest: decode(row, 17)?,
            confirmation_digest: decode(row, 18)?,
            confirmation: decode(row, 19)?,
            terminal_reason: decode(row, 20)?,
            created_at: decode(row, 21)?,
            updated_at: decode(row, 22)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresPluginPlanProjectionRepository {
    executor: PostgresExecutor,
}

impl PostgresPluginPlanProjectionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IPluginPlanProjectionRepository for PostgresPluginPlanProjectionRepository {
    async fn create(
        &self,
        projection: PluginPlanProjection,
    ) -> Result<PluginPlanProjection, RepositoryError> {
        projection.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let confirmation = projection
                        .confirmation
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()
                        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into plugin_plan_projections (organization_id, id, assignment_id, operation_id, assignment_generation, use_operation_id, plan_schema, plan_digest, expires_at, action, root_package_id, root_package_digest, root_manifest_digest, authority_decision, authority_policy_digest, impact_digest, permission_evidence_digest, provider_evidence_digest, confirmation_digest, confirmation, terminal_reason, created_at, updated_at) values (",
                        )
                        .bind(projection.organization_id.as_uuid())
                        .append(", ")
                        .bind(projection.id.as_uuid())
                        .append(", ")
                        .bind(projection.assignment_id.as_uuid())
                        .append(", ")
                        .bind(projection.operation_id.as_uuid())
                        .append(", ")
                        .bind(projection.assignment_generation as i64)
                        .append(", ")
                        .bind(projection.use_operation_id.as_str())
                        .append(", ")
                        .bind(projection.plan_schema.as_str())
                        .append(", ")
                        .bind(projection.plan_digest.as_str())
                        .append(", ")
                        .bind(projection.expires_at)
                        .append(", ")
                        .bind(action_str(projection.action))
                        .append(", ")
                        .bind(projection.root_package_id.as_str())
                        .append(", ")
                        .bind(projection.root_package_digest.as_ref().map(Sha256Digest::as_str))
                        .append(", ")
                        .bind(projection.root_manifest_digest.as_ref().map(Sha256Digest::as_str))
                        .append(", ")
                        .bind(decision_str(projection.authority_decision))
                        .append(", ")
                        .bind(projection.authority_policy_digest.as_str())
                        .append(", ")
                        .bind(projection.impact_digest.as_str())
                        .append(", ")
                        .bind(projection.permission_evidence_digest.as_str())
                        .append(", ")
                        .bind(projection.provider_evidence_digest.as_str())
                        .append(", ")
                        .bind(projection.confirmation_digest.as_ref().map(Sha256Digest::as_str))
                        .append(", ")
                        .bind(confirmation)
                        .append(", ")
                        .bind(projection.terminal_reason.as_deref())
                        .append(", ")
                        .bind(projection.created_at)
                        .append(", ")
                        .bind(projection.updated_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => Ok(projection),
                        Ok(rows) => Err(PostgresPersistenceError::Invariant(format!(
                            "creating plugin plan projection affected {rows} rows"
                        ))),
                        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
                            "plugin plan projection already exists for this plan digest".into(),
                        )
                        .into()),
                        Err(error) if is_foreign_key_violation(&error) => {
                            Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => Err(error),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn update(
        &self,
        projection: PluginPlanProjection,
        expected_confirmation_digest: Option<&Sha256Digest>,
    ) -> Result<PluginPlanProjection, RepositoryError> {
        projection.validate().map_err(RepositoryError::Storage)?;
        let expected = expected_confirmation_digest.map(|digest| digest.as_str().to_owned());
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let confirmation = projection
                        .confirmation
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()
                        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update plugin_plan_projections set confirmation_digest = ")
                            .bind(projection.confirmation_digest.as_ref().map(Sha256Digest::as_str))
                            .append(", confirmation = ")
                            .bind(confirmation)
                            .append(", terminal_reason = ")
                            .bind(projection.terminal_reason.as_deref())
                            .append(", updated_at = ")
                            .bind(projection.updated_at)
                            .append(" where organization_id = ")
                            .bind(projection.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(projection.id.as_uuid())
                            .append(" and confirmation_digest is not distinct from ")
                            .bind(expected.as_deref()),
                    )
                    .await?;
                    if updated == 0 {
                        return Err(RepositoryError::Conflict(
                            "plugin plan projection confirmation changed concurrently".into(),
                        )
                        .into());
                    }
                    Ok(projection)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        projection_id: PluginPlanProjectionId,
    ) -> Result<Option<PluginPlanProjection>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                plugin_plan_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(projection_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(from_row)
            .transpose()
    }

    async fn find_by_plan_digest(
        &self,
        organization_id: OrganizationId,
        plan_digest: &Sha256Digest,
    ) -> Result<Option<PluginPlanProjection>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                plugin_plan_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and plan_digest = ")
                    .bind(plan_digest.as_str()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(from_row)
            .transpose()
    }

    async fn list_for_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
    ) -> Result<Vec<PluginPlanProjection>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                plugin_plan_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and assignment_id = ")
                    .bind(assignment_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(from_row)
            .collect()
    }
}

fn plugin_plan_select() -> a3s_orm::SqlQuery<PluginPlanProjectionRow> {
    sql_query::<PluginPlanProjectionRow>(
        "select organization_id, id, assignment_id, operation_id, assignment_generation, use_operation_id, plan_schema, plan_digest, expires_at, action, root_package_id, root_package_digest, root_manifest_digest, authority_decision, authority_policy_digest, impact_digest, permission_evidence_digest, provider_evidence_digest, confirmation_digest, confirmation, terminal_reason, created_at, updated_at from plugin_plan_projections",
    )
}

fn from_row(row: PluginPlanProjectionRow) -> Result<PluginPlanProjection, RepositoryError> {
    Ok(PluginPlanProjection {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        id: PluginPlanProjectionId::from_uuid(row.id),
        assignment_id: PluginAssignmentId::from_uuid(row.assignment_id),
        operation_id: OperationId::from_uuid(row.operation_id),
        assignment_generation: row.assignment_generation,
        use_operation_id: row.use_operation_id,
        plan_schema: row.plan_schema,
        plan_digest: Sha256Digest::parse(row.plan_digest).map_err(RepositoryError::Storage)?,
        expires_at: row.expires_at,
        action: parse_action(&row.action)?,
        root_package_id: row.root_package_id,
        root_package_digest: row
            .root_package_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(RepositoryError::Storage)?,
        root_manifest_digest: row
            .root_manifest_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(RepositoryError::Storage)?,
        authority_decision: parse_decision(&row.authority_decision)?,
        authority_policy_digest: Sha256Digest::parse(row.authority_policy_digest)
            .map_err(RepositoryError::Storage)?,
        impact_digest: Sha256Digest::parse(row.impact_digest).map_err(RepositoryError::Storage)?,
        permission_evidence_digest: Sha256Digest::parse(row.permission_evidence_digest)
            .map_err(RepositoryError::Storage)?,
        provider_evidence_digest: Sha256Digest::parse(row.provider_evidence_digest)
            .map_err(RepositoryError::Storage)?,
        confirmation_digest: row
            .confirmation_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(RepositoryError::Storage)?,
        confirmation: row
            .confirmation
            .map(|value| {
                serde_json::from_value::<PluginOperationConfirmation>(value)
                    .map_err(|error| RepositoryError::Storage(error.to_string()))
            })
            .transpose()?,
        terminal_reason: row.terminal_reason,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn action_str(action: PluginOperationAction) -> &'static str {
    match action {
        PluginOperationAction::Install => "install",
        PluginOperationAction::Uninstall => "uninstall",
        PluginOperationAction::Upgrade => "upgrade",
        PluginOperationAction::Enable => "enable",
        PluginOperationAction::Disable => "disable",
    }
}

fn decision_str(decision: PlanPolicyDecision) -> &'static str {
    match decision {
        PlanPolicyDecision::Allow => "allow",
        PlanPolicyDecision::Ask => "ask",
        PlanPolicyDecision::Deny => "deny",
    }
}

fn parse_action(value: &str) -> Result<PluginOperationAction, RepositoryError> {
    match value {
        "install" => Ok(PluginOperationAction::Install),
        "uninstall" => Ok(PluginOperationAction::Uninstall),
        "upgrade" => Ok(PluginOperationAction::Upgrade),
        "enable" => Ok(PluginOperationAction::Enable),
        "disable" => Ok(PluginOperationAction::Disable),
        _ => Err(RepositoryError::Storage(
            "plugin plan projection action is invalid".into(),
        )),
    }
}

fn parse_decision(value: &str) -> Result<PlanPolicyDecision, RepositoryError> {
    match value {
        "allow" => Ok(PlanPolicyDecision::Allow),
        "ask" => Ok(PlanPolicyDecision::Ask),
        "deny" => Ok(PlanPolicyDecision::Deny),
        _ => Err(RepositoryError::Storage(
            "plugin plan projection authority decision is invalid".into(),
        )),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
