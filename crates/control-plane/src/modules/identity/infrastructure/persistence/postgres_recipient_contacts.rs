use super::postgres::PostgresIdentityRepository;
use super::postgres_recipient_contact_support::{
    authorize_contact_actor, contact_select, decode_contact, decode_verification,
    load_contact_by_address_for_update, load_contact_for_update, load_verification_for_update,
    store_recipient_contact_audit, verification_select, RecipientContactRow,
    RecipientContactVerificationRow,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_unique_violation, store_idempotency,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    RecipientContact, RecipientContactRecord, RecipientContactStatus, RecipientContactVerification,
    RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::events::RecipientContactChanged;
use crate::modules::identity::domain::repositories::{
    BeginRecipientContactVerificationResult, BeginRecipientContactVerificationWrite,
    CompleteRecipientContactVerificationWrite, IRecipientContactRepository,
    ResolvedRecipientContact, RevokeRecipientContactWrite,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, PrincipalId, RecipientContactId,
    RecipientContactVerificationId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect};
use async_trait::async_trait;

#[async_trait]
impl IRecipientContactRepository for PostgresIdentityRepository {
    async fn begin_recipient_contact_verification(
        &self,
        write: BeginRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<BeginRecipientContactVerificationResult>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    authorize_contact_actor(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    if let Some(replayed) = idempotency_replay::<
                        BeginRecipientContactVerificationResult,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let mut inserted_contact = false;
                    let contact = match load_contact_by_address_for_update(
                        transaction,
                        write.actor_principal_id,
                        &write.address,
                    )
                    .await?
                    {
                        Some(contact) => contact,
                        None => {
                            let contact = RecipientContact::create(
                                write.contact_id,
                                write.actor_principal_id,
                                write.address,
                                write.requested_at,
                            )
                            .map_err(RepositoryError::Storage)?;
                            let inserted = execute(
                                transaction,
                                sql_query::<()>(
                                    "insert into recipient_contacts (id, principal_id, canonical_address, address_digest, aggregate_version, state, created_at, updated_at, verified_at, revoked_at) values (",
                                )
                                .bind(contact.id.as_uuid())
                                .append(", ")
                                .bind(contact.principal_id.as_uuid())
                                .append(", ")
                                .bind(contact.address.as_str())
                                .append(", ")
                                .bind(contact.address.digest().as_str())
                                .append(", ")
                                .bind(contact.aggregate_version)
                                .append(", ")
                                .bind(contact.status.as_str())
                                .append(", ")
                                .bind(contact.created_at)
                                .append(", ")
                                .bind(contact.updated_at)
                                .append(", null, null)"),
                            )
                            .await;
                            match inserted {
                                Ok(1) => inserted_contact = true,
                                Ok(rows) => {
                                    return Err(PostgresPersistenceError::Invariant(format!(
                                        "creating recipient contact affected {rows} rows"
                                    )))
                                }
                                Err(error) if is_unique_violation(&error) => {
                                    return Err(RepositoryError::Conflict(
                                        "recipient contact already exists".into(),
                                    )
                                    .into())
                                }
                                Err(error) => return Err(error),
                            }
                            contact
                        }
                    };
                    match contact.status {
                        RecipientContactStatus::Pending => {}
                        RecipientContactStatus::Verified => {
                            return Err(RepositoryError::Conflict(
                                "recipient contact is already verified".into(),
                            )
                            .into())
                        }
                        RecipientContactStatus::Revoked => {
                            return Err(
                                RepositoryError::Conflict("recipient contact is revoked".into())
                                    .into(),
                            )
                        }
                    }
                    if !inserted_contact {
                        execute(
                            transaction,
                            sql_query::<()>(
                                "update recipient_contact_verifications set invalidated_at = greatest(issued_at, ",
                            )
                            .bind(write.requested_at)
                            .append(") where contact_id = ")
                            .bind(contact.id.as_uuid())
                            .append(" and consumed_at is null and invalidated_at is null"),
                        )
                        .await?;
                    }
                    let verification = RecipientContactVerification::create(
                        write.verification_id,
                        contact.id,
                        contact.principal_id,
                        contact.address.digest(),
                        contact.aggregate_version,
                        write.signing_key_id,
                        write.requested_at,
                        write.expires_at,
                    )
                    .map_err(RepositoryError::Storage)?;
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into recipient_contact_verifications (id, organization_id, contact_id, principal_id, address_digest, contact_version, signing_key_id, issued_at, expires_at, consumed_at, invalidated_at) values (",
                        )
                        .bind(verification.id.as_uuid())
                        .append(", ")
                        .bind(write.organization_id.as_uuid())
                        .append(", ")
                        .bind(verification.contact_id.as_uuid())
                        .append(", ")
                        .bind(verification.principal_id.as_uuid())
                        .append(", ")
                        .bind(verification.address_digest.as_str())
                        .append(", ")
                        .bind(verification.contact_version)
                        .append(", ")
                        .bind(verification.signing_key_id.as_str())
                        .append(", ")
                        .bind(verification.issued_at)
                        .append(", ")
                        .bind(verification.expires_at)
                        .append(", null, null)"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating recipient contact verification affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "recipient contact verification already exists".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    let result = BeginRecipientContactVerificationResult {
                        contact: contact.record(),
                        verification: verification.clone(),
                    };
                    store_outbox(
                        transaction,
                        &RecipientContactChanged::verification_requested(
                            write.organization_id,
                            &result.contact,
                            &verification,
                            write.request_id,
                        )?,
                    )
                    .await?;
                    store_recipient_contact_audit(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                        &result.contact,
                        Some(&verification),
                        "identity.recipient-contact.verification-requested",
                        verification.issued_at,
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &result).await?;
                    Ok(IdempotentWrite {
                        value: result,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_recipient_contact(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> Result<Option<RecipientContactRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    authorize_contact_actor(transaction, organization_id, principal_id).await?;
                    Ok(fetch_optional::<RecipientContactRow, _>(
                        transaction,
                        sql_query::<RecipientContactRow>(contact_select())
                            .append(" where principal_id = ")
                            .bind(principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(contact_id.as_uuid()),
                    )
                    .await?
                    .map(decode_contact)
                    .transpose()?
                    .map(|contact| contact.record()))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_recipient_contacts(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<RecipientContactRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    authorize_contact_actor(transaction, organization_id, principal_id).await?;
                    fetch_all::<RecipientContactRow, _>(
                        transaction,
                        sql_query::<RecipientContactRow>(contact_select())
                            .append(" where principal_id = ")
                            .bind(principal_id.as_uuid())
                            .append(" order by created_at asc, id asc"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_contact)
                    .map(|value| value.map(|contact| contact.record()))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_recipient_contact_verification(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
        verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerification>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    authorize_contact_actor(transaction, organization_id, principal_id).await?;
                    Ok(fetch_optional::<RecipientContactVerificationRow, _>(
                        transaction,
                        sql_query::<RecipientContactVerificationRow>(verification_select())
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and principal_id = ")
                            .bind(principal_id.as_uuid())
                            .append(" and contact_id = ")
                            .bind(contact_id.as_uuid())
                            .append(" and id = ")
                            .bind(verification_id.as_uuid()),
                    )
                    .await?
                    .map(decode_verification)
                    .transpose()?)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn complete_recipient_contact_verification(
        &self,
        write: CompleteRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    authorize_contact_actor(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    let mut contact = load_contact_for_update(
                        transaction,
                        write.actor_principal_id,
                        write.contact_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if let Some(replayed) = idempotency_replay::<RecipientContactRecord>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let mut verification = load_verification_for_update(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                        write.contact_id,
                        write.claims.challenge_id,
                    )
                    .await?
                    .filter(|verification| verification.claims() == write.claims)
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "recipient contact verification proof does not match an active challenge"
                                .into(),
                        )
                    })?;
                    if verification.status_at(write.completed_at)
                        != RecipientContactVerificationStatus::Pending
                    {
                        return Err(RepositoryError::Conflict(
                            "recipient contact verification is not pending".into(),
                        )
                        .into());
                    }
                    let expected_version = contact.aggregate_version;
                    contact
                        .verify(&write.claims, write.completed_at)
                        .map_err(RepositoryError::Conflict)?;
                    verification
                        .consume(write.completed_at)
                        .map_err(RepositoryError::Conflict)?;
                    let contact_rows = execute(
                        transaction,
                        sql_query::<()>("update recipient_contacts set aggregate_version = ")
                            .bind(contact.aggregate_version)
                            .append(", state = ")
                            .bind(contact.status.as_str())
                            .append(", updated_at = ")
                            .bind(contact.updated_at)
                            .append(", verified_at = ")
                            .bind(contact.verified_at)
                            .append(" where principal_id = ")
                            .bind(contact.principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(contact.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(expected_version)
                            .append(" and state = 'pending'"),
                    )
                    .await?;
                    if contact_rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "recipient contact changed while it was being verified".into(),
                        )
                        .into());
                    }
                    let verification_rows = execute(
                        transaction,
                        sql_query::<()>(
                            "update recipient_contact_verifications set consumed_at = ",
                        )
                        .bind(verification.consumed_at)
                        .append(" where id = ")
                        .bind(verification.id.as_uuid())
                        .append(" and consumed_at is null and invalidated_at is null"),
                    )
                    .await?;
                    if verification_rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "recipient contact verification changed while it was being consumed"
                                .into(),
                        )
                        .into());
                    }
                    let record = contact.record();
                    store_outbox(
                        transaction,
                        &RecipientContactChanged::verified(
                            write.organization_id,
                            &record,
                            &verification,
                            write.request_id,
                        )?,
                    )
                    .await?;
                    store_recipient_contact_audit(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                        &record,
                        Some(&verification),
                        "identity.recipient-contact.verified",
                        write.completed_at,
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

    async fn revoke_recipient_contact(
        &self,
        write: RevokeRecipientContactWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    authorize_contact_actor(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    let mut contact = load_contact_for_update(
                        transaction,
                        write.actor_principal_id,
                        write.contact_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if let Some(replayed) = idempotency_replay::<RecipientContactRecord>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    if write.expected_version == 0
                        || contact.aggregate_version != write.expected_version
                    {
                        return Err(RepositoryError::Conflict(
                            "recipient contact changed before revocation".into(),
                        )
                        .into());
                    }
                    if !contact.revoke(write.revoked_at) {
                        return Err(RepositoryError::Conflict(
                            "recipient contact is already revoked".into(),
                        )
                        .into());
                    }
                    execute(
                        transaction,
                        sql_query::<()>(
                            "update recipient_contact_verifications set invalidated_at = greatest(issued_at, ",
                        )
                        .bind(write.revoked_at)
                        .append(") where contact_id = ")
                        .bind(contact.id.as_uuid())
                        .append(" and consumed_at is null and invalidated_at is null"),
                    )
                    .await?;
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update recipient_contacts set aggregate_version = ")
                            .bind(contact.aggregate_version)
                            .append(", state = ")
                            .bind(contact.status.as_str())
                            .append(", updated_at = ")
                            .bind(contact.updated_at)
                            .append(", revoked_at = ")
                            .bind(contact.revoked_at)
                            .append(" where principal_id = ")
                            .bind(contact.principal_id.as_uuid())
                            .append(" and id = ")
                            .bind(contact.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and state <> 'revoked'"),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "recipient contact changed while it was being revoked".into(),
                        )
                        .into());
                    }
                    let record = contact.record();
                    store_outbox(
                        transaction,
                        &RecipientContactChanged::revoked(
                            write.organization_id,
                            &record,
                            write.request_id,
                        )?,
                    )
                    .await?;
                    store_recipient_contact_audit(
                        transaction,
                        write.organization_id,
                        write.actor_principal_id,
                        &record,
                        None,
                        "identity.recipient-contact.revoked",
                        write.revoked_at,
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

    async fn resolve_verified_recipient_contact(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> Result<Option<ResolvedRecipientContact>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<RecipientContactRow>(contact_select())
                    .append(" join identity_principals as principal on principal.id = recipient_contacts.principal_id join organization_memberships as membership on membership.principal_id = recipient_contacts.principal_id and membership.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and membership.revoked_at is null where recipient_contacts.principal_id = ")
                    .bind(principal_id.as_uuid())
                    .append(" and recipient_contacts.id = ")
                    .bind(contact_id.as_uuid())
                    .append(" and recipient_contacts.state = 'verified' and principal.kind = 'human' and principal.disabled_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_contact)
            .transpose()
            .map(|contact| {
                contact.and_then(|contact| {
                    contact
                        .verified_at
                        .map(|verified_at| ResolvedRecipientContact {
                            id: contact.id,
                            principal_id: contact.principal_id,
                            address: contact.address,
                            aggregate_version: contact.aggregate_version,
                            verified_at,
                        })
                })
            })
    }
}
