use super::postgres::{
    decode_column, decode_membership, decode_principal, insert_membership, insert_principal,
    MembershipRow, PostgresIdentityRepository, PrincipalRow,
};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_unique_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{IdentityPrincipal, Membership};
use crate::modules::identity::domain::events::MembershipChanged;
use crate::modules::identity::domain::repositories::{
    ChangeMembershipRoleWrite, CreateMembershipWrite, IMembershipRepository, MembershipRecord,
    RevokeMembershipWrite,
};
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, MembershipId, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct MembershipRecordRow {
    membership_id: Uuid,
    organization_id: Uuid,
    membership_principal_id: Uuid,
    role: String,
    membership_version: u64,
    membership_created_at: DateTime<Utc>,
    membership_updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    principal_id: Uuid,
    principal_kind: String,
    principal_name: String,
    principal_version: u64,
    principal_created_at: DateTime<Utc>,
    disabled_at: Option<DateTime<Utc>>,
}

impl FromRow for MembershipRecordRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            membership_id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            membership_principal_id: decode_column(row, 2)?,
            role: decode_column(row, 3)?,
            membership_version: decode_column(row, 4)?,
            membership_created_at: decode_column(row, 5)?,
            membership_updated_at: decode_column(row, 6)?,
            revoked_at: decode_column(row, 7)?,
            principal_id: decode_column(row, 8)?,
            principal_kind: decode_column(row, 9)?,
            principal_name: decode_column(row, 10)?,
            principal_version: decode_column(row, 11)?,
            principal_created_at: decode_column(row, 12)?,
            disabled_at: decode_column(row, 13)?,
        })
    }
}

fn decode_record(row: MembershipRecordRow) -> Result<MembershipRecord, RepositoryError> {
    if row.membership_principal_id != row.principal_id {
        return Err(RepositoryError::Storage(
            "stored membership crossed identity principals".into(),
        ));
    }
    Ok(MembershipRecord {
        membership: decode_membership((
            row.membership_id,
            row.organization_id,
            row.membership_principal_id,
            row.role,
            row.membership_version,
            row.membership_created_at,
            row.membership_updated_at,
            row.revoked_at,
        ))?,
        principal: decode_principal((
            row.principal_id,
            row.principal_kind,
            row.principal_name,
            row.principal_version,
            row.principal_created_at,
            row.disabled_at,
        ))?,
    })
}

pub(super) async fn lock_membership_set(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
) -> Result<(), PostgresPersistenceError> {
    let locked = fetch_optional::<i32, _>(
        transaction,
        sql_query::<i32>("select 1 from (select pg_advisory_xact_lock(hashtext(")
            .bind(format!("a3s-cloud.identity.memberships:{organization_id}"))
            .append("))) as locked"),
    )
    .await?;
    if locked != Some(1) {
        return Err(PostgresPersistenceError::Invariant(
            "membership authority lock did not return a row".into(),
        ));
    }
    Ok(())
}

pub(super) async fn load_membership_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    membership_id: MembershipId,
) -> Result<Option<Membership>, PostgresPersistenceError> {
    fetch_optional::<MembershipRow, _>(
        transaction,
        sql_query::<MembershipRow>(
            "select id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at from organization_memberships where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and id = ")
        .bind(membership_id.as_uuid())
        .append(" for update"),
    )
    .await?
    .map(decode_membership)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_active_membership_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
) -> Result<Option<Membership>, PostgresPersistenceError> {
    fetch_optional::<MembershipRow, _>(
        transaction,
        sql_query::<MembershipRow>(
            "select id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at from organization_memberships where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and principal_id = ")
        .bind(principal_id.as_uuid())
        .append(" and revoked_at is null for update"),
    )
    .await?
    .map(decode_membership)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_principal(
    transaction: &a3s_orm::PostgresTransaction,
    principal_id: PrincipalId,
) -> Result<Option<IdentityPrincipal>, PostgresPersistenceError> {
    fetch_optional::<PrincipalRow, _>(
        transaction,
        sql_query::<PrincipalRow>(
            "select id, kind, name, aggregate_version, created_at, disabled_at from identity_principals where id = ",
        )
        .bind(principal_id.as_uuid()),
    )
    .await?
    .map(decode_principal)
    .transpose()
    .map_err(Into::into)
}

pub(super) fn authorize_management(
    actor: Option<&Membership>,
    actor_is_platform_admin: bool,
    current_role: MembershipRole,
    next_role: Option<MembershipRole>,
) -> Result<(), RepositoryError> {
    if actor_is_platform_admin {
        return Ok(());
    }
    let actor = actor
        .filter(|membership| membership.is_active())
        .ok_or_else(|| {
            RepositoryError::Forbidden("actor is not an active organization member".into())
        })?;
    if !actor.role.can_manage_memberships()
        || !actor.role.can_manage_role(current_role)
        || next_role.is_some_and(|role| !actor.role.can_manage_role(role))
    {
        return Err(RepositoryError::Forbidden(
            "membership role does not permit this administration action".into(),
        ));
    }
    Ok(())
}

async fn require_another_owner(
    transaction: &a3s_orm::PostgresTransaction,
    membership: &Membership,
) -> Result<(), PostgresPersistenceError> {
    if membership.role != MembershipRole::Owner || !membership.is_active() {
        return Ok(());
    }
    let owner_count = fetch_optional::<i64, _>(
        transaction,
        sql_query::<i64>("select count(*) from organization_memberships where organization_id = ")
            .bind(membership.organization_id.as_uuid())
            .append(" and role = 'owner' and revoked_at is null"),
    )
    .await?
    .unwrap_or_default();
    if owner_count <= 1 {
        return Err(RepositoryError::Conflict(
            "organization must retain at least one active owner".into(),
        )
        .into());
    }
    Ok(())
}

pub(super) async fn store_membership_audit(
    transaction: &a3s_orm::PostgresTransaction,
    record: &MembershipRecord,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            organization_id: record.membership.organization_id.as_uuid(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: record.membership.id.as_uuid(),
            occurred_at: record.membership.updated_at,
            request_id,
            details: serde_json::json!({
                "principalId": record.principal.id,
                "principalKind": record.principal.kind.as_str(),
                "role": record.membership.role.as_str(),
                "aggregateVersion": record.membership.aggregate_version,
            }),
        },
    )
    .await
}

#[async_trait]
impl IMembershipRepository for PostgresIdentityRepository {
    async fn create_membership(
        &self,
        write: CreateMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let organization_id = write.membership.organization_id;
                    lock_membership_set(transaction, organization_id).await?;
                    let actor = load_active_membership_for_update(
                        transaction,
                        organization_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    authorize_management(
                        actor.as_ref(),
                        write.actor_is_platform_admin,
                        write.membership.role,
                        None,
                    )?;
                    if let Some(replayed) =
                        idempotency_replay::<MembershipRecord>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    if write.principal.id != write.membership.principal_id
                        || !write.principal.is_active()
                        || !write.membership.is_active()
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "membership does not bind one active principal".into(),
                        ));
                    }
                    let organization_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from organizations where id = ")
                            .bind(organization_id.as_uuid()),
                    )
                    .await?
                    .is_some();
                    if !organization_exists {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let inserted = async {
                        insert_principal(transaction, &write.principal).await?;
                        insert_membership(transaction, &write.membership).await
                    }
                    .await;
                    match inserted {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "identity principal or membership already exists".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    let record = MembershipRecord {
                        principal: write.principal,
                        membership: write.membership,
                    };
                    for event in &write.events {
                        store_outbox(transaction, event).await?;
                    }
                    store_membership_audit(
                        transaction,
                        &record,
                        write.actor_principal_id,
                        "identity.membership.created",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &record).await?;
                    Ok(IdempotentWrite {
                        value: record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Option<MembershipRecord>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<MembershipRecordRow>(
                    "select m.id, m.organization_id, m.principal_id, m.role, m.aggregate_version, m.created_at, m.updated_at, m.revoked_at, p.id, p.kind, p.name, p.aggregate_version, p.created_at, p.disabled_at from organization_memberships m join identity_principals p on p.id = m.principal_id where m.organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and m.id = ")
                .bind(membership_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_record)
            .transpose()
    }

    async fn list_memberships(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<MembershipRecord>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<MembershipRecordRow>(
                    "select m.id, m.organization_id, m.principal_id, m.role, m.aggregate_version, m.created_at, m.updated_at, m.revoked_at, p.id, p.kind, p.name, p.aggregate_version, p.created_at, p.disabled_at from organization_memberships m join identity_principals p on p.id = m.principal_id where m.organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" order by m.created_at asc, m.id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_record)
            .collect()
    }

    async fn find_active_membership_by_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<Membership>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<MembershipRow>(
                    "select id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at from organization_memberships where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and principal_id = ")
                .bind(principal_id.as_uuid())
                .append(" and revoked_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_membership)
            .transpose()
    }

    async fn change_membership_role(
        &self,
        write: ChangeMembershipRoleWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
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
                    let mut membership = load_membership_for_update(
                        transaction,
                        write.organization_id,
                        write.membership_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    authorize_management(
                        actor.as_ref(),
                        write.actor_is_platform_admin,
                        membership.role,
                        Some(write.role),
                    )?;
                    if let Some(replayed) =
                        idempotency_replay::<MembershipRecord>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    if membership.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "membership changed before its role update".into(),
                        )
                        .into());
                    }
                    if membership.role == MembershipRole::Owner
                        && write.role != MembershipRole::Owner
                    {
                        require_another_owner(transaction, &membership).await?;
                    }
                    let principal = load_principal(transaction, membership.principal_id)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "membership principal is missing".into(),
                            )
                        })?;
                    let changed = membership.change_role(write.role, write.changed_at);
                    if changed {
                        let rows = execute(
                            transaction,
                            sql_query::<()>("update organization_memberships set role = ")
                                .bind(membership.role.as_str())
                                .append(", aggregate_version = ")
                                .bind(membership.aggregate_version)
                                .append(", updated_at = ")
                                .bind(membership.updated_at)
                                .append(" where organization_id = ")
                                .bind(membership.organization_id.as_uuid())
                                .append(" and id = ")
                                .bind(membership.id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(write.expected_version)
                                .append(" and revoked_at is null"),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(RepositoryError::Conflict(
                                "membership changed while its role was being updated".into(),
                            )
                            .into());
                        }
                    }
                    let record = MembershipRecord {
                        principal,
                        membership,
                    };
                    if changed {
                        let event =
                            MembershipChanged::role_changed(&record.membership, write.request_id)?;
                        store_outbox(transaction, &event).await?;
                        store_membership_audit(
                            transaction,
                            &record,
                            write.actor_principal_id,
                            "identity.membership.role-changed",
                            write.request_id,
                        )
                        .await?;
                    }
                    store_idempotency(transaction, &write.idempotency, &record).await?;
                    Ok(IdempotentWrite {
                        value: record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_membership(
        &self,
        write: RevokeMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
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
                    let mut membership = load_membership_for_update(
                        transaction,
                        write.organization_id,
                        write.membership_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    authorize_management(
                        actor.as_ref(),
                        write.actor_is_platform_admin,
                        membership.role,
                        None,
                    )?;
                    if let Some(replayed) =
                        idempotency_replay::<MembershipRecord>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    if membership.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "membership changed before revocation".into(),
                        )
                        .into());
                    }
                    require_another_owner(transaction, &membership).await?;
                    let principal = load_principal(transaction, membership.principal_id)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "membership principal is missing".into(),
                            )
                        })?;
                    let changed = membership.revoke(write.revoked_at);
                    if changed {
                        let rows = execute(
                            transaction,
                            sql_query::<()>(
                                "update organization_memberships set aggregate_version = ",
                            )
                            .bind(membership.aggregate_version)
                            .append(", updated_at = ")
                            .bind(membership.updated_at)
                            .append(", revoked_at = ")
                            .bind(membership.revoked_at)
                            .append(" where organization_id = ")
                            .bind(membership.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(membership.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and revoked_at is null"),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(RepositoryError::Conflict(
                                "membership changed while it was being revoked".into(),
                            )
                            .into());
                        }
                    }
                    let record = MembershipRecord {
                        principal,
                        membership,
                    };
                    if changed {
                        let event =
                            MembershipChanged::revoked(&record.membership, write.request_id)?;
                        store_outbox(transaction, &event).await?;
                        store_membership_audit(
                            transaction,
                            &record,
                            write.actor_principal_id,
                            "identity.membership.revoked",
                            write.request_id,
                        )
                        .await?;
                    }
                    store_idempotency(transaction, &write.idempotency, &record).await?;
                    Ok(IdempotentWrite {
                        value: record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}
