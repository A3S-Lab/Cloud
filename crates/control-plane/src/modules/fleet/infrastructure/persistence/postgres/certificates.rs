use super::enrollment::insert_certificate;
use super::queries;
use crate::infrastructure::{
    execute, fetch_optional, is_unique_violation, lock_idempotency_key, require_one_row,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::fleet::domain::entities::{Node, NodeCertificate};
use crate::modules::fleet::domain::repositories::{
    NodeCertificateRotationDraft, NodeCertificateRotationReservation,
};
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotencyRequest, NodeCertificateId, NodeId, OrganizationId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{sql_query, PostgresExecutor, PostgresTransaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

type RotationRow = (
    String,
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub(super) async fn reserve_rotation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    current_certificate_id: NodeCertificateId,
    mut draft: NodeCertificateRotationDraft,
    idempotency: IdempotencyRequest,
) -> Result<NodeCertificateRotationReservation, RepositoryError> {
    draft.requested_at = canonical_timestamp(draft.requested_at);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_idempotency_key(transaction, &idempotency).await?;
                if let Some(row) = rotation_by_key(transaction, &idempotency, true).await? {
                    if row.0 != idempotency.request_digest {
                        return Err(RepositoryError::IdempotencyConflict.into());
                    }
                    return reservation(transaction, row, true).await;
                }
                let node = queries::node_by_identity(transaction, organization_id, node_id, true)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if node.state == NodeState::Revoked {
                    return Err(RepositoryError::Conflict(
                        "revoked node cannot rotate its certificate".into(),
                    )
                    .into());
                }
                let active = queries::active_certificate_by_node(transaction, node_id, true)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if active.id != current_certificate_id {
                    return Err(RepositoryError::Conflict(
                        "certificate rotation identity is invalid".into(),
                    )
                    .into());
                }
                let inserted = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into node_certificate_rotations (scope_key, idempotency_key, request_digest, organization_id, node_id, current_certificate_id, replacement_certificate_id, requested_at, completed_at) values (",
                    )
                    .bind(idempotency.scope.as_str())
                    .append(", ")
                    .bind(idempotency.key.as_str())
                    .append(", ")
                    .bind(idempotency.request_digest.as_str())
                    .append(", ")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(node_id.as_uuid())
                    .append(", ")
                    .bind(current_certificate_id.as_uuid())
                    .append(", ")
                    .bind(draft.replacement_certificate_id.as_uuid())
                    .append(", ")
                    .bind(draft.requested_at)
                    .append(", null)"),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("certificate rotation reservation", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "another certificate rotation is already pending".into(),
                        )
                        .into())
                    }
                    Err(error) => return Err(error),
                }
                Ok(NodeCertificateRotationReservation {
                    node,
                    current_certificate: active,
                    replacement_certificate_id: draft.replacement_certificate_id,
                    requested_at: draft.requested_at,
                    replacement: None,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn complete_rotation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    current_certificate_id: NodeCertificateId,
    replacement: NodeCertificate,
    rotated_at: DateTime<Utc>,
    event: DomainEventEnvelope,
    idempotency: IdempotencyRequest,
) -> Result<NodeCertificateRotationReservation, RepositoryError> {
    let rotated_at = canonical_timestamp(rotated_at);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                lock_idempotency_key(transaction, &idempotency).await?;
                let row = rotation_by_key(transaction, &idempotency, true)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if row.0 != idempotency.request_digest
                    || row.1 != organization_id.as_uuid()
                    || row.2 != node_id.as_uuid()
                    || row.3 != current_certificate_id.as_uuid()
                    || row.4 != replacement.id.as_uuid()
                    || replacement.node_id != node_id
                {
                    return Err(RepositoryError::IdempotencyConflict.into());
                }
                if row.6.is_some() {
                    return reservation(transaction, row, true).await;
                }
                let current = queries::certificate_by_id(transaction, current_certificate_id, true)
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "rotation current certificate is missing".into(),
                        )
                    })?;
                if current.revoked_at.is_some() {
                    return Err(RepositoryError::Conflict(
                        "rotation current certificate is no longer active".into(),
                    )
                    .into());
                }
                require_one_row(
                    "rotated certificate",
                    execute(
                        transaction,
                        sql_query::<()>("update node_certificates set revoked_at = ")
                            .bind(rotated_at)
                            .append(" where id = ")
                            .bind(current_certificate_id.as_uuid())
                            .append(" and revoked_at is null"),
                    )
                    .await?,
                )?;
                let inserted = insert_certificate(transaction, &replacement).await;
                match inserted {
                    Ok(()) => {}
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "replacement certificate identity already exists".into(),
                        )
                        .into())
                    }
                    Err(error) => return Err(error),
                }
                let mut node =
                    queries::node_by_identity(transaction, organization_id, node_id, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                node.aggregate_version += 1;
                require_one_row(
                    "rotated node version",
                    execute(
                        transaction,
                        sql_query::<()>("update nodes set aggregate_version = ")
                            .bind(node.aggregate_version)
                            .append(" where organization_id = ")
                            .bind(organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(node_id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(node.aggregate_version - 1),
                    )
                    .await?,
                )?;
                require_one_row(
                    "completed certificate rotation",
                    execute(
                        transaction,
                        sql_query::<()>("update node_certificate_rotations set completed_at = ")
                            .bind(rotated_at)
                            .append(" where scope_key = ")
                            .bind(idempotency.scope.as_str())
                            .append(" and idempotency_key = ")
                            .bind(idempotency.key.as_str())
                            .append(" and completed_at is null"),
                    )
                    .await?,
                )?;
                if event.aggregate_version != node.aggregate_version {
                    return Err(PostgresPersistenceError::Invariant(
                        "certificate rotation event version does not match the node".into(),
                    ));
                }
                store_outbox(transaction, &event).await?;
                Ok(NodeCertificateRotationReservation {
                    node,
                    current_certificate: NodeCertificate {
                        revoked_at: Some(rotated_at),
                        ..current
                    },
                    replacement_certificate_id: replacement.id,
                    requested_at: row.5,
                    replacement: Some(replacement),
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn authenticate(
    executor: &PostgresExecutor,
    fingerprint: &str,
    now: DateTime<Utc>,
) -> Result<Node, RepositoryError> {
    let fingerprint = fingerprint.to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let certificate_id = fetch_optional::<Uuid, _>(
                    transaction,
                    sql_query::<Uuid>("select id from node_certificates where fingerprint = ")
                        .bind(fingerprint)
                        .append(" and revoked_at is null and issued_at <= ")
                        .bind(now)
                        .append(" and expires_at > ")
                        .bind(now),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let certificate = queries::certificate_by_id(
                    transaction,
                    NodeCertificateId::from_uuid(certificate_id),
                    false,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "certificate fingerprint index is orphaned".into(),
                    )
                })?;
                let node = queries::node_by_id(transaction, certificate.node_id, false)
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant("certificate node is missing".into())
                    })?;
                if node.state == NodeState::Revoked {
                    return Err(RepositoryError::NotFound.into());
                }
                Ok(node)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn authenticate_rotation(
    executor: &PostgresExecutor,
    fingerprint: &str,
    now: DateTime<Utc>,
    replay_not_before: DateTime<Utc>,
) -> Result<Node, RepositoryError> {
    if replay_not_before > now {
        return Err(RepositoryError::Conflict(
            "certificate rotation replay window is invalid".into(),
        ));
    }
    let fingerprint = fingerprint.to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let certificate_id = fetch_optional::<Uuid, _>(
                    transaction,
                    sql_query::<Uuid>(
                        "select certificate.id from node_certificates certificate where certificate.fingerprint = ",
                    )
                    .bind(fingerprint)
                    .append(" and certificate.issued_at <= ")
                    .bind(now)
                    .append(" and certificate.expires_at > ")
                    .bind(now)
                    .append(" and (certificate.revoked_at is null or exists (select 1 from node_certificate_rotations rotation join node_certificates replacement on replacement.id = rotation.replacement_certificate_id and replacement.revoked_at is null and replacement.issued_at <= ")
                    .bind(now)
                    .append(" and replacement.expires_at > ")
                    .bind(now)
                    .append(" where rotation.current_certificate_id = certificate.id and rotation.node_id = certificate.node_id and rotation.completed_at >= ")
                    .bind(replay_not_before)
                    .append(" and rotation.completed_at <= ")
                    .bind(now)
                    .append("))"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let certificate = queries::certificate_by_id(
                    transaction,
                    NodeCertificateId::from_uuid(certificate_id),
                    false,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "certificate fingerprint index is orphaned".into(),
                    )
                })?;
                let node = queries::node_by_id(transaction, certificate.node_id, false)
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant("certificate node is missing".into())
                    })?;
                if node.state == NodeState::Revoked {
                    return Err(RepositoryError::NotFound.into());
                }
                Ok(node)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find_active(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> Result<NodeCertificate, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                queries::node_by_identity(transaction, organization_id, node_id, false)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                queries::active_certificate_by_node(transaction, node_id, false)
                    .await?
                    .ok_or_else(|| RepositoryError::NotFound.into())
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    certificate_id: NodeCertificateId,
) -> Result<NodeCertificate, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                queries::node_by_identity(transaction, organization_id, node_id, false)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                let certificate = queries::certificate_by_id(transaction, certificate_id, false)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if certificate.node_id != node_id {
                    return Err(RepositoryError::NotFound.into());
                }
                Ok(certificate)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn rotation_by_key(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
    lock: bool,
) -> Result<Option<RotationRow>, PostgresPersistenceError> {
    let mut query = sql_query::<RotationRow>(
        "select request_digest, organization_id, node_id, current_certificate_id, replacement_certificate_id, requested_at, completed_at from node_certificate_rotations where scope_key = ",
    )
    .bind(idempotency.scope.as_str())
    .append(" and idempotency_key = ")
    .bind(idempotency.key.as_str());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query).await
}

async fn reservation(
    transaction: &PostgresTransaction,
    row: RotationRow,
    replayed: bool,
) -> Result<NodeCertificateRotationReservation, PostgresPersistenceError> {
    let organization_id = OrganizationId::from_uuid(row.1);
    let node_id = NodeId::from_uuid(row.2);
    let current_certificate_id = NodeCertificateId::from_uuid(row.3);
    let replacement_certificate_id = NodeCertificateId::from_uuid(row.4);
    let node = queries::node_by_identity(transaction, organization_id, node_id, false)
        .await?
        .ok_or_else(|| PostgresPersistenceError::Invariant("rotation node is missing".into()))?;
    let current_certificate =
        queries::certificate_by_id(transaction, current_certificate_id, false)
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "rotation current certificate is missing".into(),
                )
            })?;
    let replacement = if row.6.is_some() {
        Some(
            queries::certificate_by_id(transaction, replacement_certificate_id, false)
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "completed rotation replacement is missing".into(),
                    )
                })?,
        )
    } else {
        None
    };
    Ok(NodeCertificateRotationReservation {
        node,
        current_certificate,
        replacement_certificate_id,
        requested_at: row.5,
        replacement,
        replayed,
    })
}
