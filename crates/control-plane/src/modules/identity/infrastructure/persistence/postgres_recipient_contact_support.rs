use super::postgres_memberships::{
    load_active_membership_for_update, load_principal, lock_membership_set,
};
use crate::infrastructure::{fetch_optional, store_audit, AuditWrite, PostgresPersistenceError};
use crate::modules::identity::domain::entities::{
    IdentityPrincipalKind, RecipientContact, RecipientContactRecord, RecipientContactStatus,
    RecipientContactVerification,
};
use crate::modules::identity::domain::value_objects::{
    RecipientContactSigningKeyId, RecipientEmailAddress,
};
use crate::modules::shared_kernel::domain::{
    PrincipalId, RecipientContactId, RecipientContactVerificationId, RepositoryError, Sha256Digest,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, PostgresTransaction, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) struct RecipientContactRow {
    id: Uuid,
    principal_id: Uuid,
    canonical_address: String,
    address_digest: String,
    aggregate_version: u64,
    state: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    verified_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for RecipientContactRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            principal_id: decode(row, 1)?,
            canonical_address: decode(row, 2)?,
            address_digest: decode(row, 3)?,
            aggregate_version: decode(row, 4)?,
            state: decode(row, 5)?,
            created_at: decode(row, 6)?,
            updated_at: decode(row, 7)?,
            verified_at: decode(row, 8)?,
            revoked_at: decode(row, 9)?,
        })
    }
}

pub(super) struct RecipientContactVerificationRow {
    id: Uuid,
    organization_id: Uuid,
    contact_id: Uuid,
    principal_id: Uuid,
    address_digest: String,
    contact_version: u64,
    signing_key_id: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    invalidated_at: Option<DateTime<Utc>>,
}

impl FromRow for RecipientContactVerificationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            contact_id: decode(row, 2)?,
            principal_id: decode(row, 3)?,
            address_digest: decode(row, 4)?,
            contact_version: decode(row, 5)?,
            signing_key_id: decode(row, 6)?,
            issued_at: decode(row, 7)?,
            expires_at: decode(row, 8)?,
            consumed_at: decode(row, 9)?,
            invalidated_at: decode(row, 10)?,
        })
    }
}

pub(super) fn contact_select() -> &'static str {
    "select recipient_contacts.id, recipient_contacts.principal_id, recipient_contacts.canonical_address, recipient_contacts.address_digest, recipient_contacts.aggregate_version, recipient_contacts.state, recipient_contacts.created_at, recipient_contacts.updated_at, recipient_contacts.verified_at, recipient_contacts.revoked_at from recipient_contacts"
}

pub(super) fn verification_select() -> &'static str {
    "select id, organization_id, contact_id, principal_id, address_digest, contact_version, signing_key_id, issued_at, expires_at, consumed_at, invalidated_at from recipient_contact_verifications"
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

pub(super) fn decode_contact(
    row: RecipientContactRow,
) -> Result<RecipientContact, RepositoryError> {
    let address = RecipientEmailAddress::parse(row.canonical_address)
        .map_err(|error| RepositoryError::Storage(format!("stored recipient address: {error}")))?;
    let address_digest = Sha256Digest::parse(row.address_digest).map_err(|error| {
        RepositoryError::Storage(format!("stored recipient address digest: {error}"))
    })?;
    if address.digest() != address_digest {
        return Err(RepositoryError::Storage(
            "stored recipient address digest does not match its canonical mailbox".into(),
        ));
    }
    let contact = RecipientContact {
        id: RecipientContactId::from_uuid(row.id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        address,
        aggregate_version: row.aggregate_version,
        status: RecipientContactStatus::parse(&row.state).map_err(RepositoryError::Storage)?,
        created_at: row.created_at,
        updated_at: row.updated_at,
        verified_at: row.verified_at,
        revoked_at: row.revoked_at,
    };
    contact.validate().map_err(RepositoryError::Storage)?;
    Ok(contact)
}

pub(super) fn decode_verification(
    row: RecipientContactVerificationRow,
) -> Result<RecipientContactVerification, RepositoryError> {
    let _organization_id = row.organization_id;
    let verification = RecipientContactVerification {
        id: RecipientContactVerificationId::from_uuid(row.id),
        contact_id: RecipientContactId::from_uuid(row.contact_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        address_digest: Sha256Digest::parse(row.address_digest).map_err(|error| {
            RepositoryError::Storage(format!("stored recipient address digest: {error}"))
        })?,
        contact_version: row.contact_version,
        signing_key_id: RecipientContactSigningKeyId::parse(row.signing_key_id)
            .map_err(RepositoryError::Storage)?,
        issued_at: row.issued_at,
        expires_at: row.expires_at,
        consumed_at: row.consumed_at,
        invalidated_at: row.invalidated_at,
    };
    verification.validate().map_err(RepositoryError::Storage)?;
    Ok(verification)
}

pub(super) async fn authorize_contact_actor(
    transaction: &PostgresTransaction,
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    principal_id: PrincipalId,
) -> Result<(), PostgresPersistenceError> {
    lock_membership_set(transaction, organization_id).await?;
    load_active_membership_for_update(transaction, organization_id, principal_id)
        .await?
        .ok_or_else(|| {
            RepositoryError::Forbidden(
                "recipient contact actor is not an active organization member".into(),
            )
        })?;
    let principal = load_principal(transaction, principal_id)
        .await?
        .filter(|principal| principal.is_active() && principal.kind == IdentityPrincipalKind::Human)
        .ok_or_else(|| {
            RepositoryError::Forbidden(
                "recipient contacts require an active human identity principal".into(),
            )
        })?;
    if principal.id != principal_id {
        return Err(PostgresPersistenceError::Invariant(
            "recipient contact principal identity drifted".into(),
        ));
    }
    Ok(())
}

pub(super) async fn load_contact_for_update(
    transaction: &PostgresTransaction,
    principal_id: PrincipalId,
    contact_id: RecipientContactId,
) -> Result<Option<RecipientContact>, PostgresPersistenceError> {
    fetch_optional::<RecipientContactRow, _>(
        transaction,
        sql_query::<RecipientContactRow>(contact_select())
            .append(" where principal_id = ")
            .bind(principal_id.as_uuid())
            .append(" and id = ")
            .bind(contact_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_contact)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_contact_by_address_for_update(
    transaction: &PostgresTransaction,
    principal_id: PrincipalId,
    address: &RecipientEmailAddress,
) -> Result<Option<RecipientContact>, PostgresPersistenceError> {
    fetch_optional::<RecipientContactRow, _>(
        transaction,
        sql_query::<RecipientContactRow>(contact_select())
            .append(" where principal_id = ")
            .bind(principal_id.as_uuid())
            .append(" and canonical_address = ")
            .bind(address.as_str())
            .append(" for update"),
    )
    .await?
    .map(decode_contact)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_verification_for_update(
    transaction: &PostgresTransaction,
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    principal_id: PrincipalId,
    contact_id: RecipientContactId,
    verification_id: RecipientContactVerificationId,
) -> Result<Option<RecipientContactVerification>, PostgresPersistenceError> {
    fetch_optional::<RecipientContactVerificationRow, _>(
        transaction,
        sql_query::<RecipientContactVerificationRow>(verification_select())
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and principal_id = ")
            .bind(principal_id.as_uuid())
            .append(" and contact_id = ")
            .bind(contact_id.as_uuid())
            .append(" and id = ")
            .bind(verification_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_verification)
    .transpose()
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn store_recipient_contact_audit(
    transaction: &PostgresTransaction,
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    actor_principal_id: PrincipalId,
    contact: &RecipientContactRecord,
    verification: Option<&RecipientContactVerification>,
    action: &'static str,
    occurred_at: DateTime<Utc>,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: AuditWrite::organization_scope(organization_id.as_uuid()),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: contact.id.as_uuid(),
            occurred_at,
            request_id,
            details: serde_json::json!({
                "contactId": contact.id,
                "principalId": contact.principal_id,
                "state": contact.status.as_str(),
                "addressDigest": contact.address_digest,
                "contactVersion": contact.aggregate_version,
                "challengeId": verification.map(|value| value.id),
                "signingKeyId": verification.map(|value| value.signing_key_id.as_str()),
                "challengeIssuedAt": verification.map(|value| value.issued_at),
                "challengeExpiresAt": verification.map(|value| value.expires_at),
                "verifiedAt": contact.verified_at,
                "revokedAt": contact.revoked_at,
            }),
        },
    )
    .await
}
