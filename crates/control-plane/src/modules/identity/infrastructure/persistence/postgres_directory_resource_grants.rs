use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_memberships::{load_active_membership_for_update, lock_membership_set};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::identity::domain::events::DirectoryResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateDirectoryResourceGrantWrite, IDirectoryResourceGrantRepository,
    RevokeDirectoryResourceGrantWrite, MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::{
    DirectoryGrantSubjectKind, DirectoryGrantSubjectRef, MembershipRole, OidcIssuer,
    ResourceGrantScope,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, EnvironmentId, IdempotentWrite, NodeId, OrganizationId, ProjectId,
    RepositoryError, ResourceGrantId,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct DirectoryResourceGrantRow {
    id: Uuid,
    organization_id: Uuid,
    subject_kind: String,
    directory_issuer: String,
    directory_subject_id: Uuid,
    scope_kind: String,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    application_id: Option<Uuid>,
    node_id: Option<Uuid>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for DirectoryResourceGrantRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            subject_kind: decode_column(row, 2)?,
            directory_issuer: decode_column(row, 3)?,
            directory_subject_id: decode_column(row, 4)?,
            scope_kind: decode_column(row, 5)?,
            project_id: decode_column(row, 6)?,
            environment_id: decode_column(row, 7)?,
            application_id: decode_column(row, 8)?,
            node_id: decode_column(row, 9)?,
            aggregate_version: decode_column(row, 10)?,
            created_at: decode_column(row, 11)?,
            updated_at: decode_column(row, 12)?,
            revoked_at: decode_column(row, 13)?,
        })
    }
}

const SELECT_DIRECTORY_RESOURCE_GRANTS: &str = "select id, organization_id, subject_kind, directory_issuer, directory_subject_id, scope_kind, project_id, environment_id, application_id, node_id, aggregate_version, created_at, updated_at, revoked_at from directory_resource_grants";

fn decode_directory_resource_grant(
    row: DirectoryResourceGrantRow,
) -> Result<DirectoryResourceGrant, RepositoryError> {
    let kind = DirectoryGrantSubjectKind::parse(&row.subject_kind)
        .map_err(|error| RepositoryError::Storage(error))?;
    let issuer = OidcIssuer::parse(row.directory_issuer).map_err(|error| {
        RepositoryError::Storage(format!("stored directory issuer is invalid: {error}"))
    })?;
    let subject = DirectoryGrantSubjectRef::new(kind, issuer, row.directory_subject_id);
    let scope = match (
        row.scope_kind.as_str(),
        row.project_id,
        row.environment_id,
        row.application_id,
        row.node_id,
    ) {
        ("project", Some(project_id), None, None, None) => ResourceGrantScope::Project {
            project_id: ProjectId::from_uuid(project_id),
        },
        ("environment", Some(project_id), Some(environment_id), None, None) => {
            ResourceGrantScope::Environment {
                project_id: ProjectId::from_uuid(project_id),
                environment_id: EnvironmentId::from_uuid(environment_id),
            }
        }
        ("application", Some(project_id), None, Some(application_id), None) => {
            ResourceGrantScope::Application {
                project_id: ProjectId::from_uuid(project_id),
                application_id: ApplicationId::from_uuid(application_id),
            }
        }
        ("node", None, None, None, Some(node_id)) => ResourceGrantScope::Node {
            node_id: NodeId::from_uuid(node_id),
        },
        _ => {
            return Err(RepositoryError::Storage(
                "stored Directory Resource Grant scope is invalid".into(),
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
            "stored Directory Resource Grant lifecycle is invalid".into(),
        ));
    }
    Ok(DirectoryResourceGrant {
        id: ResourceGrantId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        subject,
        scope,
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        revoked_at: row.revoked_at,
    })
}

async fn load_directory_resource_grant_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    resource_grant_id: ResourceGrantId,
) -> Result<Option<DirectoryResourceGrant>, PostgresPersistenceError> {
    fetch_optional::<DirectoryResourceGrantRow, _>(
        transaction,
        sql_query::<DirectoryResourceGrantRow>(SELECT_DIRECTORY_RESOURCE_GRANTS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(resource_grant_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_directory_resource_grant)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_active_directory_resource_grants_for_organization(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
) -> Result<Vec<DirectoryResourceGrant>, PostgresPersistenceError> {
    fetch_all::<DirectoryResourceGrantRow, _>(
        transaction,
        sql_query::<DirectoryResourceGrantRow>(SELECT_DIRECTORY_RESOURCE_GRANTS)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and revoked_at is null"),
    )
    .await?
    .into_iter()
    .map(decode_directory_resource_grant)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

async fn insert_directory_resource_grant(
    transaction: &a3s_orm::PostgresTransaction,
    grant: &DirectoryResourceGrant,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into directory_resource_grants (id, organization_id, subject_kind, directory_issuer, directory_subject_id, scope_kind, project_id, environment_id, application_id, node_id, aggregate_version, created_at, updated_at, revoked_at) values (",
        )
        .bind(grant.id.as_uuid())
        .append(", ")
        .bind(grant.organization_id.as_uuid())
        .append(", ")
        .bind(grant.subject.kind().as_str())
        .append(", ")
        .bind(grant.subject.issuer().as_str())
        .append(", ")
        .bind(grant.subject.subject_id())
        .append(", ")
        .bind(grant.scope.kind())
        .append(", ")
        .bind(grant.scope.project_id().map(|id| id.as_uuid()))
        .append(", ")
        .bind(grant.scope.environment_id().map(|id| id.as_uuid()))
        .append(", ")
        .bind(grant.scope.application_id().map(|id| id.as_uuid()))
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
            "creating Directory Resource Grant affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_directory_resource_grant_audit(
    transaction: &a3s_orm::PostgresTransaction,
    grant: &DirectoryResourceGrant,
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
                "subjectKind": grant.subject.kind().as_str(),
                "issuer": grant.subject.issuer().as_str(),
                "subjectId": grant.subject.subject_id(),
                "subjectRef": grant.subject.format_ref(),
                "scopeKind": grant.scope.kind(),
                "projectId": grant.scope.project_id(),
                "environmentId": grant.scope.environment_id(),
                "applicationId": grant.scope.application_id(),
                "nodeId": grant.scope.node_id(),
                "aggregateVersion": grant.aggregate_version,
                "revokedAt": grant.revoked_at,
            }),
        },
    )
    .await
}

#[async_trait]
impl IDirectoryResourceGrantRepository for PostgresIdentityRepository {
    async fn create_directory_resource_grant(
        &self,
        write: CreateDirectoryResourceGrantWrite,
    ) -> Result<IdempotentWrite<DirectoryResourceGrant>, RepositoryError> {
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
                    MembershipAdministration::authorize(
                        actor.as_ref(),
                        write.grant.organization_id,
                        MembershipRole::Restricted,
                        None,
                    )
                    .map_err(RepositoryError::Forbidden)?;
                    if let Some(replayed) = idempotency_replay::<DirectoryResourceGrant>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    if !write.grant.is_active() || write.grant.aggregate_version != 1 {
                        return Err(PostgresPersistenceError::Invariant(
                            "new Directory Resource Grant is not at its initial lifecycle state"
                                .into(),
                        ));
                    }
                    let active_count = fetch_optional::<i64, _>(
                        transaction,
                        sql_query::<i64>(
                            "select count(*) from directory_resource_grants where organization_id = ",
                        )
                        .bind(write.grant.organization_id.as_uuid())
                        .append(" and revoked_at is null"),
                    )
                    .await?
                    .unwrap_or_default();
                    if active_count >= i64::from(MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG) {
                        return Err(RepositoryError::Conflict(format!(
                            "organization cannot have more than {MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG} active Directory Resource Grants"
                        ))
                        .into());
                    }
                    match insert_directory_resource_grant(transaction, &write.grant).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "an active Directory Resource Grant already covers this exact subject and scope".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_directory_resource_grant_audit(
                        transaction,
                        &write.grant,
                        write.actor_principal_id,
                        "identity.directory-resource-grant.created",
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

    async fn find_directory_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<Option<DirectoryResourceGrant>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<DirectoryResourceGrantRow>(SELECT_DIRECTORY_RESOURCE_GRANTS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(resource_grant_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_directory_resource_grant)
            .transpose()
    }

    async fn list_directory_resource_grants_by_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<DirectoryResourceGrant>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<DirectoryResourceGrantRow>(SELECT_DIRECTORY_RESOURCE_GRANTS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_directory_resource_grant)
            .collect()
    }

    async fn list_active_directory_resource_grants_for_subjects(
        &self,
        organization_id: OrganizationId,
        subjects: &[DirectoryGrantSubjectRef],
    ) -> Result<Vec<DirectoryResourceGrant>, RepositoryError> {
        if subjects.is_empty() {
            return Ok(Vec::new());
        }
        let mut grants = Vec::new();
        // Subject cardinality is bounded by projection; load org actives and filter in memory.
        let active = Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<DirectoryResourceGrantRow>(SELECT_DIRECTORY_RESOURCE_GRANTS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and revoked_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_directory_resource_grant)
            .collect::<Result<Vec<_>, _>>()?;
        let subject_set = subjects.iter().cloned().collect::<std::collections::BTreeSet<_>>();
        for grant in active {
            if subject_set.contains(&grant.subject) {
                grants.push(grant);
            }
        }
        grants.sort_by_key(|grant| grant.id);
        Ok(grants)
    }

    async fn revoke_directory_resource_grant(
        &self,
        write: RevokeDirectoryResourceGrantWrite,
    ) -> Result<IdempotentWrite<DirectoryResourceGrant>, RepositoryError> {
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
                    MembershipAdministration::authorize(
                        actor.as_ref(),
                        write.organization_id,
                        MembershipRole::Restricted,
                        None,
                    )
                    .map_err(RepositoryError::Forbidden)?;
                    let mut grant = load_directory_resource_grant_for_update(
                        transaction,
                        write.organization_id,
                        write.resource_grant_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if let Some(replayed) = idempotency_replay::<DirectoryResourceGrant>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    if grant.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "Directory Resource Grant changed before revocation".into(),
                        )
                        .into());
                    }
                    let changed = grant.revoke(write.revoked_at);
                    if changed {
                        let rows = execute(
                            transaction,
                            sql_query::<()>(
                                "update directory_resource_grants set aggregate_version = ",
                            )
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
                                "Directory Resource Grant changed while it was being revoked"
                                    .into(),
                            )
                            .into());
                        }
                        let event =
                            DirectoryResourceGrantChanged::revoked(&grant, write.request_id)?;
                        store_outbox(transaction, &event).await?;
                        store_directory_resource_grant_audit(
                            transaction,
                            &grant,
                            write.actor_principal_id,
                            "identity.directory-resource-grant.revoked",
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
