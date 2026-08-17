use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_audit, store_idempotency, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::durable_cells::domain::{
    CreateDurableCellDeploymentWrite, DurableCellDeployment, DurableCellProjectionIdentity,
    DurableCellProviderBinding, DurableCellStorageBinding, IDurableCellDeploymentRepository,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    IdempotencyRequest, IdempotentWrite, OperationId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest, StorageNamespaceId, WorkloadId, WorkloadRevisionId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_DEPLOYMENTS: &str = "select organization_id, project_id, environment_id, application_id, application_revision_id, application_revision_number, application_definition_digest, storage_namespace_id, credential_binding_generation, credential_binding_digest, storage_provider_profile_digest, storage_provider_profile_acl, retention_policy_digest, workload_id, workload_revision_id, workload_generation, service_profile_digest, service_template_digest, provider_artifact_digest, deployment_id, operation_id, placement_policy_digest, requested_by, request_id, requested_at from durable_cell_deployments";

#[derive(Clone)]
pub struct PostgresDurableCellDeploymentRepository {
    executor: PostgresExecutor,
}

impl PostgresDurableCellDeploymentRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IDurableCellDeploymentRepository for PostgresDurableCellDeploymentRepository {
    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    Ok(
                        idempotency_replay::<DurableCellDeployment>(transaction, &idempotency)
                            .await?
                            .map(|replay| replay.value),
                    )
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create(
        &self,
        write: CreateDurableCellDeploymentWrite,
    ) -> Result<IdempotentWrite<DurableCellDeployment>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) = idempotency_replay::<DurableCellDeployment>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: replay.value,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let deployment = &write.deployment;
                    let projection = &deployment.projection;
                    let storage = &deployment.storage;
                    let provider = &deployment.provider;
                    let inserted = execute(
                        transaction,
                        sql_query::<()>("insert into durable_cell_deployments (organization_id, project_id, environment_id, application_id, application_revision_id, application_revision_number, application_definition_digest, storage_namespace_id, credential_binding_generation, credential_binding_digest, storage_provider_profile_digest, storage_provider_profile_acl, retention_policy_digest, workload_id, workload_revision_id, workload_generation, service_profile_digest, service_template_digest, provider_artifact_digest, deployment_id, operation_id, placement_policy_digest, requested_by, request_id, requested_at) values (")
                            .bind(projection.organization_id.as_uuid())
                            .append(", ")
                            .bind(projection.project_id.as_uuid())
                            .append(", ")
                            .bind(projection.environment_id.as_uuid())
                            .append(", ")
                            .bind(projection.application_id.as_uuid())
                            .append(", ")
                            .bind(projection.application_revision_id.as_uuid())
                            .append(", ")
                            .bind(projection.application_revision_number)
                            .append(", ")
                            .bind(projection.application_definition_digest.as_str())
                            .append(", ")
                            .bind(projection.storage_namespace_id.as_uuid())
                            .append(", ")
                            .bind(storage.credential_binding_generation)
                            .append(", ")
                            .bind(storage.credential_binding_digest.as_str())
                            .append(", ")
                            .bind(storage.provider_profile_digest.as_str())
                            .append(", ")
                            .bind(deployment.storage_provider_profile_acl.as_deref())
                            .append(", ")
                            .bind(storage.retention_policy_digest.as_str())
                            .append(", ")
                            .bind(projection.workload_id.as_uuid())
                            .append(", ")
                            .bind(projection.workload_revision_id.as_uuid())
                            .append(", ")
                            .bind(provider.workload_generation)
                            .append(", ")
                            .bind(provider.service_profile_digest.as_str())
                            .append(", ")
                            .bind(provider.service_template_digest.as_str())
                            .append(", ")
                            .bind(provider.provider_artifact_digest.as_str())
                            .append(", ")
                            .bind(projection.deployment_id.as_uuid())
                            .append(", ")
                            .bind(projection.operation_id.as_uuid())
                            .append(", ")
                            .bind(deployment.placement_policy_digest.as_str())
                            .append(", ")
                            .bind(deployment.requested_by.as_uuid())
                            .append(", ")
                            .bind(deployment.request_id)
                            .append(", ")
                            .bind(deployment.requested_at)
                            .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(rows) => require_one_row("Durable Cell deployment correlation", rows)?,
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Durable Cell deployment correlation identity is already in use"
                                    .into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: projection.organization_id.as_uuid(),
                            actor_id: Some(deployment.requested_by.as_uuid()),
                            action: "durable-cell.deployment.requested",
                            aggregate_id: projection.application_id.as_uuid(),
                            occurred_at: deployment.requested_at,
                            request_id: deployment.request_id,
                            details: serde_json::json!({
                                "projectId": projection.project_id,
                                "environmentId": projection.environment_id,
                                "applicationRevisionId": projection.application_revision_id,
                                "applicationRevisionNumber": projection.application_revision_number,
                                "applicationDefinitionDigest": projection.application_definition_digest,
                                "storageNamespaceId": projection.storage_namespace_id,
                                "credentialBindingDigest": storage.credential_binding_digest,
                                "retentionPolicyDigest": storage.retention_policy_digest,
                                "workloadId": projection.workload_id,
                                "workloadRevisionId": projection.workload_revision_id,
                                "workloadGeneration": provider.workload_generation,
                                "deploymentId": projection.deployment_id,
                                "operationId": projection.operation_id,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, deployment).await?;
                    Ok(IdempotentWrite {
                        value: write.deployment,
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
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        application_revision_id: DurableCellApplicationRevisionId,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                deployment_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and application_revision_id = ")
                    .bind(application_revision_id.as_uuid()),
            )
            .await
            .map_err(storage_error)?
            .map(decode_deployment)
            .transpose()
    }

    async fn find_by_workload_revision(
        &self,
        organization_id: OrganizationId,
        workload_revision_id: WorkloadRevisionId,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                deployment_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and workload_revision_id = ")
                    .bind(workload_revision_id.as_uuid()),
            )
            .await
            .map_err(storage_error)?
            .map(decode_deployment)
            .transpose()
    }
}

struct DurableCellDeploymentRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    application_id: Uuid,
    application_revision_id: Uuid,
    application_revision_number: u64,
    application_definition_digest: String,
    storage_namespace_id: Uuid,
    credential_binding_generation: u64,
    credential_binding_digest: String,
    storage_provider_profile_digest: String,
    storage_provider_profile_acl: Option<String>,
    retention_policy_digest: String,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    workload_generation: u64,
    service_profile_digest: String,
    service_template_digest: String,
    provider_artifact_digest: String,
    deployment_id: Uuid,
    operation_id: Uuid,
    placement_policy_digest: String,
    requested_by: Uuid,
    request_id: Uuid,
    requested_at: DateTime<Utc>,
}

impl FromRow for DurableCellDeploymentRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            application_id: decode(row, 3)?,
            application_revision_id: decode(row, 4)?,
            application_revision_number: decode(row, 5)?,
            application_definition_digest: decode(row, 6)?,
            storage_namespace_id: decode(row, 7)?,
            credential_binding_generation: decode(row, 8)?,
            credential_binding_digest: decode(row, 9)?,
            storage_provider_profile_digest: decode(row, 10)?,
            storage_provider_profile_acl: decode(row, 11)?,
            retention_policy_digest: decode(row, 12)?,
            workload_id: decode(row, 13)?,
            workload_revision_id: decode(row, 14)?,
            workload_generation: decode(row, 15)?,
            service_profile_digest: decode(row, 16)?,
            service_template_digest: decode(row, 17)?,
            provider_artifact_digest: decode(row, 18)?,
            deployment_id: decode(row, 19)?,
            operation_id: decode(row, 20)?,
            placement_policy_digest: decode(row, 21)?,
            requested_by: decode(row, 22)?,
            request_id: decode(row, 23)?,
            requested_at: decode(row, 24)?,
        })
    }
}

fn deployment_select() -> a3s_orm::SqlQuery<DurableCellDeploymentRow> {
    sql_query::<DurableCellDeploymentRow>(SELECT_DEPLOYMENTS)
}

fn decode_deployment(
    row: DurableCellDeploymentRow,
) -> Result<DurableCellDeployment, RepositoryError> {
    let application_definition_digest = digest(
        row.application_definition_digest,
        "Durable Cell application definition digest",
    )?;
    DurableCellDeployment {
        projection: DurableCellProjectionIdentity {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            project_id: ProjectId::from_uuid(row.project_id),
            environment_id: EnvironmentId::from_uuid(row.environment_id),
            application_id: DurableCellApplicationId::from_uuid(row.application_id),
            application_revision_id: DurableCellApplicationRevisionId::from_uuid(
                row.application_revision_id,
            ),
            application_revision_number: row.application_revision_number,
            application_definition_digest: application_definition_digest.clone(),
            storage_namespace_id: StorageNamespaceId::from_uuid(row.storage_namespace_id),
            workload_id: WorkloadId::from_uuid(row.workload_id),
            workload_revision_id: WorkloadRevisionId::from_uuid(row.workload_revision_id),
            deployment_id: DeploymentId::from_uuid(row.deployment_id),
            operation_id: OperationId::from_uuid(row.operation_id),
        },
        storage: DurableCellStorageBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            project_id: ProjectId::from_uuid(row.project_id),
            environment_id: EnvironmentId::from_uuid(row.environment_id),
            application_id: DurableCellApplicationId::from_uuid(row.application_id),
            application_revision_id: DurableCellApplicationRevisionId::from_uuid(
                row.application_revision_id,
            ),
            application_revision_number: row.application_revision_number,
            application_definition_digest: application_definition_digest.clone(),
            storage_namespace_id: StorageNamespaceId::from_uuid(row.storage_namespace_id),
            credential_binding_generation: row.credential_binding_generation,
            credential_binding_digest: digest(
                row.credential_binding_digest,
                "Durable Cell credential binding digest",
            )?,
            provider_profile_digest: digest(
                row.storage_provider_profile_digest,
                "Durable Cell storage provider profile digest",
            )?,
            retention_policy_digest: digest(
                row.retention_policy_digest,
                "Durable Cell retention policy digest",
            )?,
        },
        storage_provider_profile_acl: row.storage_provider_profile_acl,
        provider: DurableCellProviderBinding {
            application_id: DurableCellApplicationId::from_uuid(row.application_id),
            application_revision_id: DurableCellApplicationRevisionId::from_uuid(
                row.application_revision_id,
            ),
            application_revision_number: row.application_revision_number,
            application_definition_digest,
            workload_id: WorkloadId::from_uuid(row.workload_id),
            workload_revision_id: WorkloadRevisionId::from_uuid(row.workload_revision_id),
            workload_generation: row.workload_generation,
            service_profile_digest: digest(
                row.service_profile_digest,
                "Durable Cell Service profile digest",
            )?,
            service_template_digest: digest(
                row.service_template_digest,
                "Durable Cell Service template digest",
            )?,
            provider_artifact_digest: digest(
                row.provider_artifact_digest,
                "Durable Cell provider artifact digest",
            )?,
        },
        placement_policy_digest: digest(
            row.placement_policy_digest,
            "Durable Cell placement policy digest",
        )?,
        requested_by: PrincipalId::from_uuid(row.requested_by),
        request_id: row.request_id,
        requested_at: row.requested_at,
    }
    .restore()
    .map_err(stored("Durable Cell deployment correlation"))
}

fn digest(value: String, label: &'static str) -> Result<Sha256Digest, RepositoryError> {
    Sha256Digest::parse(value).map_err(stored(label))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn storage_error(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
