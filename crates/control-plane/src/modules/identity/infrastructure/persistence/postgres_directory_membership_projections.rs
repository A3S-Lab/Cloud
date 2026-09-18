use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_memberships::{
    load_active_membership_for_update, load_principal, lock_membership_set,
};
use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, store_audit, store_idempotency,
    store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use crate::modules::identity::domain::repositories::{
    ClearDirectoryMembershipProjectionWrite, IDirectoryMembershipProjectionRepository,
    ReplaceDirectoryMembershipProjectionWrite,
    MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::{
    DirectoryGrantSubjectKind, DirectoryGrantSubjectRef, MembershipRole, OidcIssuer,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct DirectoryMembershipProjectionRow {
    organization_id: Uuid,
    subject_kind: String,
    directory_issuer: String,
    directory_subject_id: Uuid,
    principal_id: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for DirectoryMembershipProjectionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode_column(row, 0)?,
            subject_kind: decode_column(row, 1)?,
            directory_issuer: decode_column(row, 2)?,
            directory_subject_id: decode_column(row, 3)?,
            principal_id: decode_column(row, 4)?,
            created_at: decode_column(row, 5)?,
        })
    }
}

const SELECT_DIRECTORY_MEMBERSHIP_PROJECTIONS: &str = "select organization_id, subject_kind, directory_issuer, directory_subject_id, principal_id, created_at from directory_membership_projections";

fn decode_binding(
    row: DirectoryMembershipProjectionRow,
) -> Result<DirectoryMembershipProjectionBinding, RepositoryError> {
    let kind = DirectoryGrantSubjectKind::parse(&row.subject_kind)
        .map_err(RepositoryError::Storage)?;
    let issuer = OidcIssuer::parse(row.directory_issuer).map_err(|error| {
        RepositoryError::Storage(format!("stored directory issuer is invalid: {error}"))
    })?;
    Ok(DirectoryMembershipProjectionBinding::new(
        OrganizationId::from_uuid(row.organization_id),
        DirectoryGrantSubjectRef::new(kind, issuer, row.directory_subject_id),
        PrincipalId::from_uuid(row.principal_id),
        row.created_at,
    ))
}

async fn delete_bindings_for_subject(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    subject: &DirectoryGrantSubjectRef,
) -> Result<(), PostgresPersistenceError> {
    execute(
        transaction,
        sql_query::<()>(
            "delete from directory_membership_projections where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and subject_kind = ")
        .bind(subject.kind().as_str())
        .append(" and directory_issuer = ")
        .bind(subject.issuer().as_str())
        .append(" and directory_subject_id = ")
        .bind(subject.subject_id()),
    )
    .await?;
    Ok(())
}

async fn insert_binding(
    transaction: &a3s_orm::PostgresTransaction,
    binding: &DirectoryMembershipProjectionBinding,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into directory_membership_projections (organization_id, subject_kind, directory_issuer, directory_subject_id, principal_id, created_at) values (",
        )
        .bind(binding.organization_id.as_uuid())
        .append(", ")
        .bind(binding.subject.kind().as_str())
        .append(", ")
        .bind(binding.subject.issuer().as_str())
        .append(", ")
        .bind(binding.subject.subject_id())
        .append(", ")
        .bind(binding.principal_id.as_uuid())
        .append(", ")
        .bind(binding.created_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating directory membership projection affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_projection_audit(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    subject: &DirectoryGrantSubjectRef,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
    principal_ids: &[PrincipalId],
    occurred_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let details = serde_json::json!({
        "subjectRef": subject.format_ref(),
        "principalIds": principal_ids.iter().map(|id| id.as_uuid()).collect::<Vec<_>>(),
    });
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: subject.subject_id(),
            occurred_at,
            request_id,
            scope: AuditWrite::organization_scope(organization_id.as_uuid()),
            details,
        },
    )
    .await
}

#[async_trait]
impl IDirectoryMembershipProjectionRepository for PostgresIdentityRepository {
    async fn replace_bindings(
        &self,
        write: ReplaceDirectoryMembershipProjectionWrite,
    ) -> Result<IdempotentWrite<Vec<DirectoryMembershipProjectionBinding>>, RepositoryError> {
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
                    if let Some(replayed) = idempotency_replay::<
                        Vec<DirectoryMembershipProjectionBinding>,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(replayed);
                    }
                    if write.bindings.len()
                        > usize::from(MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT)
                    {
                        return Err(RepositoryError::Conflict(format!(
                            "subject cannot bind more than {MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT} principals"
                        ))
                        .into());
                    }
                    let mut bindings = write.bindings;
                    bindings.sort_by(|left, right| left.principal_id.cmp(&right.principal_id));
                    bindings.dedup_by(|left, right| left.principal_id == right.principal_id);
                    for binding in &bindings {
                        if binding.organization_id != write.organization_id
                            || binding.subject != write.subject
                        {
                            return Err(RepositoryError::Conflict(
                                "directory membership projection binding does not match replace subject"
                                    .into(),
                            )
                            .into());
                        }
                        let membership = load_active_membership_for_update(
                            transaction,
                            write.organization_id,
                            binding.principal_id,
                        )
                        .await?;
                        let principal = load_principal(transaction, binding.principal_id)
                            .await?
                            .filter(|principal| principal.is_active());
                        if membership.is_none() || principal.is_none() {
                            return Err(RepositoryError::Forbidden(
                                "directory membership projection principal is not an active organization member"
                                    .into(),
                            )
                            .into());
                        }
                    }
                    delete_bindings_for_subject(transaction, write.organization_id, &write.subject)
                        .await?;
                    for binding in &bindings {
                        match insert_binding(transaction, binding).await {
                            Ok(()) => {}
                            Err(error) if is_foreign_key_violation(&error) => {
                                return Err(RepositoryError::NotFound.into())
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    let principal_ids = bindings
                        .iter()
                        .map(|binding| binding.principal_id)
                        .collect::<Vec<_>>();
                    let occurred_at = bindings
                        .first()
                        .map(|binding| binding.created_at)
                        .unwrap_or_else(Utc::now);
                    store_outbox(transaction, &write.event).await?;
                    store_projection_audit(
                        transaction,
                        write.organization_id,
                        &write.subject,
                        write.actor_principal_id,
                        "identity.directory-membership-projection.replaced",
                        write.request_id,
                        &principal_ids,
                        occurred_at,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &bindings).await?;
                    Ok(IdempotentWrite {
                        value: bindings,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_bindings_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<DirectoryGrantSubjectRef>, RepositoryError> {
        Ok(self
            .list_projection_bindings_for_principal(organization_id, principal_id)
            .await?
            .into_iter()
            .map(|binding| binding.subject)
            .collect())
    }

    async fn list_bindings_for_subject(
        &self,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
    ) -> Result<Vec<PrincipalId>, RepositoryError> {
        Ok(self
            .list_projection_bindings_for_subject(organization_id, subject)
            .await?
            .into_iter()
            .map(|binding| binding.principal_id)
            .collect())
    }

    async fn list_projection_bindings_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<DirectoryMembershipProjectionBinding>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<DirectoryMembershipProjectionRow>(SELECT_DIRECTORY_MEMBERSHIP_PROJECTIONS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and principal_id = ")
                    .bind(principal_id.as_uuid())
                    .append(" order by subject_kind asc, directory_issuer asc, directory_subject_id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_binding)
            .collect()
    }

    async fn list_projection_bindings_for_subject(
        &self,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
    ) -> Result<Vec<DirectoryMembershipProjectionBinding>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<DirectoryMembershipProjectionRow>(SELECT_DIRECTORY_MEMBERSHIP_PROJECTIONS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and subject_kind = ")
                    .bind(subject.kind().as_str())
                    .append(" and directory_issuer = ")
                    .bind(subject.issuer().as_str())
                    .append(" and directory_subject_id = ")
                    .bind(subject.subject_id())
                    .append(" order by principal_id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_binding)
            .collect()
    }

    async fn clear_bindings_for_subject(
        &self,
        write: ClearDirectoryMembershipProjectionWrite,
    ) -> Result<IdempotentWrite<()>, RepositoryError> {
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
                    if let Some(replayed) =
                        idempotency_replay::<()>(transaction, &write.idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    delete_bindings_for_subject(transaction, write.organization_id, &write.subject)
                        .await?;
                    store_outbox(transaction, &write.event).await?;
                    store_projection_audit(
                        transaction,
                        write.organization_id,
                        &write.subject,
                        write.actor_principal_id,
                        "identity.directory-membership-projection.cleared",
                        write.request_id,
                        &[],
                        Utc::now(),
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &()).await?;
                    Ok(IdempotentWrite {
                        value: (),
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}
