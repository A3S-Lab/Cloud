use super::queries;
use super::rows::{self, EnrollmentTokenRow, SELECT_TOKENS};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::fleet::domain::entities::{EnrollmentToken, Node, NodeCertificate};
use crate::modules::fleet::domain::repositories::{NodeEnrollmentDraft, NodeEnrollmentReservation};
use crate::modules::fleet::domain::value_objects::EnrollmentTokenCredential;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnrollmentTokenId, IdempotencyRequest, IdempotentWrite, NodeId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{sql_query, PostgresExecutor, PostgresTransaction};
use uuid::Uuid;

pub(super) async fn issue_token(
    executor: &PostgresExecutor,
    token: EnrollmentToken,
    event: DomainEventEnvelope,
    idempotency: IdempotencyRequest,
) -> Result<IdempotentWrite<EnrollmentToken>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replayed) =
                    idempotency_replay::<EnrollmentToken>(transaction, &idempotency).await?
                {
                    return Ok(replayed);
                }
                let inserted = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into enrollment_tokens (id, organization_id, name, name_key, token_hash, aggregate_version, created_at, expires_at, used_at, revoked_at) values (",
                    )
                    .bind(token.id.as_uuid())
                    .append(", ")
                    .bind(token.organization_id.as_uuid())
                    .append(", ")
                    .bind(token.name.as_str())
                    .append(", ")
                    .bind(token.name_key.as_str())
                    .append(", ")
                    .bind(token.credential.digest())
                    .append(", ")
                    .bind(token.aggregate_version)
                    .append(", ")
                    .bind(token.created_at)
                    .append(", ")
                    .bind(token.expires_at)
                    .append(", ")
                    .bind(token.used_at)
                    .append(", ")
                    .bind(token.revoked_at)
                    .append(")"),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("enrollment token", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "enrollment token name or secret already exists".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                store_outbox(transaction, &event).await?;
                store_idempotency(transaction, &idempotency, &token).await?;
                Ok(IdempotentWrite {
                    value: token,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn reserve(
    executor: &PostgresExecutor,
    credential: &EnrollmentTokenCredential,
    mut draft: NodeEnrollmentDraft,
) -> Result<NodeEnrollmentReservation, RepositoryError> {
    draft.requested_at = canonical_timestamp(draft.requested_at);
    let digest = credential.digest().to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = fetch_optional::<EnrollmentTokenRow, _>(
                    transaction,
                    sql_query::<EnrollmentTokenRow>(SELECT_TOKENS)
                    .append(" where token_hash = ")
                    .bind(digest)
                    .append(" for update"),
                )
                .await?;
                let token = row
                    .map(rows::token)
                    .transpose()?
                    .ok_or(RepositoryError::NotFound)?;
                if let Some((request_digest, node_id)) = fetch_optional::<(String, Uuid), _>(
                    transaction,
                    sql_query::<(String, Uuid)>(
                        "select request_digest, node_id from node_enrollment_reservations where enrollment_token_id = ",
                    )
                    .bind(token.id.as_uuid())
                    .append(" for update"),
                )
                .await?
                {
                    if request_digest != draft.request_digest {
                        return Err(RepositoryError::IdempotencyConflict.into());
                    }
                    return load_reservation(
                        transaction,
                        token,
                        NodeId::from_uuid(node_id),
                        true,
                    )
                    .await;
                }
                if !token.is_usable_at(draft.requested_at) {
                    return Err(RepositoryError::Conflict(
                        "enrollment token is expired, revoked, or already used".into(),
                    )
                    .into());
                }
                let node = Node::enroll(
                    draft.proposed_node_id,
                    token.organization_id,
                    draft.name,
                    draft.agent_instance_id,
                    draft.agent_version,
                    draft.capabilities,
                    draft.requested_at,
                )
                .map_err(RepositoryError::Conflict)?;
                let inserted = insert_node(transaction, &node).await;
                match inserted {
                    Ok(()) => {}
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "node name or identity already exists".into(),
                        )
                        .into())
                    }
                    Err(error) => return Err(error),
                }
                require_one_row(
                    "used enrollment token",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "update enrollment_tokens set used_at = ",
                        )
                        .bind(draft.requested_at)
                        .append(", aggregate_version = aggregate_version + 1 where id = ")
                        .bind(token.id.as_uuid())
                        .append(" and used_at is null and aggregate_version = ")
                        .bind(token.aggregate_version),
                    )
                    .await?,
                )?;
                require_one_row(
                    "node enrollment reservation",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "insert into node_enrollment_reservations (enrollment_token_id, node_id, request_digest, reserved_at) values (",
                        )
                        .bind(token.id.as_uuid())
                        .append(", ")
                        .bind(node.id.as_uuid())
                        .append(", ")
                        .bind(draft.request_digest)
                        .append(", ")
                        .bind(draft.requested_at)
                        .append(")"),
                    )
                    .await?,
                )?;
                let mut used_token = token;
                used_token.used_at = Some(draft.requested_at);
                used_token.aggregate_version += 1;
                Ok(NodeEnrollmentReservation {
                    enrollment_token: used_token,
                    node,
                    certificate: None,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn complete(
    executor: &PostgresExecutor,
    token_id: EnrollmentTokenId,
    node_id: NodeId,
    request_digest: &str,
    certificate: NodeCertificate,
    event: DomainEventEnvelope,
) -> Result<NodeEnrollmentReservation, RepositoryError> {
    let request_digest = request_digest.to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let reservation = fetch_optional::<(String, Uuid), _>(
                    transaction,
                    sql_query::<(String, Uuid)>(
                        "select request_digest, node_id from node_enrollment_reservations where enrollment_token_id = ",
                    )
                    .bind(token_id.as_uuid())
                    .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if reservation.0 != request_digest
                    || reservation.1 != node_id.as_uuid()
                    || certificate.node_id != node_id
                {
                    return Err(RepositoryError::IdempotencyConflict.into());
                }
                let token = queries::token_by_id(transaction, token_id, false)
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "enrollment reservation token is missing".into(),
                        )
                    })?;
                let node = queries::node_by_id(transaction, node_id, false)
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "enrollment reservation node is missing".into(),
                        )
                    })?;
                if let Some(existing) =
                    queries::active_certificate_by_node(transaction, node_id, true).await?
                {
                    return Ok(NodeEnrollmentReservation {
                        enrollment_token: token,
                        node,
                        certificate: Some(existing),
                        replayed: true,
                    });
                }
                insert_certificate(transaction, &certificate).await?;
                store_outbox(transaction, &event).await?;
                Ok(NodeEnrollmentReservation {
                    enrollment_token: token,
                    node,
                    certificate: Some(certificate),
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn load_reservation(
    transaction: &PostgresTransaction,
    token: EnrollmentToken,
    node_id: NodeId,
    replayed: bool,
) -> Result<NodeEnrollmentReservation, PostgresPersistenceError> {
    let node = queries::node_by_id(transaction, node_id, false)
        .await?
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant("enrollment reservation node is missing".into())
        })?;
    let certificate = queries::active_certificate_by_node(transaction, node_id, false).await?;
    Ok(NodeEnrollmentReservation {
        enrollment_token: token,
        node,
        certificate,
        replayed,
    })
}

async fn insert_node(
    transaction: &PostgresTransaction,
    node: &Node,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, last_sequence, aggregate_version) values (",
        )
        .bind(node.organization_id.as_uuid())
        .append(", ")
        .bind(node.id.as_uuid())
        .append(", ")
        .bind(node.name.value())
        .append(", ")
        .bind(node.name.uniqueness_key())
        .append(", ")
        .bind(node.state.as_str())
        .append(", ")
        .bind(node.agent_instance_id)
        .append(", ")
        .bind(node.agent_version.as_str())
        .append(", ")
        .bind(node.capabilities.provider_id())
        .append(", ")
        .bind(node.capabilities.provider_build())
        .append(", ")
        .bind(node.capabilities.digest())
        .append(", ")
        .bind(node.capabilities.document().clone())
        .append(", ")
        .bind(node.enrolled_at)
        .append(", ")
        .bind(node.last_observed_at)
        .append(", ")
        .bind(node.last_sequence)
        .append(", ")
        .bind(node.aggregate_version)
        .append(")"),
    )
    .await?;
    require_one_row("node", rows)
}

pub(super) async fn insert_certificate(
    transaction: &PostgresTransaction,
    certificate: &NodeCertificate,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into node_certificates (id, node_id, serial_number, fingerprint, certificate_pem, ca_bundle_pem, issued_at, expires_at, revoked_at) values (",
        )
        .bind(certificate.id.as_uuid())
        .append(", ")
        .bind(certificate.node_id.as_uuid())
        .append(", ")
        .bind(certificate.serial_number.as_str())
        .append(", ")
        .bind(certificate.fingerprint.as_str())
        .append(", ")
        .bind(certificate.certificate_pem.as_str())
        .append(", ")
        .bind(certificate.ca_bundle_pem.as_str())
        .append(", ")
        .bind(certificate.issued_at)
        .append(", ")
        .bind(certificate.expires_at)
        .append(", ")
        .bind(certificate.revoked_at)
        .append(")"),
    )
    .await?;
    require_one_row("node certificate", rows)
}
