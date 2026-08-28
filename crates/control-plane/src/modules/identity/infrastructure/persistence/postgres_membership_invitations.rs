use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_memberships::{
    authorize_management, load_active_membership_for_update, load_principal, lock_membership_set,
    store_membership_audit,
};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, store_audit, store_idempotency, store_outbox,
    transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{Membership, MembershipInvitation};
use crate::modules::identity::domain::events::{MembershipChanged, MembershipInvitationChanged};
use crate::modules::identity::domain::repositories::{
    AcceptMembershipInvitationWrite, CreateMembershipInvitationWrite,
    IMembershipInvitationRepository, MembershipInvitationAcceptance, MembershipRecord,
    RevokeMembershipInvitationWrite,
};
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, MembershipId, MembershipInvitationId, OrganizationId, PrincipalId,
    RepositoryError,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct MembershipInvitationRow {
    id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    role: String,
    invited_by_principal_id: Uuid,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    accepted_membership_id: Option<Uuid>,
    accepted_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for MembershipInvitationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            principal_id: decode_column(row, 2)?,
            role: decode_column(row, 3)?,
            invited_by_principal_id: decode_column(row, 4)?,
            aggregate_version: decode_column(row, 5)?,
            created_at: decode_column(row, 6)?,
            updated_at: decode_column(row, 7)?,
            expires_at: decode_column(row, 8)?,
            accepted_membership_id: decode_column(row, 9)?,
            accepted_at: decode_column(row, 10)?,
            revoked_at: decode_column(row, 11)?,
        })
    }
}

fn decode_invitation(
    row: MembershipInvitationRow,
) -> Result<MembershipInvitation, RepositoryError> {
    Ok(MembershipInvitation {
        id: MembershipInvitationId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        role: MembershipRole::parse(&row.role).map_err(|error| {
            RepositoryError::Storage(format!(
                "stored membership invitation role is invalid: {error}"
            ))
        })?,
        invited_by_principal_id: PrincipalId::from_uuid(row.invited_by_principal_id),
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        expires_at: row.expires_at,
        accepted_membership_id: row.accepted_membership_id.map(MembershipId::from_uuid),
        accepted_at: row.accepted_at,
        revoked_at: row.revoked_at,
    })
}

fn invitation_select() -> &'static str {
    "select id, organization_id, principal_id, role, invited_by_principal_id, aggregate_version, created_at, updated_at, expires_at, accepted_membership_id, accepted_at, revoked_at from membership_invitations"
}

async fn load_invitation_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    invitation_id: MembershipInvitationId,
) -> Result<Option<MembershipInvitation>, PostgresPersistenceError> {
    fetch_optional::<MembershipInvitationRow, _>(
        transaction,
        sql_query::<MembershipInvitationRow>(invitation_select())
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(invitation_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_invitation)
    .transpose()
    .map_err(Into::into)
}

async fn insert_invitation(
    transaction: &a3s_orm::PostgresTransaction,
    invitation: &MembershipInvitation,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into membership_invitations (id, organization_id, principal_id, role, invited_by_principal_id, aggregate_version, created_at, updated_at, expires_at, accepted_membership_id, accepted_at, revoked_at) values (")
            .bind(invitation.id.as_uuid())
            .append(", ")
            .bind(invitation.organization_id.as_uuid())
            .append(", ")
            .bind(invitation.principal_id.as_uuid())
            .append(", ")
            .bind(invitation.role.as_str())
            .append(", ")
            .bind(invitation.invited_by_principal_id.as_uuid())
            .append(", ")
            .bind(invitation.aggregate_version)
            .append(", ")
            .bind(invitation.created_at)
            .append(", ")
            .bind(invitation.updated_at)
            .append(", ")
            .bind(invitation.expires_at)
            .append(", ")
            .bind(invitation.accepted_membership_id.map(|id| id.as_uuid()))
            .append(", ")
            .bind(invitation.accepted_at)
            .append(", ")
            .bind(invitation.revoked_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating membership invitation affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_invitation_audit(
    transaction: &a3s_orm::PostgresTransaction,
    invitation: &MembershipInvitation,
    actor_principal_id: PrincipalId,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: AuditWrite::organization_scope(invitation.organization_id.as_uuid()),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: invitation.id.as_uuid(),
            occurred_at: invitation.updated_at,
            request_id,
            details: serde_json::json!({
                "principalId": invitation.principal_id,
                "role": invitation.role.as_str(),
                "expiresAt": invitation.expires_at,
                "acceptedMembershipId": invitation.accepted_membership_id,
                "aggregateVersion": invitation.aggregate_version,
            }),
        },
    )
    .await
}

#[async_trait]
impl IMembershipInvitationRepository for PostgresIdentityRepository {
    async fn create_membership_invitation(
        &self,
        write: CreateMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitation>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let invitation = write.invitation;
                    lock_membership_set(transaction, invitation.organization_id).await?;
                    let actor = load_active_membership_for_update(
                        transaction,
                        invitation.organization_id,
                        invitation.invited_by_principal_id,
                    )
                    .await?;
                    authorize_management(
                        actor.as_ref(),
                        write.actor_is_platform_admin,
                        invitation.role,
                        None,
                    )?;
                    if let Some(replayed) =
                        idempotency_replay::<MembershipInvitation>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    let principal = load_principal(transaction, invitation.principal_id)
                        .await?
                        .filter(|principal| principal.is_active())
                        .ok_or(RepositoryError::NotFound)?;
                    let membership_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from organization_memberships where organization_id = ",
                        )
                        .bind(invitation.organization_id.as_uuid())
                        .append(" and principal_id = ")
                        .bind(principal.id.as_uuid()),
                    )
                    .await?
                    .is_some();
                    if membership_exists {
                        return Err(RepositoryError::Conflict(
                            "principal already has an organization membership".into(),
                        )
                        .into());
                    }
                    let pending_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from membership_invitations where organization_id = ",
                        )
                        .bind(invitation.organization_id.as_uuid())
                        .append(" and principal_id = ")
                        .bind(principal.id.as_uuid())
                        .append(" and accepted_at is null and revoked_at is null and expires_at > ")
                        .bind(invitation.created_at)
                        .append(" limit 1"),
                    )
                    .await?
                    .is_some();
                    if pending_exists {
                        return Err(RepositoryError::Conflict(
                            "principal already has a pending membership invitation".into(),
                        )
                        .into());
                    }
                    insert_invitation(transaction, &invitation).await?;
                    store_outbox(transaction, &write.event).await?;
                    store_invitation_audit(
                        transaction,
                        &invitation,
                        invitation.invited_by_principal_id,
                        "identity.membership-invitation.created",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &invitation).await?;
                    Ok(IdempotentWrite {
                        value: invitation,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_membership_invitation(
        &self,
        organization_id: OrganizationId,
        invitation_id: MembershipInvitationId,
    ) -> Result<Option<MembershipInvitation>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<MembershipInvitationRow>(invitation_select())
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(invitation_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_invitation)
            .transpose()
    }

    async fn list_membership_invitations(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<MembershipInvitation>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<MembershipInvitationRow>(invitation_select())
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_invitation)
            .collect()
    }

    async fn list_membership_invitations_for_principal(
        &self,
        principal_id: PrincipalId,
    ) -> Result<Vec<MembershipInvitation>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<MembershipInvitationRow>(invitation_select())
                    .append(" where principal_id = ")
                    .bind(principal_id.as_uuid())
                    .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_invitation)
            .collect()
    }

    async fn accept_membership_invitation(
        &self,
        write: AcceptMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitationAcceptance>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let visible = fetch_optional::<MembershipInvitationRow, _>(
                        transaction,
                        sql_query::<MembershipInvitationRow>(invitation_select())
                            .append(" where id = ")
                            .bind(write.invitation_id.as_uuid())
                            .append(" and principal_id = ")
                            .bind(write.actor_principal_id.as_uuid()),
                    )
                    .await?
                    .map(decode_invitation)
                    .transpose()?
                    .ok_or(RepositoryError::NotFound)?;
                    lock_membership_set(transaction, visible.organization_id).await?;
                    let mut invitation = load_invitation_for_update(
                        transaction,
                        visible.organization_id,
                        write.invitation_id,
                    )
                    .await?
                    .filter(|invitation| invitation.principal_id == write.actor_principal_id)
                    .ok_or(RepositoryError::NotFound)?;
                    if let Some(replayed) = idempotency_replay::<MembershipInvitationAcceptance>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    if invitation.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "membership invitation changed before acceptance".into(),
                        )
                        .into());
                    }
                    let principal = load_principal(transaction, invitation.principal_id)
                        .await?
                        .filter(|principal| principal.is_active())
                        .ok_or(RepositoryError::NotFound)?;
                    invitation
                        .accept(
                            write.actor_principal_id,
                            write.membership_id,
                            write.accepted_at,
                        )
                        .map_err(RepositoryError::Conflict)?;
                    let membership = Membership::create(
                        write.membership_id,
                        invitation.organization_id,
                        invitation.principal_id,
                        invitation.role,
                        invitation.updated_at,
                    );
                    super::postgres::insert_membership(transaction, &membership)
                        .await
                        .map_err(|error| match error {
                            error if crate::infrastructure::is_unique_violation(&error) => {
                                RepositoryError::Conflict(
                                    "principal already has an organization membership".into(),
                                )
                                .into()
                            }
                            error => error,
                        })?;
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update membership_invitations set aggregate_version = ")
                            .bind(invitation.aggregate_version)
                            .append(", updated_at = ")
                            .bind(invitation.updated_at)
                            .append(", accepted_membership_id = ")
                            .bind(invitation.accepted_membership_id.map(|id| id.as_uuid()))
                            .append(", accepted_at = ")
                            .bind(invitation.accepted_at)
                            .append(" where id = ")
                            .bind(invitation.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and accepted_at is null and revoked_at is null"),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "membership invitation changed while it was being accepted".into(),
                        )
                        .into());
                    }
                    let record = MembershipRecord {
                        principal,
                        membership,
                    };
                    let acceptance = MembershipInvitationAcceptance {
                        invitation: invitation.clone(),
                        membership: record.clone(),
                    };
                    store_outbox(
                        transaction,
                        &MembershipChanged::created(&record.membership, write.request_id)?,
                    )
                    .await?;
                    store_outbox(
                        transaction,
                        &MembershipInvitationChanged::accepted(&invitation, write.request_id)?,
                    )
                    .await?;
                    store_membership_audit(
                        transaction,
                        &record,
                        write.actor_principal_id,
                        "identity.membership.created",
                        write.request_id,
                    )
                    .await?;
                    store_invitation_audit(
                        transaction,
                        &invitation,
                        write.actor_principal_id,
                        "identity.membership-invitation.accepted",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &acceptance).await?;
                    Ok(IdempotentWrite {
                        value: acceptance,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_membership_invitation(
        &self,
        write: RevokeMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitation>, RepositoryError> {
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
                    let mut invitation = load_invitation_for_update(
                        transaction,
                        write.organization_id,
                        write.invitation_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    authorize_management(
                        actor.as_ref(),
                        write.actor_is_platform_admin,
                        invitation.role,
                        None,
                    )?;
                    if let Some(replayed) =
                        idempotency_replay::<MembershipInvitation>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    if invitation.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "membership invitation changed before revocation".into(),
                        )
                        .into());
                    }
                    let changed = invitation.revoke(write.revoked_at);
                    if changed {
                        let rows = execute(
                            transaction,
                            sql_query::<()>(
                                "update membership_invitations set aggregate_version = ",
                            )
                            .bind(invitation.aggregate_version)
                            .append(", updated_at = ")
                            .bind(invitation.updated_at)
                            .append(", revoked_at = ")
                            .bind(invitation.revoked_at)
                            .append(" where organization_id = ")
                            .bind(invitation.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(invitation.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and accepted_at is null and revoked_at is null"),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(RepositoryError::Conflict(
                                "membership invitation changed while it was being revoked".into(),
                            )
                            .into());
                        }
                        store_outbox(
                            transaction,
                            &MembershipInvitationChanged::revoked(&invitation, write.request_id)?,
                        )
                        .await?;
                        store_invitation_audit(
                            transaction,
                            &invitation,
                            write.actor_principal_id,
                            "identity.membership-invitation.revoked",
                            write.request_id,
                        )
                        .await?;
                    }
                    store_idempotency(transaction, &write.idempotency, &invitation).await?;
                    Ok(IdempotentWrite {
                        value: invitation,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}
