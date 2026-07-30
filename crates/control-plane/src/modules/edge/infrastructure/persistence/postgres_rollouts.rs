use super::postgres::{PublicationRow, PublicationSelection};
use super::postgres_rollout_routes;
use super::postgres_schema::{
    GatewayCertificates, GatewayPublications, GatewayRolloutReplicas, GatewayRolloutRollbacks,
    GatewayRollouts, GatewayScopes,
};
use super::postgres_tls::{update_certificate, CertificateRow, CertificateSelection};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, is_unique_violation, require_one_row, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, GatewayPublication, GatewayReplicaRolloutState,
    GatewayRollout, GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState,
    GatewayScope, GatewayScopeState,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, NodeId, OrganizationId, RepositoryError,
};
use a3s_orm::{insert_into, select_from, update_table, PostgresExecutor};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

mod dispatches;
mod models;
mod queries;
mod recovery;
mod staging;

use models::{RollbackRow, RollbackSelection};
use queries::{lock_by_id, lock_rollback};

pub(super) use dispatches::pending as pending_dispatches;
pub(super) use queries::{find, find_rollback, next_generation, pending_rollbacks, replay};
pub(super) use recovery::{
    pending as pending_recoveries, record_failure as record_recovery_failure,
    record_observation as record_recovery_observation,
    stage_observation as stage_recovery_observation,
};
pub(super) use staging::{stage, stage_managed, stage_managed_rollback, stage_rollback};

pub(super) async fn mark_unavailable(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    let failure = failure.to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let (revision, command_id) = fetch_optional::<(u64, Uuid), _>(
                    transaction,
                    select_from::<GatewayRolloutReplicas>()
                        .select((
                            GatewayRolloutReplicas::revision(),
                            GatewayRolloutReplicas::command_id(),
                        ))
                        .filter(
                            GatewayRolloutReplicas::gateway_rollout_id().eq(rollout_id.as_uuid()),
                        )
                        .filter(GatewayRolloutReplicas::node_id().eq(node_id.as_uuid())),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let publication_row = fetch_optional::<PublicationRow, _>(
                    transaction,
                    select_from::<GatewayPublications>()
                        .select(PublicationSelection)
                        .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                        .filter(GatewayPublications::revision().eq(revision))
                        .filter(GatewayPublications::command_id().eq(command_id))
                        .for_update(),
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "Gateway rollout unavailable publication disappeared".into(),
                    )
                })?;
                let mut publication = publication_row.publication()?;
                let (stored_organization_id, mut rollout) = lock_by_id(transaction, rollout_id)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                if stored_organization_id != organization_id.as_uuid() {
                    return Err(RepositoryError::NotFound.into());
                }
                if rollout.aggregate_version != expected_version {
                    return Err(RepositoryError::Conflict(
                        "Gateway rollout changed before unavailability was recorded".into(),
                    )
                    .into());
                }
                let replica = rollout
                    .replicas
                    .iter()
                    .find(|replica| replica.node_id == node_id)
                    .cloned()
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "Gateway rollout omitted its unavailable replica".into(),
                        )
                    })?;
                if replica.revision != revision
                    || replica.command_id.as_uuid() != command_id
                    || publication.snapshot_digest != replica.snapshot_digest
                    || publication.snapshot_expires_at != replica.snapshot_expires_at
                {
                    return Err(PostgresPersistenceError::Invariant(
                        "Gateway rollout unavailable publication is inconsistent".into(),
                    ));
                }
                publication
                    .mark_unavailable(&failure, observed_at)
                    .map_err(RepositoryError::Conflict)?;
                rollout
                    .mark_unavailable(node_id, &failure, observed_at)
                    .map_err(RepositoryError::Conflict)?;

                let certificate = lock_certificate_binding(
                    transaction,
                    stored_organization_id,
                    &rollout,
                    &publication,
                    rollout.started_at,
                )
                .await?;
                let (certificate, certificate_version, certificate_changed) = match certificate {
                    Some((mut certificate, false)) => {
                        let version = certificate.aggregate_version;
                        let changed = certificate
                            .mark_delivery_unavailable(&failure, observed_at)
                            .map_err(RepositoryError::Conflict)?;
                        (Some(certificate), Some(version), changed)
                    }
                    Some((_, true)) | None => (None, None, false),
                };

                persist_unavailable_publication(transaction, &publication).await?;
                if let (Some(certificate), Some(certificate_version), true) =
                    (&certificate, certificate_version, certificate_changed)
                {
                    update_certificate(transaction, certificate, certificate_version).await?;
                }
                postgres_rollout_routes::project_unavailability(
                    transaction,
                    &rollout,
                    node_id,
                    &failure,
                    observed_at,
                )
                .await?;
                persist_transition(transaction, &rollout, node_id, expected_version).await?;
                Ok(rollout)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn persist_unavailable_publication(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    if publication.state != crate::modules::edge::domain::GatewayPublicationState::Unavailable {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway publication unavailable transition has a non-terminal state".into(),
        ));
    }
    require_one_row(
        "Gateway publication unavailable transition",
        execute(
            transaction,
            update_table::<GatewayPublications>()
                .set(GatewayPublications::state(), publication.state.as_str())
                .set(GatewayPublications::failure(), publication.failure.clone())
                .set(
                    GatewayPublications::acknowledged_at(),
                    publication.acknowledged_at,
                )
                .filter(GatewayPublications::node_id().eq(publication.node_id.as_uuid()))
                .filter(GatewayPublications::revision().eq(publication.revision))
                .filter(GatewayPublications::state().eq("pending")),
        )
        .await?,
    )
}

pub(super) async fn lock_by_gateway_identity(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: Uuid,
    revision: u64,
    command_id: Uuid,
) -> Result<Option<(Uuid, GatewayRollout)>, PostgresPersistenceError> {
    let rollout_id = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<GatewayRolloutReplicas>()
            .select(GatewayRolloutReplicas::gateway_rollout_id())
            .filter(GatewayRolloutReplicas::node_id().eq(node_id))
            .filter(GatewayRolloutReplicas::revision().eq(revision))
            .filter(GatewayRolloutReplicas::command_id().eq(command_id)),
    )
    .await?;
    let Some(rollout_id) = rollout_id else {
        return Ok(None);
    };
    lock_by_id(transaction, GatewayRolloutId::from_uuid(rollout_id)).await
}

pub(super) async fn lock_certificate_binding(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: Uuid,
    rollout: &GatewayRollout,
    publication: &GatewayPublication,
    valid_at: DateTime<Utc>,
) -> Result<Option<(GatewayCertificate, bool)>, PostgresPersistenceError> {
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == publication.node_id)
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Gateway rollout publication omitted its replica certificate binding".into(),
            )
        })?;
    if replica.revision != publication.revision
        || replica.command_id != publication.command_id
        || replica.snapshot_digest != publication.snapshot_digest
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollout publication and replica certificate binding diverged".into(),
        ));
    }
    let current_certificates = fetch_all::<CertificateRow, _>(
        transaction,
        select_from::<GatewayCertificates>()
            .select(CertificateSelection)
            .filter(GatewayCertificates::node_id().eq(publication.node_id.as_uuid()))
            .filter(GatewayCertificates::gateway_revision().eq(publication.revision))
            .filter(GatewayCertificates::gateway_command_id().eq(publication.command_id.as_uuid()))
            .for_update(),
    )
    .await?
    .into_iter()
    .map(CertificateRow::certificate)
    .collect::<Result<Vec<_>, _>>()?;
    let current_certificate_ids = current_certificates
        .iter()
        .map(|certificate| certificate.id)
        .collect::<BTreeSet<_>>();
    let Some(expected_certificate_id) = replica.gateway_certificate_id else {
        if publication.certificate_request.is_some() || !current_certificate_ids.is_empty() {
            return Err(PostgresPersistenceError::Invariant(
                "certificate-free Gateway rollout has staged certificate material".into(),
            ));
        }
        return Ok(None);
    };
    let request = publication
        .certificate_request
        .as_ref()
        .filter(|request| request.certificate_id == expected_certificate_id.as_uuid())
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Gateway rollout certificate request changed after staging".into(),
            )
        })?;
    let certificate = fetch_optional::<CertificateRow, _>(
        transaction,
        select_from::<GatewayCertificates>()
            .select(CertificateSelection)
            .filter(GatewayCertificates::id().eq(expected_certificate_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("Gateway rollout certificate disappeared".into())
    })?
    .certificate()?;
    if certificate.organization_id.as_uuid() != organization_id
        || certificate.node_id != publication.node_id
        || certificate.request != *request
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollout certificate identity or request changed".into(),
        ));
    }
    let is_new = certificate.gateway_revision == publication.revision
        && certificate.gateway_command_id == publication.command_id
        && certificate.snapshot_digest == publication.snapshot_digest;
    if is_new {
        if current_certificate_ids != BTreeSet::from([expected_certificate_id]) {
            return Err(PostgresPersistenceError::Invariant(
                "Gateway rollout has inconsistent newly staged certificate material".into(),
            ));
        }
        return Ok(Some((certificate, false)));
    }
    let staged_rollback = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<GatewayRolloutRollbacks>()
            .select(GatewayRolloutRollbacks::failed_rollout_id())
            .filter(GatewayRolloutRollbacks::rollback_rollout_id().eq(rollout.id.as_uuid()))
            .filter(GatewayRolloutRollbacks::state().eq("staged"))
            .for_update(),
    )
    .await?;
    if !current_certificate_ids.is_empty()
        || staged_rollback.is_none()
        || certificate.state != GatewayCertificateState::Ready
        || certificate.material.as_ref().is_none_or(|material| {
            material.validate().is_err()
                || material.issued_at > valid_at
                || material.expires_at <= valid_at
        })
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollback reused certificate is no longer exact and valid".into(),
        ));
    }
    Ok(Some((certificate, true)))
}

pub(super) async fn persist_acknowledgement(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    node_id: NodeId,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    persist_transition(transaction, rollout, node_id, expected_version).await
}

async fn persist_rollback_stage(
    transaction: &a3s_orm::PostgresTransaction,
    rollback: &GatewayRolloutRollback,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    rollback.validate().map_err(RepositoryError::Conflict)?;
    require_one_row(
        "Gateway rollout rollback stage",
        execute(
            transaction,
            update_table::<GatewayRolloutRollbacks>()
                .set(GatewayRolloutRollbacks::state(), rollback.state.as_str())
                .set(
                    GatewayRolloutRollbacks::aggregate_version(),
                    rollback.aggregate_version,
                )
                .set(GatewayRolloutRollbacks::staged_at(), rollback.staged_at)
                .set(
                    GatewayRolloutRollbacks::completed_at(),
                    rollback.completed_at,
                )
                .set(GatewayRolloutRollbacks::failure(), rollback.failure.clone())
                .filter(
                    GatewayRolloutRollbacks::failed_rollout_id()
                        .eq(rollback.failed_rollout_id.as_uuid()),
                )
                .filter(GatewayRolloutRollbacks::state().eq("required"))
                .filter(GatewayRolloutRollbacks::aggregate_version().eq(expected_version)),
        )
        .await?,
    )?;
    Ok(())
}

async fn lock_physical_scope(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
) -> Result<GatewayScopeState, PostgresPersistenceError> {
    let scope = fetch_optional::<(u64, Option<u64>, u64), _>(
        transaction,
        select_from::<GatewayScopes>()
            .select((
                GatewayScopes::last_issued_revision(),
                GatewayScopes::installed_revision(),
                GatewayScopes::aggregate_version(),
            ))
            .filter(GatewayScopes::node_id().eq(node_id.as_uuid()))
            .for_update(),
    )
    .await?;
    match scope {
        Some((last_issued_revision, installed_revision, aggregate_version))
            if last_issued_revision > 0
                && aggregate_version > 0
                && installed_revision
                    .is_none_or(|installed| installed > 0 && installed <= last_issued_revision) =>
        {
            Ok(GatewayScopeState {
                node_id,
                last_issued_revision,
                installed_revision,
                aggregate_version,
            })
        }
        Some(_) => Err(PostgresPersistenceError::Invariant(
            "stored physical Gateway scope is invalid".into(),
        )),
        None => Ok(GatewayScopeState::empty(node_id)),
    }
}

async fn advance_physical_scope(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &crate::modules::edge::domain::GatewayPublication,
    current: &GatewayScopeState,
) -> Result<(), PostgresPersistenceError> {
    if current.aggregate_version == 0 {
        require_one_row(
            "physical Gateway scope",
            execute(
                transaction,
                insert_into::<GatewayScopes>()
                    .value(GatewayScopes::node_id(), publication.node_id.as_uuid())
                    .value(GatewayScopes::last_issued_revision(), publication.revision)
                    .value(
                        GatewayScopes::installed_revision(),
                        current.installed_revision,
                    )
                    .value(GatewayScopes::aggregate_version(), 1_u64)
                    .value(GatewayScopes::updated_at(), publication.command_issued_at),
            )
            .await?,
        )?;
    } else {
        let next_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "physical Gateway scope aggregate version overflowed".into(),
            )
        })?;
        require_one_row(
            "physical Gateway scope",
            execute(
                transaction,
                update_table::<GatewayScopes>()
                    .set(GatewayScopes::last_issued_revision(), publication.revision)
                    .set(GatewayScopes::aggregate_version(), next_version)
                    .set(GatewayScopes::updated_at(), publication.command_issued_at)
                    .filter(GatewayScopes::node_id().eq(publication.node_id.as_uuid()))
                    .filter(GatewayScopes::aggregate_version().eq(current.aggregate_version)),
            )
            .await?,
        )?;
    }
    Ok(())
}

async fn insert_rollout(
    transaction: &a3s_orm::PostgresTransaction,
    scope: &GatewayScope,
    rollout: &GatewayRollout,
) -> Result<(), PostgresPersistenceError> {
    let desired_replicas = u32::try_from(rollout.replicas.len()).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "Gateway rollout desired replica count exceeds supported bounds".into(),
        )
    })?;
    let inserted = execute(
        transaction,
        insert_into::<GatewayRollouts>()
            .value(GatewayRollouts::id(), rollout.id.as_uuid())
            .value(
                GatewayRollouts::organization_id(),
                scope.organization_id.as_uuid(),
            )
            .value(GatewayRollouts::project_id(), scope.project_id.as_uuid())
            .value(
                GatewayRollouts::environment_id(),
                scope.environment_id.as_uuid(),
            )
            .value(
                GatewayRollouts::gateway_scope_id(),
                rollout.gateway_scope_id.as_uuid(),
            )
            .value(
                GatewayRollouts::membership_generation(),
                rollout.membership_generation,
            )
            .value(GatewayRollouts::generation(), rollout.generation)
            .value(GatewayRollouts::correlation_id(), rollout.correlation_id)
            .value(GatewayRollouts::min_ready(), rollout.policy.min_ready)
            .value(
                GatewayRollouts::max_unavailable(),
                rollout.policy.max_unavailable,
            )
            .value(GatewayRollouts::desired_replicas(), desired_replicas)
            .value(GatewayRollouts::state(), rollout.state.as_str())
            .value(GatewayRollouts::ready_replicas(), rollout.ready_replicas)
            .value(
                GatewayRollouts::unavailable_replicas(),
                rollout.unavailable_replicas,
            )
            .value(
                GatewayRollouts::aggregate_version(),
                rollout.aggregate_version,
            )
            .value(GatewayRollouts::started_at(), rollout.started_at)
            .value(GatewayRollouts::completed_at(), rollout.completed_at),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Gateway rollout", rows)?,
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "Gateway rollout identity, generation, or active slot already exists".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }
    for replica in &rollout.replicas {
        require_one_row(
            "Gateway rollout replica",
            execute(
                transaction,
                insert_into::<GatewayRolloutReplicas>()
                    .value(
                        GatewayRolloutReplicas::gateway_rollout_id(),
                        rollout.id.as_uuid(),
                    )
                    .value(
                        GatewayRolloutReplicas::gateway_scope_id(),
                        rollout.gateway_scope_id.as_uuid(),
                    )
                    .value(
                        GatewayRolloutReplicas::membership_generation(),
                        rollout.membership_generation,
                    )
                    .value(GatewayRolloutReplicas::node_id(), replica.node_id.as_uuid())
                    .value(GatewayRolloutReplicas::revision(), replica.revision)
                    .value(
                        GatewayRolloutReplicas::command_id(),
                        replica.command_id.as_uuid(),
                    )
                    .value(
                        GatewayRolloutReplicas::snapshot_digest(),
                        replica.snapshot_digest.as_str(),
                    )
                    .value(
                        GatewayRolloutReplicas::snapshot_expires_at(),
                        replica.snapshot_expires_at,
                    )
                    .value(
                        GatewayRolloutReplicas::gateway_certificate_id(),
                        replica.gateway_certificate_id.map(|id| id.as_uuid()),
                    )
                    .value(GatewayRolloutReplicas::state(), replica.state.as_str())
                    .value(GatewayRolloutReplicas::failure(), replica.failure.clone())
                    .value(
                        GatewayRolloutReplicas::acknowledged_at(),
                        replica.acknowledged_at,
                    )
                    .value(
                        GatewayRolloutReplicas::recovery(),
                        replica
                            .recovery
                            .as_ref()
                            .map(serde_json::to_value)
                            .transpose()?,
                    ),
            )
            .await?,
        )?;
    }
    Ok(())
}

async fn persist_transition(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    node_id: NodeId,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    rollout.validate().map_err(RepositoryError::Conflict)?;
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Gateway rollout transition omitted its replica".into(),
            )
        })?;
    require_one_row(
        "Gateway rollout replica transition",
        execute(
            transaction,
            update_table::<GatewayRolloutReplicas>()
                .set(GatewayRolloutReplicas::state(), replica.state.as_str())
                .set(GatewayRolloutReplicas::failure(), replica.failure.clone())
                .set(
                    GatewayRolloutReplicas::acknowledged_at(),
                    replica.acknowledged_at,
                )
                .set(
                    GatewayRolloutReplicas::recovery(),
                    replica
                        .recovery
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()?,
                )
                .filter(GatewayRolloutReplicas::gateway_rollout_id().eq(rollout.id.as_uuid()))
                .filter(GatewayRolloutReplicas::node_id().eq(node_id.as_uuid()))
                .filter(GatewayRolloutReplicas::state().eq("pending")),
        )
        .await?,
    )?;
    require_one_row(
        "Gateway rollout transition",
        execute(
            transaction,
            update_table::<GatewayRollouts>()
                .set(GatewayRollouts::state(), rollout.state.as_str())
                .set(GatewayRollouts::ready_replicas(), rollout.ready_replicas)
                .set(
                    GatewayRollouts::unavailable_replicas(),
                    rollout.unavailable_replicas,
                )
                .set(
                    GatewayRollouts::aggregate_version(),
                    rollout.aggregate_version,
                )
                .set(GatewayRollouts::completed_at(), rollout.completed_at)
                .filter(GatewayRollouts::id().eq(rollout.id.as_uuid()))
                .filter(GatewayRollouts::aggregate_version().eq(expected_version)),
        )
        .await?,
    )?;
    project_terminal_rollback(transaction, rollout).await?;
    Ok(())
}

async fn project_terminal_rollback(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
) -> Result<(), PostgresPersistenceError> {
    if !rollout.state.terminal() {
        return Ok(());
    }
    let child_rollback = fetch_optional::<RollbackRow, _>(
        transaction,
        select_from::<GatewayRolloutRollbacks>()
            .select(RollbackSelection)
            .filter(GatewayRolloutRollbacks::rollback_rollout_id().eq(rollout.id.as_uuid()))
            .for_update(),
    )
    .await?
    .map(RollbackRow::rollback)
    .transpose()?;
    if let Some(mut rollback) = child_rollback {
        let expected_version = rollback.aggregate_version;
        match rollout.state {
            GatewayRolloutState::Succeeded => {
                rollback
                    .succeed(rollout)
                    .map_err(RepositoryError::Conflict)?;
            }
            GatewayRolloutState::Degraded => {
                rollback
                    .diverge(
                        rollout,
                        "Gateway exact rollback did not receive every member acknowledgement",
                    )
                    .map_err(RepositoryError::Conflict)?;
            }
            GatewayRolloutState::Pending | GatewayRolloutState::Ready => unreachable!(),
        }
        persist_rollback_completion(transaction, &rollback, expected_version).await?;
        if rollback.state == GatewayRolloutRollbackState::Succeeded {
            let (_, failed) = lock_by_id(transaction, rollback.failed_rollout_id)
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "failed Gateway rollout disappeared before ownership release".into(),
                    )
                })?;
            postgres_rollout_routes::release_failed_ownership(transaction, &failed).await?;
        }
        return Ok(());
    }
    if rollout.state == GatewayRolloutState::Degraded
        && !rollout
            .serves_traffic()
            .map_err(PostgresPersistenceError::Invariant)?
    {
        let required =
            GatewayRolloutRollback::required(rollout).map_err(RepositoryError::Conflict)?;
        insert_required_rollback(transaction, &required).await?;
    }
    Ok(())
}

async fn insert_required_rollback(
    transaction: &a3s_orm::PostgresTransaction,
    rollback: &GatewayRolloutRollback,
) -> Result<(), PostgresPersistenceError> {
    rollback.validate().map_err(RepositoryError::Conflict)?;
    let inserted = execute(
        transaction,
        insert_into::<GatewayRolloutRollbacks>()
            .value(
                GatewayRolloutRollbacks::failed_rollout_id(),
                rollback.failed_rollout_id.as_uuid(),
            )
            .value(
                GatewayRolloutRollbacks::gateway_scope_id(),
                rollback.gateway_scope_id.as_uuid(),
            )
            .value(
                GatewayRolloutRollbacks::membership_generation(),
                rollback.membership_generation,
            )
            .value(
                GatewayRolloutRollbacks::failed_generation(),
                rollback.failed_generation,
            )
            .value(
                GatewayRolloutRollbacks::rollback_rollout_id(),
                rollback.rollback_rollout_id.as_uuid(),
            )
            .value(
                GatewayRolloutRollbacks::rollback_generation(),
                rollback.rollback_generation,
            )
            .value(GatewayRolloutRollbacks::state(), rollback.state.as_str())
            .value(
                GatewayRolloutRollbacks::aggregate_version(),
                rollback.aggregate_version,
            )
            .value(GatewayRolloutRollbacks::required_at(), rollback.required_at)
            .value(GatewayRolloutRollbacks::staged_at(), rollback.staged_at)
            .value(
                GatewayRolloutRollbacks::completed_at(),
                rollback.completed_at,
            )
            .value(GatewayRolloutRollbacks::failure(), rollback.failure.clone()),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Gateway rollout rollback requirement", rows),
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Invariant(
            "Gateway rollout terminal transition produced conflicting rollback intent".into(),
        )),
        Err(error) => Err(error),
    }
}

async fn persist_rollback_completion(
    transaction: &a3s_orm::PostgresTransaction,
    rollback: &GatewayRolloutRollback,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    rollback.validate().map_err(RepositoryError::Conflict)?;
    require_one_row(
        "Gateway rollout rollback completion",
        execute(
            transaction,
            update_table::<GatewayRolloutRollbacks>()
                .set(GatewayRolloutRollbacks::state(), rollback.state.as_str())
                .set(
                    GatewayRolloutRollbacks::aggregate_version(),
                    rollback.aggregate_version,
                )
                .set(
                    GatewayRolloutRollbacks::completed_at(),
                    rollback.completed_at,
                )
                .set(GatewayRolloutRollbacks::failure(), rollback.failure.clone())
                .filter(
                    GatewayRolloutRollbacks::failed_rollout_id()
                        .eq(rollback.failed_rollout_id.as_uuid()),
                )
                .filter(GatewayRolloutRollbacks::state().eq("staged"))
                .filter(GatewayRolloutRollbacks::aggregate_version().eq(expected_version)),
        )
        .await?,
    )
}

pub(super) async fn persist_recovered_physical_scope(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
    installed_revision: Option<u64>,
    observed_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let current = lock_physical_scope(transaction, node_id).await?;
    if current.aggregate_version == 0 {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway recovery observation has no physical scope".into(),
        ));
    }
    if installed_revision
        .is_some_and(|revision| revision == 0 || revision > current.last_issued_revision)
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway recovery observation exceeds the issued physical revision".into(),
        ));
    }
    if current.installed_revision == installed_revision {
        return Ok(());
    }
    let next_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "physical Gateway scope version space is exhausted during recovery".into(),
        )
    })?;
    require_one_row(
        "physical Gateway recovery projection",
        execute(
            transaction,
            update_table::<GatewayScopes>()
                .set(GatewayScopes::installed_revision(), installed_revision)
                .set(GatewayScopes::aggregate_version(), next_version)
                .set(GatewayScopes::updated_at(), observed_at)
                .filter(GatewayScopes::node_id().eq(node_id.as_uuid()))
                .filter(GatewayScopes::aggregate_version().eq(current.aggregate_version)),
        )
        .await?,
    )
}

pub(super) async fn diverge_required_rollback(
    transaction: &a3s_orm::PostgresTransaction,
    failed_rollout_id: GatewayRolloutId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let mut rollback = lock_rollback(transaction, failed_rollout_id)
        .await?
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Gateway physical divergence omitted its rollback intent".into(),
            )
        })?;
    match rollback.state {
        GatewayRolloutRollbackState::Required => {
            let expected_version = rollback.aggregate_version;
            rollback
                .diverge_before_staging(failure, observed_at)
                .map_err(RepositoryError::Conflict)?;
            require_one_row(
                "Gateway rollback physical divergence",
                execute(
                    transaction,
                    update_table::<GatewayRolloutRollbacks>()
                        .set(GatewayRolloutRollbacks::state(), rollback.state.as_str())
                        .set(
                            GatewayRolloutRollbacks::aggregate_version(),
                            rollback.aggregate_version,
                        )
                        .set(
                            GatewayRolloutRollbacks::completed_at(),
                            rollback.completed_at,
                        )
                        .set(GatewayRolloutRollbacks::failure(), rollback.failure.clone())
                        .filter(
                            GatewayRolloutRollbacks::failed_rollout_id()
                                .eq(failed_rollout_id.as_uuid()),
                        )
                        .filter(GatewayRolloutRollbacks::state().eq("required"))
                        .filter(GatewayRolloutRollbacks::aggregate_version().eq(expected_version)),
                )
                .await?,
            )
        }
        GatewayRolloutRollbackState::Diverged => Ok(()),
        GatewayRolloutRollbackState::Staged | GatewayRolloutRollbackState::Succeeded => {
            Err(PostgresPersistenceError::Invariant(
                "Gateway physical divergence raced a staged or completed rollback".into(),
            ))
        }
    }
}

async fn persist_recovery_transition(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    node_id: NodeId,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    rollout.validate().map_err(RepositoryError::Conflict)?;
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "Gateway rollout recovery transition omitted its replica".into(),
            )
        })?;
    let recovery = replica.recovery.as_ref().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Gateway rollout recovery transition omitted its recovery state".into(),
        )
    })?;
    if replica.state != GatewayReplicaRolloutState::Unavailable
        || matches!(
            recovery.state,
            crate::modules::edge::domain::GatewayReplicaRecoveryState::Required
        ) && recovery.attempt == 0
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollout recovery transition is invalid".into(),
        ));
    }
    require_one_row(
        "Gateway rollout replica recovery transition",
        execute(
            transaction,
            update_table::<GatewayRolloutReplicas>()
                .set(
                    GatewayRolloutReplicas::recovery(),
                    serde_json::to_value(recovery)?,
                )
                .filter(GatewayRolloutReplicas::gateway_rollout_id().eq(rollout.id.as_uuid()))
                .filter(GatewayRolloutReplicas::node_id().eq(node_id.as_uuid()))
                .filter(GatewayRolloutReplicas::state().eq("unavailable")),
        )
        .await?,
    )?;
    require_one_row(
        "Gateway rollout recovery version transition",
        execute(
            transaction,
            update_table::<GatewayRollouts>()
                .set(
                    GatewayRollouts::aggregate_version(),
                    rollout.aggregate_version,
                )
                .filter(GatewayRollouts::id().eq(rollout.id.as_uuid()))
                .filter(GatewayRollouts::aggregate_version().eq(expected_version)),
        )
        .await?,
    )?;
    Ok(())
}
