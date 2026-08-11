use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::{
    CreatePluginRegistryWrite, IPluginRegistryRepository,
};
use crate::modules::plugins::domain::services::{
    IPluginRegistryEnrollmentAuthorizer, PluginRegistryEnrollmentAuthorizationError,
};
use crate::modules::plugins::domain::value_objects::{
    PluginRegistryEndpoint, PluginRegistryState, PluginTrustRoot, PluginTrustRootObjectRef,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, PluginRegistryId, PrincipalId, RepositoryError, ResourceName,
    Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct PluginRegistryRow {
    organization_id: Uuid,
    id: Uuid,
    name: String,
    endpoint: String,
    root_object_ref: String,
    root_sha256: String,
    root_version: u64,
    state: String,
    aggregate_version: u64,
    last_actor_id: Uuid,
    last_request_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for PluginRegistryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            name: decode(row, 2)?,
            endpoint: decode(row, 3)?,
            root_object_ref: decode(row, 4)?,
            root_sha256: decode(row, 5)?,
            root_version: decode(row, 6)?,
            state: decode(row, 7)?,
            aggregate_version: decode(row, 8)?,
            last_actor_id: decode(row, 9)?,
            last_request_id: decode(row, 10)?,
            created_at: decode(row, 11)?,
            updated_at: decode(row, 12)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresPluginRegistryRepository {
    executor: PostgresExecutor,
}

impl PostgresPluginRegistryRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IPluginRegistryRepository for PostgresPluginRegistryRepository {
    async fn create(
        &self,
        write: CreatePluginRegistryWrite,
    ) -> Result<IdempotentWrite<PluginRegistry>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let CreatePluginRegistryWrite {
            registry,
            event,
            actor_id,
            request_id,
            idempotency,
        } = write;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let actor_is_active_human = fetch_optional::<i32, _>(
                        transaction,
                        active_human_member_query(registry.organization_id, actor_id),
                    )
                    .await?
                    .is_some();
                    if !actor_is_active_human {
                        return Err(RepositoryError::Forbidden(
                            "plugin registry enrollment requires an active human organization member"
                                .into(),
                        )
                        .into());
                    }
                    if let Some(replayed) =
                        idempotency_replay::<PluginRegistry>(transaction, &idempotency).await?
                    {
                        CreatePluginRegistryWrite::validate_replay(&registry, &replayed.value)
                            .map_err(RepositoryError::Storage)?;
                        return Ok(replayed);
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into plugin_registries (organization_id, id, name, name_key, endpoint, root_object_ref, root_sha256, root_version, state, aggregate_version, last_actor_id, last_request_id, created_at, updated_at) values (",
                        )
                        .bind(registry.organization_id.as_uuid())
                        .append(", ")
                        .bind(registry.id.as_uuid())
                        .append(", ")
                        .bind(registry.name.as_str())
                        .append(", ")
                        .bind(registry.name.key())
                        .append(", ")
                        .bind(registry.endpoint.as_str())
                        .append(", ")
                        .bind(registry.trust_root.object_ref().as_str())
                        .append(", ")
                        .bind(registry.trust_root.digest().as_str())
                        .append(", ")
                        .bind(registry.trust_root.version())
                        .append(", ")
                        .bind(registry.state.as_str())
                        .append(", ")
                        .bind(registry.aggregate_version)
                        .append(", ")
                        .bind(registry.last_actor_id.as_uuid())
                        .append(", ")
                        .bind(registry.last_request_id)
                        .append(", ")
                        .bind(registry.created_at)
                        .append(", ")
                        .bind(registry.updated_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating plugin registry affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "plugin registry name or endpoint is already enrolled".into(),
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
                            organization_id: registry.organization_id.as_uuid(),
                            actor_id: Some(actor_id.as_uuid()),
                            action: "plugins.registry.enrolled",
                            aggregate_id: registry.id.as_uuid(),
                            occurred_at: registry.created_at,
                            request_id,
                            details: serde_json::json!({
                                "endpoint": registry.endpoint.as_str(),
                                "rootObjectRef": registry.trust_root.object_ref().as_str(),
                                "rootSha256": registry.trust_root.digest().as_str(),
                                "rootVersion": registry.trust_root.version(),
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &idempotency, &registry).await?;
                    Ok(IdempotentWrite {
                        value: registry,
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
        registry_id: PluginRegistryId,
    ) -> Result<Option<PluginRegistry>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                plugin_registry_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(registry_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(plugin_registry_from_row)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PluginRegistry>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                plugin_registry_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(plugin_registry_from_row)
            .collect()
    }
}

#[async_trait]
impl IPluginRegistryEnrollmentAuthorizer for PostgresPluginRegistryRepository {
    async fn authorize_enrollment(
        &self,
        organization_id: OrganizationId,
        actor_id: PrincipalId,
    ) -> Result<(), PluginRegistryEnrollmentAuthorizationError> {
        let authorized = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(active_human_member_query(organization_id, actor_id))
            .await
            .map_err(|error| {
                PluginRegistryEnrollmentAuthorizationError::Unavailable(error.to_string())
            })?
            .is_some();
        if authorized {
            Ok(())
        } else {
            Err(PluginRegistryEnrollmentAuthorizationError::Forbidden)
        }
    }
}

fn active_human_member_query(
    organization_id: OrganizationId,
    actor_id: PrincipalId,
) -> a3s_orm::SqlQuery<i32> {
    sql_query::<i32>(
        "select 1 from identity_principals p join organization_memberships m on m.principal_id = p.id where p.id = ",
    )
    .bind(actor_id.as_uuid())
    .append(" and p.kind = 'human' and p.disabled_at is null and m.organization_id = ")
    .bind(organization_id.as_uuid())
    .append(" and m.revoked_at is null")
}

fn plugin_registry_select() -> a3s_orm::SqlQuery<PluginRegistryRow> {
    sql_query::<PluginRegistryRow>(
        "select organization_id, id, name, endpoint, root_object_ref, root_sha256, root_version, state, aggregate_version, last_actor_id, last_request_id, created_at, updated_at from plugin_registries",
    )
}

fn plugin_registry_from_row(row: PluginRegistryRow) -> Result<PluginRegistry, RepositoryError> {
    let PluginRegistryRow {
        organization_id,
        id,
        name: stored_name,
        endpoint: stored_endpoint,
        root_object_ref: stored_root_object_ref,
        root_sha256: stored_root_sha256,
        root_version,
        state: stored_state,
        aggregate_version,
        last_actor_id,
        last_request_id,
        created_at,
        updated_at,
    } = row;
    let name = ResourceName::parse(&stored_name).map_err(|error| stored_error("name", error))?;
    if name.as_str() != stored_name {
        return Err(stored_error("name", "value is not canonical"));
    }
    let endpoint = PluginRegistryEndpoint::parse(&stored_endpoint)
        .map_err(|error| stored_error("endpoint", error))?;
    if endpoint.as_str() != stored_endpoint {
        return Err(stored_error("endpoint", "value is not canonical"));
    }
    let digest = Sha256Digest::parse(stored_root_sha256)
        .map_err(|error| stored_error("root digest", error))?;
    let object_ref = PluginTrustRootObjectRef::parse(stored_root_object_ref)
        .map_err(|error| stored_error("root object reference", error))?;
    let trust_root = PluginTrustRoot::new(object_ref, digest, root_version)
        .map_err(|error| stored_error("trust root", error))?;
    let state =
        PluginRegistryState::parse(&stored_state).map_err(|error| stored_error("state", error))?;
    let registry = PluginRegistry {
        organization_id: OrganizationId::from_uuid(organization_id),
        id: PluginRegistryId::from_uuid(id),
        name,
        endpoint,
        trust_root,
        state,
        aggregate_version,
        last_actor_id: PrincipalId::from_uuid(last_actor_id),
        last_request_id,
        created_at,
        updated_at,
    };
    registry
        .validate()
        .map_err(|error| stored_error("record", error))?;
    Ok(registry)
}

fn stored_error(field: &str, error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(format!(
        "stored plugin registry {field} is invalid: {error}"
    ))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
