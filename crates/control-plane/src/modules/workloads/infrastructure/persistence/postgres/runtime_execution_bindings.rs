use super::schema::DeploymentRuntimeExecutionBindings;
use super::{queries, replicas};
use crate::infrastructure::{
    execute, fetch_optional, require_one_row, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, IdempotentWrite, NodePoolId, OrganizationId, ProjectId,
    RepositoryError, Sha256Digest, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    DeploymentRuntimeExecutionBinding, WorkloadRuntimeExecutionBinding,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel as IsolationLevel, RuntimeUnitClass};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, DecodeError, Expression, FromRow, FromValue, PostgresExecutor,
    PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct BindingSelection;

impl Selection for BindingSelection {
    type Output = BindingRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            DeploymentRuntimeExecutionBindings::binding_schema().expression(),
            DeploymentRuntimeExecutionBindings::deployment_id().expression(),
            DeploymentRuntimeExecutionBindings::organization_id().expression(),
            DeploymentRuntimeExecutionBindings::project_id().expression(),
            DeploymentRuntimeExecutionBindings::environment_id().expression(),
            DeploymentRuntimeExecutionBindings::workload_id().expression(),
            DeploymentRuntimeExecutionBindings::workload_revision_id().expression(),
            DeploymentRuntimeExecutionBindings::node_pool_id().expression(),
            DeploymentRuntimeExecutionBindings::runtime_class().expression(),
            DeploymentRuntimeExecutionBindings::isolation_level().expression(),
            DeploymentRuntimeExecutionBindings::semantics_profile_digest().expression(),
            DeploymentRuntimeExecutionBindings::identity_attachment_digest().expression(),
            DeploymentRuntimeExecutionBindings::authorized_at().expression(),
            DeploymentRuntimeExecutionBindings::admitted_at().expression(),
            DeploymentRuntimeExecutionBindings::binding_digest().expression(),
        ]
    }
}

struct BindingRow {
    binding_schema: String,
    deployment_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    node_pool_id: Option<Uuid>,
    runtime_class: Option<String>,
    isolation_level: Option<String>,
    semantics_profile_digest: Option<String>,
    identity_attachment_digest: Option<String>,
    authorized_at: Option<DateTime<Utc>>,
    admitted_at: DateTime<Utc>,
    binding_digest: String,
}

impl FromRow for BindingRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            binding_schema: decode(row, 0)?,
            deployment_id: decode(row, 1)?,
            organization_id: decode(row, 2)?,
            project_id: decode(row, 3)?,
            environment_id: decode(row, 4)?,
            workload_id: decode(row, 5)?,
            workload_revision_id: decode(row, 6)?,
            node_pool_id: decode(row, 7)?,
            runtime_class: decode(row, 8)?,
            isolation_level: decode(row, 9)?,
            semantics_profile_digest: decode(row, 10)?,
            identity_attachment_digest: decode(row, 11)?,
            authorized_at: decode(row, 12)?,
            admitted_at: decode(row, 13)?,
            binding_digest: decode(row, 14)?,
        })
    }
}

impl BindingRow {
    fn binding(self) -> Result<DeploymentRuntimeExecutionBinding, PostgresPersistenceError> {
        let execution = match (
            self.runtime_class.as_deref(),
            self.isolation_level.as_deref(),
            self.semantics_profile_digest,
            self.identity_attachment_digest,
        ) {
            (Some(runtime_class), Some(isolation), Some(semantics), Some(attachment)) => Some(
                WorkloadRuntimeExecutionBinding::new(
                    parse_runtime_class(runtime_class)?,
                    parse_isolation(isolation)?,
                    Sha256Digest::parse(semantics).map_err(PostgresPersistenceError::Invariant)?,
                    Sha256Digest::parse(attachment).map_err(PostgresPersistenceError::Invariant)?,
                )
                .map_err(PostgresPersistenceError::Invariant)?,
            ),
            (None, None, None, None) => None,
            _ => {
                return Err(PostgresPersistenceError::Invariant(
                    "stored Deployment Runtime admission has a partial execution binding".into(),
                ))
            }
        };
        DeploymentRuntimeExecutionBinding::restore(
            self.binding_schema,
            DeploymentId::from_uuid(self.deployment_id),
            OrganizationId::from_uuid(self.organization_id),
            ProjectId::from_uuid(self.project_id),
            EnvironmentId::from_uuid(self.environment_id),
            WorkloadId::from_uuid(self.workload_id),
            WorkloadRevisionId::from_uuid(self.workload_revision_id),
            self.node_pool_id.map(NodePoolId::from_uuid),
            execution,
            self.authorized_at,
            self.admitted_at,
            Sha256Digest::parse(self.binding_digest)
                .map_err(PostgresPersistenceError::Invariant)?,
        )
        .map_err(PostgresPersistenceError::Invariant)
    }
}

pub(super) async fn bind(
    executor: &PostgresExecutor,
    binding: DeploymentRuntimeExecutionBinding,
) -> Result<IdempotentWrite<DeploymentRuntimeExecutionBinding>, RepositoryError> {
    binding.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                // Deployment transitions and placement-group scheduling use
                // this same Deployment -> Workload Control lock order. The
                // migration trigger repeats it as a database-level guard.
                let deployment =
                    queries::deployment_in_transaction(transaction, binding.deployment_id(), true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                if let Some(existing) = load_in_transaction(
                    transaction,
                    binding.organization_id(),
                    binding.deployment_id(),
                )
                .await?
                {
                    return if existing == binding {
                        Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        })
                    } else {
                        Err(RepositoryError::IdempotencyConflict.into())
                    };
                }
                let workload = queries::workload_in_transaction(
                    transaction,
                    binding.organization_id(),
                    binding.workload_id(),
                    false,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "Deployment Runtime binding references a missing Workload".into(),
                    )
                })?;
                let revision = queries::revision_in_transaction(
                    transaction,
                    binding.organization_id(),
                    binding.workload_revision_id(),
                    false,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "Deployment Runtime binding references a missing revision".into(),
                    )
                })?;
                let control = replicas::control_for_update(
                    transaction,
                    binding.organization_id(),
                    binding.workload_id(),
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "Deployment Runtime binding references a missing control record".into(),
                    )
                })?;
                binding
                    .validate_admission(&deployment, &workload, &revision, &control)
                    .map_err(RepositoryError::Conflict)?;
                let rows = execute(
                    transaction,
                    insert_into::<DeploymentRuntimeExecutionBindings>()
                        .value(
                            DeploymentRuntimeExecutionBindings::deployment_id(),
                            binding.deployment_id().as_uuid(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::organization_id(),
                            binding.organization_id().as_uuid(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::project_id(),
                            binding.project_id().as_uuid(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::environment_id(),
                            binding.environment_id().as_uuid(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::workload_id(),
                            binding.workload_id().as_uuid(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::workload_revision_id(),
                            binding.workload_revision_id().as_uuid(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::node_pool_id(),
                            binding.node_pool_id().map(NodePoolId::as_uuid),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::binding_schema(),
                            binding.schema(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::runtime_class(),
                            binding.execution().map(|execution| {
                                runtime_class_name(execution.runtime_class()).to_owned()
                            }),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::isolation_level(),
                            binding
                                .execution()
                                .map(|execution| isolation_name(execution.isolation()).to_owned()),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::semantics_profile_digest(),
                            binding.execution().map(|execution| {
                                execution.semantics_profile_digest().as_str().to_owned()
                            }),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::identity_attachment_digest(),
                            binding.execution().map(|execution| {
                                execution.identity_attachment_digest().as_str().to_owned()
                            }),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::authorized_at(),
                            binding.authorized_at(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::admitted_at(),
                            binding.admitted_at(),
                        )
                        .value(
                            DeploymentRuntimeExecutionBindings::binding_digest(),
                            binding.binding_digest().as_str(),
                        ),
                )
                .await?;
                require_one_row("Deployment Runtime execution binding", rows)?;
                Ok(IdempotentWrite {
                    value: binding,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Option<DeploymentRuntimeExecutionBinding>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                load_in_transaction(transaction, organization_id, deployment_id).await
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn load_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Option<DeploymentRuntimeExecutionBinding>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<DeploymentRuntimeExecutionBindings>()
            .select(BindingSelection)
            .filter(
                DeploymentRuntimeExecutionBindings::organization_id().eq(organization_id.as_uuid()),
            )
            .filter(
                DeploymentRuntimeExecutionBindings::deployment_id().eq(deployment_id.as_uuid()),
            ),
    )
    .await?
    .map(BindingRow::binding)
    .transpose()
}

fn runtime_class_name(value: RuntimeUnitClass) -> &'static str {
    match value {
        RuntimeUnitClass::Task => "task",
        RuntimeUnitClass::Service => "service",
    }
}

fn parse_runtime_class(value: &str) -> Result<RuntimeUnitClass, PostgresPersistenceError> {
    match value {
        "task" => Ok(RuntimeUnitClass::Task),
        "service" => Ok(RuntimeUnitClass::Service),
        _ => Err(PostgresPersistenceError::Invariant(
            "stored Deployment Runtime class is invalid".into(),
        )),
    }
}

fn isolation_name(value: IsolationLevel) -> &'static str {
    match value {
        IsolationLevel::Process => "process",
        IsolationLevel::Container => "container",
        IsolationLevel::Sandbox => "sandbox",
        IsolationLevel::Confidential => "confidential",
    }
}

fn parse_isolation(value: &str) -> Result<IsolationLevel, PostgresPersistenceError> {
    match value {
        "process" => Ok(IsolationLevel::Process),
        "container" => Ok(IsolationLevel::Container),
        "sandbox" => Ok(IsolationLevel::Sandbox),
        "confidential" => Ok(IsolationLevel::Confidential),
        _ => Err(PostgresPersistenceError::Invariant(
            "stored Deployment Runtime isolation is invalid".into(),
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
