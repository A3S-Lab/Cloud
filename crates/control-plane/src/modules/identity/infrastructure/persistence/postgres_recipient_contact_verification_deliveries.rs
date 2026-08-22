use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_memberships::{
    load_active_membership_for_update, load_principal, lock_membership_set,
};
use super::postgres_recipient_contact_support::{
    load_contact_for_update, load_verification_for_update,
};
use crate::infrastructure::{execute, fetch_optional, transaction_error, PostgresPersistenceError};
use crate::modules::identity::domain::entities::{
    IdentityPrincipalKind, RecipientContactStatus, RecipientContactVerification,
    RecipientContactVerificationDeliveryFact, RecipientContactVerificationDeliveryOutcome,
    RecipientContactVerificationDeliveryRecord, RecipientContactVerificationDeliveryReservation,
    RecipientContactVerificationDeliveryStatus, RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::repositories::{
    IRecipientContactVerificationDeliveryRepository, RecipientContactVerificationDeliveryAdmission,
    RecipientContactVerificationDispatchStart,
};
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, RecipientContactVerificationId, RepositoryError,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, PostgresDialect, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct RecipientContactVerificationDeliveryRow {
    verification_id: Uuid,
    state: String,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    dispatch_started_at: Option<DateTime<Utc>>,
    settled_at: Option<DateTime<Utc>>,
}

impl FromRow for RecipientContactVerificationDeliveryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            verification_id: decode_column(row, 0)?,
            state: decode_column(row, 1)?,
            fence_token: decode_column(row, 2)?,
            reserved_at: decode_column(row, 3)?,
            lease_expires_at: decode_column(row, 4)?,
            dispatch_started_at: decode_column(row, 5)?,
            settled_at: decode_column(row, 6)?,
        })
    }
}

fn delivery_select() -> &'static str {
    "select verification_id, state, fence_token, reserved_at, lease_expires_at, dispatch_started_at, settled_at from recipient_contact_verification_deliveries"
}

fn decode_delivery(
    row: RecipientContactVerificationDeliveryRow,
) -> Result<RecipientContactVerificationDeliveryRecord, RepositoryError> {
    RecipientContactVerificationDeliveryRecord::restore(
        RecipientContactVerificationId::from_uuid(row.verification_id),
        RecipientContactVerificationDeliveryStatus::parse(&row.state)
            .map_err(RepositoryError::Storage)?,
        row.fence_token,
        row.reserved_at,
        row.lease_expires_at,
        row.dispatch_started_at,
        row.settled_at,
    )
    .map_err(RepositoryError::Storage)
}

async fn load_delivery_for_update(
    transaction: &PostgresTransaction,
    verification_id: RecipientContactVerificationId,
) -> Result<Option<RecipientContactVerificationDeliveryRecord>, PostgresPersistenceError> {
    fetch_optional::<RecipientContactVerificationDeliveryRow, _>(
        transaction,
        sql_query::<RecipientContactVerificationDeliveryRow>(delivery_select())
            .append(" where verification_id = ")
            .bind(verification_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_delivery)
    .transpose()
    .map_err(Into::into)
}

enum DeliveryAuthority {
    Current {
        verification: RecipientContactVerification,
        address: RecipientEmailAddress,
    },
    Obsolete,
    InvalidFact,
}

async fn delivery_authority(
    transaction: &PostgresTransaction,
    fact: &RecipientContactVerificationDeliveryFact,
    now: DateTime<Utc>,
) -> Result<DeliveryAuthority, PostgresPersistenceError> {
    lock_membership_set(transaction, fact.organization_id).await?;
    let membership = load_active_membership_for_update(
        transaction,
        fact.organization_id,
        fact.verification.principal_id,
    )
    .await?;
    let principal = load_principal(transaction, fact.verification.principal_id).await?;
    let contact = load_contact_for_update(
        transaction,
        fact.verification.principal_id,
        fact.verification.contact_id,
    )
    .await?;
    let verification = load_verification_for_update(
        transaction,
        fact.organization_id,
        fact.verification.principal_id,
        fact.verification.contact_id,
        fact.verification.id,
    )
    .await?;
    let Some(verification) = verification else {
        return Ok(DeliveryAuthority::InvalidFact);
    };
    if verification.claims() != fact.verification.claims() {
        return Ok(DeliveryAuthority::InvalidFact);
    }
    let principal_active = principal.is_some_and(|principal| {
        principal.kind == IdentityPrincipalKind::Human && principal.is_active()
    });
    let contact_current = contact.as_ref().is_some_and(|contact| {
        contact.principal_id == verification.principal_id
            && contact.status == RecipientContactStatus::Pending
            && contact.aggregate_version == verification.contact_version
            && contact.address.digest() == verification.address_digest
    });
    if membership.is_none()
        || !principal_active
        || !contact_current
        || verification.status_at(now) != RecipientContactVerificationStatus::Pending
    {
        return Ok(DeliveryAuthority::Obsolete);
    }
    let Some(contact) = contact else {
        return Ok(DeliveryAuthority::Obsolete);
    };
    Ok(DeliveryAuthority::Current {
        verification,
        address: contact.address,
    })
}

async fn settle_dispatching_as_indeterminate(
    transaction: &PostgresTransaction,
    record: &mut RecipientContactVerificationDeliveryRecord,
    settled_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let dispatch_started_at = record.dispatch_started_at.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "dispatching recipient contact verification delivery has no start time".into(),
        )
    })?;
    let settled_at = settled_at.max(dispatch_started_at);
    let rows = execute(
        transaction,
        sql_query::<()>(
            "update recipient_contact_verification_deliveries set state = 'indeterminate', settled_at = ",
        )
        .bind(settled_at)
        .append(" where verification_id = ")
        .bind(record.verification_id.as_uuid())
        .append(" and state = 'dispatching' and fence_token = ")
        .bind(record.fence_token),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "settling replayed recipient contact verification delivery affected {rows} rows"
        )));
    }
    record.status = RecipientContactVerificationDeliveryStatus::Indeterminate;
    record.settled_at = Some(settled_at);
    record
        .validate()
        .map_err(PostgresPersistenceError::Invariant)
}

async fn make_obsolete(
    transaction: &PostgresTransaction,
    record: &mut RecipientContactVerificationDeliveryRecord,
    settled_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let settled_at = settled_at.max(record.reserved_at);
    let rows = execute(
        transaction,
        sql_query::<()>(
            "update recipient_contact_verification_deliveries set state = 'obsolete', settled_at = ",
        )
        .bind(settled_at)
        .append(" where verification_id = ")
        .bind(record.verification_id.as_uuid())
        .append(" and state = 'reserved' and fence_token = ")
        .bind(record.fence_token),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "obsoleting recipient contact verification delivery affected {rows} rows"
        )));
    }
    record.status = RecipientContactVerificationDeliveryStatus::Obsolete;
    record.settled_at = Some(settled_at);
    record
        .validate()
        .map_err(PostgresPersistenceError::Invariant)
}

#[async_trait]
impl IRecipientContactVerificationDeliveryRepository for PostgresIdentityRepository {
    async fn reserve_recipient_contact_verification_delivery(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDeliveryAdmission, RepositoryError> {
        if fact.validate().is_err() {
            return Ok(RecipientContactVerificationDeliveryAdmission::InvalidFact);
        }
        let fact = fact.clone();
        let reserved_at = canonical_timestamp(reserved_at);
        let lease_expires_at = canonical_timestamp(lease_expires_at);
        if fence_token.is_nil() || lease_expires_at <= reserved_at {
            return Err(RepositoryError::Storage(
                "recipient contact verification delivery reservation is invalid".into(),
            ));
        }
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut existing = load_delivery_for_update(transaction, fact.id()).await?;
                    if let Some(record) = existing.as_mut() {
                        if record.status.is_terminal() {
                            return Ok(RecipientContactVerificationDeliveryAdmission::Terminal(
                                record.status,
                            ));
                        }
                        if record.status
                            == RecipientContactVerificationDeliveryStatus::Dispatching
                        {
                            settle_dispatching_as_indeterminate(
                                transaction,
                                record,
                                reserved_at,
                            )
                            .await?;
                            return Ok(RecipientContactVerificationDeliveryAdmission::Terminal(
                                record.status,
                            ));
                        }
                        if reserved_at < record.lease_expires_at {
                            return Ok(RecipientContactVerificationDeliveryAdmission::Deferred {
                                lease_expires_at: record.lease_expires_at,
                            });
                        }
                    }

                    let authority = delivery_authority(transaction, &fact, reserved_at).await?;
                    if matches!(authority, DeliveryAuthority::InvalidFact) {
                        return Ok(RecipientContactVerificationDeliveryAdmission::InvalidFact);
                    }

                    let mut record = if existing.is_some() {
                        let rows = execute(
                            transaction,
                            sql_query::<()>(
                                "update recipient_contact_verification_deliveries set state = 'reserved', fence_token = ",
                            )
                            .bind(fence_token)
                            .append(", reserved_at = ")
                            .bind(reserved_at)
                            .append(", lease_expires_at = ")
                            .bind(lease_expires_at)
                            .append(", dispatch_started_at = null, settled_at = null where verification_id = ")
                            .bind(fact.id().as_uuid())
                            .append(" and state = 'reserved' and lease_expires_at <= ")
                            .bind(reserved_at),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "renewing recipient contact verification delivery affected {rows} rows"
                            )));
                        }
                        RecipientContactVerificationDeliveryRecord::restore(
                            fact.id(),
                            RecipientContactVerificationDeliveryStatus::Reserved,
                            fence_token,
                            reserved_at,
                            lease_expires_at,
                            None,
                            None,
                        )
                        .map_err(PostgresPersistenceError::Invariant)?
                    } else {
                        let initial_status = if matches!(authority, DeliveryAuthority::Obsolete) {
                            RecipientContactVerificationDeliveryStatus::Obsolete
                        } else {
                            RecipientContactVerificationDeliveryStatus::Reserved
                        };
                        let settled_at = (initial_status
                            == RecipientContactVerificationDeliveryStatus::Obsolete)
                            .then_some(reserved_at);
                        let rows = execute(
                            transaction,
                            sql_query::<()>(
                                "insert into recipient_contact_verification_deliveries (verification_id, state, fence_token, reserved_at, lease_expires_at, dispatch_started_at, settled_at) values (",
                            )
                            .bind(fact.id().as_uuid())
                            .append(", ")
                            .bind(initial_status.as_str())
                            .append(", ")
                            .bind(fence_token)
                            .append(", ")
                            .bind(reserved_at)
                            .append(", ")
                            .bind(lease_expires_at)
                            .append(", null, ")
                            .bind(settled_at)
                            .append(") on conflict (verification_id) do nothing"),
                        )
                        .await?;
                        if rows == 0 {
                            let concurrent = load_delivery_for_update(transaction, fact.id())
                                .await?
                                .ok_or_else(|| {
                                    PostgresPersistenceError::Invariant(
                                        "concurrent delivery reservation disappeared".into(),
                                    )
                                })?;
                            if concurrent.status.is_terminal() {
                                return Ok(
                                    RecipientContactVerificationDeliveryAdmission::Terminal(
                                        concurrent.status,
                                    ),
                                );
                            }
                            return Ok(RecipientContactVerificationDeliveryAdmission::Deferred {
                                lease_expires_at: concurrent.lease_expires_at,
                            });
                        }
                        if rows != 1 {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "reserving recipient contact verification delivery affected {rows} rows"
                            )));
                        }
                        RecipientContactVerificationDeliveryRecord::restore(
                            fact.id(),
                            initial_status,
                            fence_token,
                            reserved_at,
                            lease_expires_at,
                            None,
                            settled_at,
                        )
                        .map_err(PostgresPersistenceError::Invariant)?
                    };

                    match authority {
                        DeliveryAuthority::InvalidFact => Ok(
                            RecipientContactVerificationDeliveryAdmission::InvalidFact,
                        ),
                        DeliveryAuthority::Obsolete => {
                            if record.status
                                == RecipientContactVerificationDeliveryStatus::Reserved
                            {
                                make_obsolete(transaction, &mut record, reserved_at).await?;
                            }
                            Ok(RecipientContactVerificationDeliveryAdmission::Terminal(
                                RecipientContactVerificationDeliveryStatus::Obsolete,
                            ))
                        }
                        DeliveryAuthority::Current {
                            verification,
                            address,
                        } => Ok(RecipientContactVerificationDeliveryAdmission::Reserved(
                            RecipientContactVerificationDeliveryReservation {
                                verification,
                                address,
                                fence_token,
                                lease_expires_at,
                            },
                        )),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn start_recipient_contact_verification_dispatch(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
        fence_token: Uuid,
        started_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDispatchStart, RepositoryError> {
        let fact = fact.clone();
        let started_at = canonical_timestamp(started_at);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(mut record) = load_delivery_for_update(transaction, fact.id()).await?
                    else {
                        return Ok(RecipientContactVerificationDispatchStart::Deferred);
                    };
                    if record.status.is_terminal() {
                        return Ok(RecipientContactVerificationDispatchStart::Terminal(
                            record.status,
                        ));
                    }
                    if record.status == RecipientContactVerificationDeliveryStatus::Dispatching {
                        settle_dispatching_as_indeterminate(
                            transaction,
                            &mut record,
                            started_at,
                        )
                        .await?;
                        return Ok(RecipientContactVerificationDispatchStart::Terminal(
                            record.status,
                        ));
                    }
                    if record.fence_token != fence_token
                        || started_at >= record.lease_expires_at
                    {
                        return Ok(RecipientContactVerificationDispatchStart::Deferred);
                    }
                    match delivery_authority(transaction, &fact, started_at).await? {
                        DeliveryAuthority::InvalidFact => {
                            return Ok(RecipientContactVerificationDispatchStart::Deferred)
                        }
                        DeliveryAuthority::Obsolete => {
                            make_obsolete(transaction, &mut record, started_at).await?;
                            return Ok(RecipientContactVerificationDispatchStart::Terminal(
                                record.status,
                            ));
                        }
                        DeliveryAuthority::Current { .. } => {}
                    }
                    let started_at = started_at.max(record.reserved_at);
                    let rows = execute(
                        transaction,
                        sql_query::<()>(
                            "update recipient_contact_verification_deliveries set state = 'dispatching', dispatch_started_at = ",
                        )
                        .bind(started_at)
                        .append(" where verification_id = ")
                        .bind(fact.id().as_uuid())
                        .append(" and state = 'reserved' and fence_token = ")
                        .bind(fence_token)
                        .append(" and lease_expires_at > ")
                        .bind(started_at),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "starting recipient contact verification dispatch affected {rows} rows"
                        )));
                    }
                    Ok(RecipientContactVerificationDispatchStart::Authorized)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn settle_recipient_contact_verification_delivery(
        &self,
        verification_id: RecipientContactVerificationId,
        fence_token: Uuid,
        outcome: RecipientContactVerificationDeliveryOutcome,
        settled_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDeliveryRecord, RepositoryError> {
        let settled_at = canonical_timestamp(settled_at);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut record = load_delivery_for_update(transaction, verification_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if record.status == outcome.status() && record.fence_token == fence_token {
                        return Ok(record);
                    }
                    if record.status != RecipientContactVerificationDeliveryStatus::Dispatching
                        || record.fence_token != fence_token
                    {
                        return Err(RepositoryError::Conflict(
                            "recipient contact verification delivery crossed its dispatch fence"
                                .into(),
                        )
                        .into());
                    }
                    let dispatch_started_at = record.dispatch_started_at.ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "recipient contact verification dispatch has no start time".into(),
                        )
                    })?;
                    let settled_at = settled_at.max(dispatch_started_at);
                    let rows = execute(
                        transaction,
                        sql_query::<()>(
                            "update recipient_contact_verification_deliveries set state = ",
                        )
                        .bind(outcome.status().as_str())
                        .append(", settled_at = ")
                        .bind(settled_at)
                        .append(" where verification_id = ")
                        .bind(verification_id.as_uuid())
                        .append(" and state = 'dispatching' and fence_token = ")
                        .bind(fence_token),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "settling recipient contact verification delivery affected {rows} rows"
                        )));
                    }
                    record.status = outcome.status();
                    record.settled_at = Some(settled_at);
                    record
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    Ok(record)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_recipient_contact_verification_delivery(
        &self,
        verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerificationDeliveryRecord>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<RecipientContactVerificationDeliveryRow>(delivery_select())
                    .append(" where verification_id = ")
                    .bind(verification_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_delivery)
            .transpose()
    }
}
