use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_memberships::{
    load_active_membership_for_update, load_membership_for_update, lock_membership_set,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, store_audit, store_idempotency, store_outbox, transaction_error,
    AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::identity::domain::events::ResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateResourceGrantWrite, IResourceGrantRepository, RevokeResourceGrantWrite,
    MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::{MembershipRole, ResourceGrantScope};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, MembershipId, NodeId, OrganizationId, ProjectId,
    RepositoryError, ResourceGrantId,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct ResourceGrantRow {
    id: Uuid,
    organization_id: Uuid,
    membership_id: Uuid,
    scope_kind: String,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    node_id: Option<Uuid>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for ResourceGrantRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            membership_id: decode_column(row, 2)?,
            scope_kind: decode_column(row, 3)?,
            project_id: decode_column(row, 4)?,
            environment_id: decode_column(row, 5)?,
            node_id: decode_column(row, 6)?,
            aggregate_version: decode_column(row, 7)?,
            created_at: decode_column(row, 8)?,
            updated_at: decode_column(row, 9)?,
            revoked_at: decode_column(row, 10)?,
        })
    }
}

const SELECT_RESOURCE_GRANTS: &str = "select id, organization_id, membership_id, scope_kind, project_id, environment_id, node_id, aggregate_version, created_at, updated_at, revoked_at from resource_grants";

fn decode_resource_grant(row: ResourceGrantRow) -> Result<ResourceGrant, RepositoryError> {
    let scope = match (
        row.scope_kind.as_str(),
        row.project_id,
        row.environment_id,
        row.node_id,
    ) {
        ("project", Some(project_id), None, None) => ResourceGrantScope::Project {
            project_id: ProjectId::from_uuid(project_id),
        },
        ("environment", Some(project_id), Some(environment_id), None) => {
            ResourceGrantScope::Environment {
                project_id: ProjectId::from_uuid(project_id),
                environment_id: EnvironmentId::from_uuid(environment_id),
            }
        }
        ("node", None, None, Some(node_id)) => ResourceGrantScope::Node {
            node_id: NodeId::from_uuid(node_id),
        },
        _ => {
            return Err(RepositoryError::Storage(
                "stored Resource Grant scope is invalid".into(),
            ))
        }
    };
    if row.aggregate_version == 0
        || row.updated_at < row.created_at
        || row
            .revoked_at
            .is_some_and(|revoked_at| revoked_at != row.updated_at)
    {
        return Err(RepositoryError::Storage(
            "stored Resource Grant lifecycle is invalid".into(),
        ));
    }
    Ok(ResourceGrant {
        id: ResourceGrantId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        membership_id: MembershipId::from_uuid(row.membership_id),
        scope,
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revoked_at: row.revoked_at,
    })
}

async fn load_resource_grant_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    resource_grant_id: ResourceGrantId,
) -> Result<Option<ResourceGrant>, PostgresPersistenceError> {
    fetch_optional::<ResourceGrantRow, _>(
        transaction,
        sql_query::<ResourceGrantRow>(SELECT_RESOURCE_GRANTS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(resource_grant_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_resource_grant)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_active_resource_grants_for_membership(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    membership_id: MembershipId,
) -> Result<Vec<ResourceGrant>, PostgresPersistenceError> {
    fetch_all::<ResourceGrantRow, _>(
        transaction,
        sql_query::<ResourceGrantRow>(SELECT_RESOURCE_GRANTS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and membership_id = ")
            .bind(membership_id.as_uuid())
            .append(" and revoked_at is null order by id asc"),
    )
    .await?
    .into_iter()
    .map(decode_resource_grant)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

async fn insert_resource_grant(
    transaction: &a3s_orm::PostgresTransaction,
    grant: &ResourceGrant,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into resource_grants (id, organization_id, membership_id, scope_kind, project_id, environment_id, node_id, aggregate_version, created_at, updated_at, revoked_at) values (",
        )
        .bind(grant.id.as_uuid())
        .append(", ")
        .bind(grant.organization_id.as_uuid())
        .append(", ")
        .bind(grant.membership_id.as_uuid())
        .append(", ")
        .bind(grant.scope.kind())
        .append(", ")
        .bind(grant.scope.project_id().map(|id| id.as_uuid()))
        .append(", ")
        .bind(grant.scope.environment_id().map(|id| id.as_uuid()))
        .append(", ")
        .bind(grant.scope.node_id().map(|id| id.as_uuid()))
        .append(", ")
        .bind(grant.aggregate_version)
        .append(", ")
        .bind(grant.created_at)
        .append(", ")
        .bind(grant.updated_at)
        .append(", ")
        .bind(grant.revoked_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating Resource Grant affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_resource_grant_audit(
    transaction: &a3s_orm::PostgresTransaction,
    grant: &ResourceGrant,
    actor_principal_id: crate::modules::shared_kernel::domain::PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: grant.id.as_uuid(),
            occurred_at: grant.updated_at,
            request_id,
            scope: grant.scope.project_id().map_or_else(
                || AuditWrite::organization_scope(grant.organization_id.as_uuid()),
                |project_id| {
                    AuditWrite::resource_scope(
                        grant.organization_id.as_uuid(),
                        project_id,
                        grant.scope.environment_id(),
                    )
                },
            ),
            details: serde_json::json!({
                "membershipId": grant.membership_id,
                "scopeKind": grant.scope.kind(),
                "projectId": grant.scope.project_id(),
                "environmentId": grant.scope.environment_id(),
                "nodeId": grant.scope.node_id(),
                "aggregateVersion": grant.aggregate_version,
                "revokedAt": grant.revoked_at,
            }),
        },
    )
    .await
}

#[async_trait]
impl IResourceGrantRepository for PostgresIdentityRepository {
    async fn create_resource_grant(
        &self,
        write: CreateResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_membership_set(transaction, write.grant.organization_id).await?;
                    let actor = load_active_membership_for_update(
                        transaction,
                        write.grant.organization_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    let target = load_membership_for_update(
                        transaction,
                        write.grant.organization_id,
                        write.grant.membership_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    MembershipAdministration::authorize(
                        actor.as_ref(),
                        write.grant.organization_id,
                        target.role,
                        None,
                    )
                    .map_err(RepositoryError::Forbidden)?;
                    if let Some(replayed) =
                        idempotency_replay::<ResourceGrant>(transaction, &write.idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    if !target.is_active() {
                        return Err(RepositoryError::Conflict(
                            "Resource Grants require an active membership".into(),
                        )
                        .into());
                    }
                    if target.role != MembershipRole::Restricted {
                        return Err(RepositoryError::Conflict(
                            "Resource Grants require a restricted membership".into(),
                        )
                        .into());
                    }
                    if !write.grant.is_active() || write.grant.aggregate_version != 1 {
                        return Err(PostgresPersistenceError::Invariant(
                            "new Resource Grant is not at its initial lifecycle state".into(),
                        ));
                    }
                    let active_count = fetch_optional::<i64, _>(
                        transaction,
                        sql_query::<i64>(
                            "select count(*) from resource_grants where organization_id = ",
                        )
                        .bind(write.grant.organization_id.as_uuid())
                        .append(" and membership_id = ")
                        .bind(write.grant.membership_id.as_uuid())
                        .append(" and revoked_at is null"),
                    )
                    .await?
                    .unwrap_or_default();
                    if active_count >= i64::from(MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP) {
                        return Err(RepositoryError::Conflict(format!(
                            "membership cannot have more than {MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP} active Resource Grants"
                        ))
                        .into());
                    }
                    match insert_resource_grant(transaction, &write.grant).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "an active Resource Grant already covers this exact scope".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_resource_grant_audit(
                        transaction,
                        &write.grant,
                        write.actor_principal_id,
                        "identity.resource-grant.created",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.grant).await?;
                    Ok(IdempotentWrite {
                        value: write.grant,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<Option<ResourceGrant>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ResourceGrantRow>(SELECT_RESOURCE_GRANTS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(resource_grant_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_resource_grant)
            .transpose()
    }

    async fn list_resource_grants(
        &self,
        organization_id: OrganizationId,
        membership_id: Option<MembershipId>,
    ) -> Result<Vec<ResourceGrant>, RepositoryError> {
        let mut query = sql_query::<ResourceGrantRow>(SELECT_RESOURCE_GRANTS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid());
        if let Some(membership_id) = membership_id {
            query = query
                .append(" and membership_id = ")
                .bind(membership_id.as_uuid());
        }
        query = query.append(" order by created_at asc, id asc");
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(query)
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_resource_grant)
            .collect()
    }

    async fn list_active_resource_grants_for_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Vec<ResourceGrant>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<ResourceGrantRow>(SELECT_RESOURCE_GRANTS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and membership_id = ")
                    .bind(membership_id.as_uuid())
                    .append(" and revoked_at is null order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_resource_grant)
            .collect()
    }

    async fn revoke_resource_grant(
        &self,
        write: RevokeResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_membership_set(transaction, write.organization_id).await?;
                    let actor = load_active_membership_for_update(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    let mut grant = load_resource_grant_for_update(
                        transaction,
                        write.organization_id,
                        write.resource_grant_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    let target = load_membership_for_update(
                        transaction,
                        write.organization_id,
                        grant.membership_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "Resource Grant membership is missing".into(),
                        )
                    })?;
                    MembershipAdministration::authorize(
                        actor.as_ref(),
                        write.organization_id,
                        target.role,
                        None,
                    )
                    .map_err(RepositoryError::Forbidden)?;
                    if let Some(replayed) =
                        idempotency_replay::<ResourceGrant>(transaction, &write.idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    if grant.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "Resource Grant changed before revocation".into(),
                        )
                        .into());
                    }
                    let changed = grant.revoke(write.revoked_at);
                    if changed {
                        let rows = execute(
                            transaction,
                            sql_query::<()>("update resource_grants set aggregate_version = ")
                                .bind(grant.aggregate_version)
                                .append(", updated_at = ")
                                .bind(grant.updated_at)
                                .append(", revoked_at = ")
                                .bind(grant.revoked_at)
                                .append(" where organization_id = ")
                                .bind(grant.organization_id.as_uuid())
                                .append(" and id = ")
                                .bind(grant.id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(write.expected_version)
                                .append(" and revoked_at is null"),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(RepositoryError::Conflict(
                                "Resource Grant changed while it was being revoked".into(),
                            )
                            .into());
                        }
                        let event = ResourceGrantChanged::revoked(&grant, write.request_id)?;
                        store_outbox(transaction, &event).await?;
                        store_resource_grant_audit(
                            transaction,
                            &grant,
                            write.actor_principal_id,
                            "identity.resource-grant.revoked",
                            write.request_id,
                        )
                        .await?;
                    }
                    store_idempotency(transaction, &write.idempotency, &grant).await?;
                    Ok(IdempotentWrite {
                        value: grant,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}
