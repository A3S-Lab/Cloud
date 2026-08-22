use super::PostgresNotificationRepository;
use crate::infrastructure::{execute, fetch_optional, transaction_error, PostgresPersistenceError};
use crate::modules::notifications::domain::{
    outbound_notification_smtp_attempt_id, IOutboundNotificationSmtpAttemptRepository,
    OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationSmtpAttemptAdmission, OutboundNotificationSmtpAttemptOutcome,
    OutboundNotificationSmtpAttemptRecord, OutboundNotificationSmtpAttemptSettlement,
    OutboundNotificationSmtpAttemptState, OutboundNotificationSmtpDispatchStart,
    OutboundNotificationTerminalReceipt, MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS,
    MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, RecipientContactId, RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresTransaction, Row,
};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

const SELECT_SMTP_ATTEMPTS: &str = "select organization_id, delivery_id, recipient_contact_id, generation, attempt_id, state, outcome, fence_generation, fence_token, reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, completed_at from notification_outbound_smtp_attempts";
const SELECT_SMTP_DELIVERIES: &str = "select organization_id, id, notification_id, recipient_principal_id, requested_event_id, payload_digest, maximum_provider_attempts, channel, connector_project_id, connector_environment_id, connector_profile_id, connector_revision_id, recipient_contact_id, occurred_at, terminal_outcome, terminal_generation, terminal_attempt_id, terminal_at from notification_outbound_deliveries";

struct SmtpAttemptRow {
    organization_id: Uuid,
    delivery_id: Uuid,
    recipient_contact_id: Uuid,
    generation: u64,
    attempt_id: Uuid,
    state: String,
    outcome: Option<String>,
    fence_generation: u64,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    dispatch_started_at: Option<DateTime<Utc>>,
    outcome_deadline_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

impl FromRow for SmtpAttemptRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            delivery_id: decode(row, 1)?,
            recipient_contact_id: decode(row, 2)?,
            generation: decode(row, 3)?,
            attempt_id: decode(row, 4)?,
            state: decode(row, 5)?,
            outcome: decode(row, 6)?,
            fence_generation: decode(row, 7)?,
            fence_token: decode(row, 8)?,
            reserved_at: decode(row, 9)?,
            lease_expires_at: decode(row, 10)?,
            dispatch_started_at: decode(row, 11)?,
            outcome_deadline_at: decode(row, 12)?,
            completed_at: decode(row, 13)?,
        })
    }
}

struct SmtpDeliveryRow {
    organization_id: Uuid,
    id: Uuid,
    notification_id: Uuid,
    recipient_principal_id: Uuid,
    requested_event_id: Uuid,
    payload_digest: String,
    maximum_provider_attempts: u64,
    channel: String,
    connector_project_id: Option<Uuid>,
    connector_environment_id: Option<Uuid>,
    connector_profile_id: Option<Uuid>,
    connector_revision_id: Option<Uuid>,
    recipient_contact_id: Option<Uuid>,
    occurred_at: DateTime<Utc>,
    terminal_outcome: Option<String>,
    terminal_generation: Option<u64>,
    terminal_attempt_id: Option<Uuid>,
    terminal_at: Option<DateTime<Utc>>,
}

impl FromRow for SmtpDeliveryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            notification_id: decode(row, 2)?,
            recipient_principal_id: decode(row, 3)?,
            requested_event_id: decode(row, 4)?,
            payload_digest: decode(row, 5)?,
            maximum_provider_attempts: decode(row, 6)?,
            channel: decode(row, 7)?,
            connector_project_id: decode(row, 8)?,
            connector_environment_id: decode(row, 9)?,
            connector_profile_id: decode(row, 10)?,
            connector_revision_id: decode(row, 11)?,
            recipient_contact_id: decode(row, 12)?,
            occurred_at: decode(row, 13)?,
            terminal_outcome: decode(row, 14)?,
            terminal_generation: decode(row, 15)?,
            terminal_attempt_id: decode(row, 16)?,
            terminal_at: decode(row, 17)?,
        })
    }
}

struct RecipientAuthorityRow {
    contact_id: Uuid,
}

impl FromRow for RecipientAuthorityRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            contact_id: decode(row, 0)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn decode_attempt(
    row: SmtpAttemptRow,
) -> Result<OutboundNotificationSmtpAttemptRecord, PostgresPersistenceError> {
    OutboundNotificationSmtpAttemptRecord::restore(
        OrganizationId::from_uuid(row.organization_id),
        row.delivery_id,
        RecipientContactId::from_uuid(row.recipient_contact_id),
        row.generation,
        row.attempt_id,
        OutboundNotificationSmtpAttemptState::parse(&row.state)
            .map_err(PostgresPersistenceError::Invariant)?,
        row.outcome
            .as_deref()
            .map(OutboundNotificationSmtpAttemptOutcome::parse)
            .transpose()
            .map_err(PostgresPersistenceError::Invariant)?,
        row.fence_generation,
        row.fence_token,
        row.reserved_at,
        row.lease_expires_at,
        row.dispatch_started_at,
        row.outcome_deadline_at,
        row.completed_at,
    )
    .map_err(PostgresPersistenceError::Invariant)
}

fn delivery_matches(
    row: &SmtpDeliveryRow,
    delivery: &OutboundNotificationDelivery,
) -> Result<bool, PostgresPersistenceError> {
    let payload_digest = Sha256Digest::from_bytes(
        &delivery
            .canonical_payload()
            .map_err(PostgresPersistenceError::Invariant)?,
    );
    Ok(row.organization_id == delivery.organization_id().as_uuid()
        && row.id == delivery.id()
        && row.notification_id == delivery.notification_id().as_uuid()
        && row.recipient_principal_id == delivery.recipient_principal_id().as_uuid()
        && row.requested_event_id == delivery.requested_event_id()
        && row.payload_digest == payload_digest.as_str()
        && row.maximum_provider_attempts == delivery.maximum_provider_attempts()
        && row.channel == OutboundNotificationChannel::Smtp.as_str()
        && row.connector_project_id.is_none()
        && row.connector_environment_id.is_none()
        && row.connector_profile_id.is_none()
        && row.connector_revision_id.is_none()
        && row.recipient_contact_id == delivery.recipient_contact_id().map(|value| value.as_uuid())
        && row.occurred_at == delivery.occurred_at())
}

fn decode_delivery_receipt(
    row: &SmtpDeliveryRow,
) -> Result<Option<OutboundNotificationTerminalReceipt>, PostgresPersistenceError> {
    match (
        row.terminal_outcome.as_deref(),
        row.terminal_generation,
        row.terminal_attempt_id,
        row.terminal_at,
        row.recipient_contact_id,
    ) {
        (None, None, None, None, Some(_)) => Ok(None),
        (
            Some(outcome),
            Some(generation),
            Some(attempt_id),
            Some(terminal_at),
            Some(contact_id),
        ) => OutboundNotificationTerminalReceipt::restore_with_provider_attempt_budget(
            OrganizationId::from_uuid(row.organization_id),
            row.id,
            crate::modules::notifications::domain::OutboundNotificationTarget::RecipientContact(
                RecipientContactId::from_uuid(contact_id),
            ),
            row.maximum_provider_attempts,
            crate::modules::notifications::domain::OutboundNotificationTerminalOutcome::parse(
                outcome,
            )
            .map_err(PostgresPersistenceError::Invariant)?,
            generation,
            attempt_id,
            terminal_at,
        )
        .map(Some)
        .map_err(PostgresPersistenceError::Invariant),
        _ => Err(PostgresPersistenceError::Invariant(
            "stored SMTP delivery terminal receipt is incomplete".into(),
        )),
    }
}

async fn load_delivery_for_update(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
) -> Result<Option<SmtpDeliveryRow>, PostgresPersistenceError> {
    fetch_optional::<SmtpDeliveryRow, _>(
        transaction,
        sql_query::<SmtpDeliveryRow>(SELECT_SMTP_DELIVERIES)
            .append(" where organization_id = ")
            .bind(delivery.organization_id().as_uuid())
            .append(" and id = ")
            .bind(delivery.id())
            .append(" for update"),
    )
    .await
}

async fn load_attempt_for_update(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
    generation: u64,
) -> Result<Option<OutboundNotificationSmtpAttemptRecord>, PostgresPersistenceError> {
    fetch_optional::<SmtpAttemptRow, _>(
        transaction,
        sql_query::<SmtpAttemptRow>(SELECT_SMTP_ATTEMPTS)
            .append(" where organization_id = ")
            .bind(delivery.organization_id().as_uuid())
            .append(" and delivery_id = ")
            .bind(delivery.id())
            .append(" and generation = ")
            .bind(generation)
            .append(" for update"),
    )
    .await?
    .map(decode_attempt)
    .transpose()
}

async fn recipient_authority_is_current(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
) -> Result<bool, PostgresPersistenceError> {
    let contact_id = delivery.recipient_contact_id().ok_or_else(|| {
        PostgresPersistenceError::Invariant("SMTP delivery has no recipient contact".into())
    })?;
    let authority = fetch_optional::<RecipientAuthorityRow, _>(
        transaction,
        sql_query::<RecipientAuthorityRow>(
            "select c.id from recipient_contacts c join identity_principals p on p.id = c.principal_id join organization_memberships m on m.principal_id = c.principal_id and m.organization_id = ",
        )
        .bind(delivery.organization_id().as_uuid())
        .append(" where c.id = ")
        .bind(contact_id.as_uuid())
        .append(" and c.principal_id = ")
        .bind(delivery.recipient_principal_id().as_uuid())
        .append(" and c.state = 'verified' and p.kind = 'human' and p.disabled_at is null and m.revoked_at is null for share of c, p, m"),
    )
    .await?;
    Ok(authority.is_some_and(|row| row.contact_id == contact_id.as_uuid()))
}

fn validate_attempt_against_delivery(
    attempt: &OutboundNotificationSmtpAttemptRecord,
    delivery: &OutboundNotificationDelivery,
) -> Result<(), PostgresPersistenceError> {
    attempt
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if attempt.organization_id != delivery.organization_id()
        || attempt.delivery_id != delivery.id()
        || Some(attempt.recipient_contact_id) != delivery.recipient_contact_id()
        || attempt.generation > delivery.maximum_provider_attempts()
    {
        return Err(RepositoryError::Conflict(
            "SMTP attempt does not match its exact delivery".into(),
        )
        .into());
    }
    Ok(())
}

fn terminal_receipt(
    delivery: &OutboundNotificationDelivery,
    attempt: &OutboundNotificationSmtpAttemptRecord,
) -> Result<Option<OutboundNotificationTerminalReceipt>, PostgresPersistenceError> {
    let outcome = attempt.outcome.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "terminal SMTP notification attempt has no outcome".into(),
        )
    })?;
    let completed_at = attempt.completed_at.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "terminal SMTP notification attempt has no completion time".into(),
        )
    })?;
    OutboundNotificationTerminalReceipt::from_smtp_outcome(
        delivery,
        attempt.generation,
        outcome,
        completed_at,
    )
    .map_err(PostgresPersistenceError::Invariant)
}

async fn persist_receipt(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
    receipt: &OutboundNotificationTerminalReceipt,
) -> Result<(), PostgresPersistenceError> {
    receipt
        .validate_against(delivery)
        .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        sql_query::<()>(
            "update notification_outbound_deliveries set terminal_outcome = ",
        )
        .bind(receipt.outcome().as_str())
        .append(", terminal_generation = ")
        .bind(receipt.generation())
        .append(", terminal_attempt_id = ")
        .bind(receipt.attempt_id())
        .append(", terminal_at = ")
        .bind(receipt.terminal_at())
        .append(" where organization_id = ")
        .bind(delivery.organization_id().as_uuid())
        .append(" and id = ")
        .bind(delivery.id())
        .append(" and terminal_outcome is null and terminal_generation is null and terminal_attempt_id is null and terminal_at is null"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "settling SMTP notification delivery affected {rows} rows"
        )));
    }
    Ok(())
}

async fn insert_attempt(
    transaction: &PostgresTransaction,
    attempt: &OutboundNotificationSmtpAttemptRecord,
) -> Result<(), PostgresPersistenceError> {
    attempt
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into notification_outbound_smtp_attempts (organization_id, delivery_id, recipient_contact_id, generation, attempt_id, state, outcome, fence_generation, fence_token, reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, completed_at) values (",
        )
        .bind(attempt.organization_id.as_uuid())
        .append(", ")
        .bind(attempt.delivery_id)
        .append(", ")
        .bind(attempt.recipient_contact_id.as_uuid())
        .append(", ")
        .bind(attempt.generation)
        .append(", ")
        .bind(attempt.attempt_id)
        .append(", ")
        .bind(attempt.state.as_str())
        .append(", ")
        .bind(attempt.outcome.map(|value| value.as_str()))
        .append(", ")
        .bind(attempt.fence_generation)
        .append(", ")
        .bind(attempt.fence_token)
        .append(", ")
        .bind(attempt.reserved_at)
        .append(", ")
        .bind(attempt.lease_expires_at)
        .append(", ")
        .bind(attempt.dispatch_started_at)
        .append(", ")
        .bind(attempt.outcome_deadline_at)
        .append(", ")
        .bind(attempt.completed_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "inserting SMTP notification attempt affected {rows} rows"
        )));
    }
    Ok(())
}

async fn take_over_reservation(
    transaction: &PostgresTransaction,
    existing: &OutboundNotificationSmtpAttemptRecord,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> Result<OutboundNotificationSmtpAttemptRecord, PostgresPersistenceError> {
    let fence_generation = existing.fence_generation.checked_add(1).ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "outbound SMTP notification fence generation overflowed".into(),
        )
    })?;
    let replacement = OutboundNotificationSmtpAttemptRecord::restore(
        existing.organization_id,
        existing.delivery_id,
        existing.recipient_contact_id,
        existing.generation,
        existing.attempt_id,
        OutboundNotificationSmtpAttemptState::Reserved,
        None,
        fence_generation,
        fence_token,
        reserved_at,
        lease_expires_at,
        None,
        None,
        None,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        sql_query::<()>("update notification_outbound_smtp_attempts set fence_generation = ")
            .bind(fence_generation)
            .append(", fence_token = ")
            .bind(fence_token)
            .append(", reserved_at = ")
            .bind(reserved_at)
            .append(", lease_expires_at = ")
            .bind(lease_expires_at)
            .append(" where organization_id = ")
            .bind(existing.organization_id.as_uuid())
            .append(" and delivery_id = ")
            .bind(existing.delivery_id)
            .append(" and generation = ")
            .bind(existing.generation)
            .append(" and state = 'reserved' and fence_generation = ")
            .bind(existing.fence_generation)
            .append(" and fence_token = ")
            .bind(existing.fence_token)
            .append(" and lease_expires_at <= ")
            .bind(reserved_at),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "taking over SMTP notification reservation affected {rows} rows"
        )));
    }
    Ok(replacement)
}

async fn settle_attempt_record(
    transaction: &PostgresTransaction,
    existing: &OutboundNotificationSmtpAttemptRecord,
    outcome: OutboundNotificationSmtpAttemptOutcome,
    completed_at: DateTime<Utc>,
) -> Result<OutboundNotificationSmtpAttemptRecord, PostgresPersistenceError> {
    let (dispatch_started_at, outcome_deadline_at) = match existing.state {
        OutboundNotificationSmtpAttemptState::Reserved
            if outcome == OutboundNotificationSmtpAttemptOutcome::Obsolete =>
        {
            (None, None)
        }
        OutboundNotificationSmtpAttemptState::Dispatching
            if outcome != OutboundNotificationSmtpAttemptOutcome::Obsolete =>
        {
            (existing.dispatch_started_at, existing.outcome_deadline_at)
        }
        _ => {
            return Err(RepositoryError::Conflict(
                "SMTP notification attempt cannot settle from its current state".into(),
            )
            .into())
        }
    };
    let terminal = OutboundNotificationSmtpAttemptRecord::restore(
        existing.organization_id,
        existing.delivery_id,
        existing.recipient_contact_id,
        existing.generation,
        existing.attempt_id,
        OutboundNotificationSmtpAttemptState::Terminal,
        Some(outcome),
        existing.fence_generation,
        existing.fence_token,
        existing.reserved_at,
        existing.lease_expires_at,
        dispatch_started_at,
        outcome_deadline_at,
        Some(completed_at),
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        sql_query::<()>(
            "update notification_outbound_smtp_attempts set state = 'terminal', outcome = ",
        )
        .bind(outcome.as_str())
        .append(", completed_at = ")
        .bind(completed_at)
        .append(" where organization_id = ")
        .bind(existing.organization_id.as_uuid())
        .append(" and delivery_id = ")
        .bind(existing.delivery_id)
        .append(" and generation = ")
        .bind(existing.generation)
        .append(" and state = ")
        .bind(existing.state.as_str())
        .append(" and fence_generation = ")
        .bind(existing.fence_generation)
        .append(" and fence_token = ")
        .bind(existing.fence_token),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "settling SMTP notification attempt affected {rows} rows"
        )));
    }
    Ok(terminal)
}

async fn settle_with_receipt(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
    existing: &OutboundNotificationSmtpAttemptRecord,
    outcome: OutboundNotificationSmtpAttemptOutcome,
    completed_at: DateTime<Utc>,
) -> Result<OutboundNotificationSmtpAttemptSettlement, PostgresPersistenceError> {
    let terminal = settle_attempt_record(transaction, existing, outcome, completed_at).await?;
    let receipt = terminal_receipt(delivery, &terminal)?;
    if let Some(receipt) = &receipt {
        persist_receipt(transaction, delivery, receipt).await?;
    }
    Ok(OutboundNotificationSmtpAttemptSettlement {
        attempt: terminal,
        receipt,
    })
}

async fn recover_indeterminate(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
    existing: &OutboundNotificationSmtpAttemptRecord,
) -> Result<OutboundNotificationTerminalReceipt, PostgresPersistenceError> {
    let deadline = existing.outcome_deadline_at.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "dispatching SMTP notification attempt has no outcome deadline".into(),
        )
    })?;
    let settlement = settle_with_receipt(
        transaction,
        delivery,
        existing,
        OutboundNotificationSmtpAttemptOutcome::Indeterminate,
        deadline,
    )
    .await?;
    settlement.receipt.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "indeterminate SMTP notification attempt produced no receipt".into(),
        )
    })
}

fn valid_reservation(
    delivery: &OutboundNotificationDelivery,
    generation: u64,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> bool {
    delivery.validate().is_ok()
        && delivery.channel() == OutboundNotificationChannel::Smtp
        && delivery.recipient_contact_id().is_some()
        && generation > 0
        && generation <= delivery.maximum_provider_attempts()
        && !fence_token.is_nil()
        && reserved_at == canonical_timestamp(reserved_at)
        && lease_expires_at == canonical_timestamp(lease_expires_at)
        && reserved_at >= delivery.occurred_at()
        && lease_expires_at > reserved_at
        && lease_expires_at - reserved_at
            <= Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_LEASE_SECONDS)
}

fn valid_dispatch_start(
    delivery: &OutboundNotificationDelivery,
    generation: u64,
    fence_token: Uuid,
    started_at: DateTime<Utc>,
    outcome_deadline_at: DateTime<Utc>,
) -> bool {
    delivery.validate().is_ok()
        && delivery.channel() == OutboundNotificationChannel::Smtp
        && generation > 0
        && generation <= delivery.maximum_provider_attempts()
        && !fence_token.is_nil()
        && started_at == canonical_timestamp(started_at)
        && outcome_deadline_at == canonical_timestamp(outcome_deadline_at)
        && started_at >= delivery.occurred_at()
        && outcome_deadline_at > started_at
        && outcome_deadline_at - started_at
            <= Duration::seconds(MAXIMUM_OUTBOUND_NOTIFICATION_SMTP_OUTCOME_SECONDS)
}

fn terminal_admission(
    delivery: &OutboundNotificationDelivery,
    delivery_row: &SmtpDeliveryRow,
    attempt: OutboundNotificationSmtpAttemptRecord,
) -> Result<OutboundNotificationSmtpAttemptAdmission, PostgresPersistenceError> {
    validate_attempt_against_delivery(&attempt, delivery)?;
    let expected = terminal_receipt(delivery, &attempt)?;
    let stored = decode_delivery_receipt(delivery_row)?;
    match expected {
        Some(expected) if stored.as_ref() == Some(&expected) => {
            Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(expected))
        }
        Some(_) => Err(PostgresPersistenceError::Invariant(
            "terminal SMTP attempt has no matching atomic delivery receipt".into(),
        )),
        None if stored.is_none() => {
            Ok(OutboundNotificationSmtpAttemptAdmission::Retryable(attempt))
        }
        None => Err(PostgresPersistenceError::Invariant(
            "retryable SMTP attempt unexpectedly has a delivery receipt".into(),
        )),
    }
}

fn terminal_start(
    delivery: &OutboundNotificationDelivery,
    delivery_row: &SmtpDeliveryRow,
    attempt: OutboundNotificationSmtpAttemptRecord,
) -> Result<OutboundNotificationSmtpDispatchStart, PostgresPersistenceError> {
    match terminal_admission(delivery, delivery_row, attempt)? {
        OutboundNotificationSmtpAttemptAdmission::Terminal(receipt) => {
            Ok(OutboundNotificationSmtpDispatchStart::Terminal(receipt))
        }
        OutboundNotificationSmtpAttemptAdmission::Retryable(_) => Err(RepositoryError::Conflict(
            "retryable SMTP attempt cannot restart dispatch".into(),
        )
        .into()),
        _ => Err(PostgresPersistenceError::Invariant(
            "terminal SMTP start produced a nonterminal admission".into(),
        )),
    }
}

async fn validate_prior_generation(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
    generation: u64,
) -> Result<(), PostgresPersistenceError> {
    if generation == 1 {
        return Ok(());
    }
    let prior = load_attempt_for_update(transaction, delivery, generation - 1).await?;
    if prior.is_some_and(|attempt| {
        attempt.state == OutboundNotificationSmtpAttemptState::Terminal
            && attempt.outcome == Some(OutboundNotificationSmtpAttemptOutcome::Retryable)
    }) {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(
            "SMTP attempt requires exact prior retryable evidence".into(),
        )
        .into())
    }
}

async fn create_reservation_or_obsolete(
    transaction: &PostgresTransaction,
    delivery: &OutboundNotificationDelivery,
    generation: u64,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    authority_current: bool,
) -> Result<OutboundNotificationSmtpAttemptAdmission, PostgresPersistenceError> {
    let outcome = (!authority_current).then_some(OutboundNotificationSmtpAttemptOutcome::Obsolete);
    let state = if outcome.is_some() {
        OutboundNotificationSmtpAttemptState::Terminal
    } else {
        OutboundNotificationSmtpAttemptState::Reserved
    };
    let completed_at = outcome.map(|_| reserved_at);
    let attempt = OutboundNotificationSmtpAttemptRecord::restore(
        delivery.organization_id(),
        delivery.id(),
        delivery.recipient_contact_id().ok_or_else(|| {
            PostgresPersistenceError::Invariant("SMTP delivery has no recipient contact".into())
        })?,
        generation,
        outbound_notification_smtp_attempt_id(delivery.id(), generation)
            .map_err(PostgresPersistenceError::Invariant)?,
        state,
        outcome,
        1,
        fence_token,
        reserved_at,
        lease_expires_at,
        None,
        None,
        completed_at,
    )
    .map_err(PostgresPersistenceError::Invariant)?;
    insert_attempt(transaction, &attempt).await?;
    if authority_current {
        return Ok(OutboundNotificationSmtpAttemptAdmission::Reserved(attempt));
    }
    let receipt = terminal_receipt(delivery, &attempt)?.ok_or_else(|| {
        PostgresPersistenceError::Invariant("obsolete SMTP attempt produced no receipt".into())
    })?;
    persist_receipt(transaction, delivery, &receipt).await?;
    Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(receipt))
}

#[path = "outbound_smtp_postgres_repository.rs"]
mod repository;
