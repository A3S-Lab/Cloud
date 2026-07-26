use super::{super::State, validate_batch_limit};
use crate::modules::edge::domain::repositories::{
    GatewayRolloutDispatchTarget, GatewayRolloutResult, GatewayRolloutRollbackTarget,
};
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, GatewayPublication, GatewayReplicaRecoveryState,
    GatewayReplicaRolloutState, GatewayRollout, GatewayRolloutRollback,
    GatewayRolloutRollbackState, GatewayRolloutState,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, GatewayScopeId, IdempotencyRequest, OrganizationId, RepositoryError,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

pub(in super::super) fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<GatewayRolloutResult>, RepositoryError> {
    let Some((digest, existing)) = state
        .rollout_idempotency
        .get(&(idempotency.scope.clone(), idempotency.key.clone()))
    else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    let mut replay = existing.clone();
    replay.replayed = true;
    Ok(Some(replay))
}

pub(in super::super) fn next_generation(
    state: &State,
    organization_id: OrganizationId,
    gateway_scope_id: GatewayScopeId,
) -> Result<u64, RepositoryError> {
    let scope = state
        .gateway_scopes
        .get(&gateway_scope_id)
        .filter(|scope| scope.organization_id == organization_id)
        .ok_or(RepositoryError::NotFound)?;
    let current = state
        .rollouts
        .values()
        .filter(|rollout| rollout.gateway_scope_id == scope.id)
        .map(|rollout| rollout.generation)
        .max()
        .unwrap_or(0);
    current.checked_add(1).ok_or_else(|| {
        RepositoryError::Conflict("Gateway rollout generation space is exhausted".into())
    })
}

pub(in super::super) fn find(
    state: &State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
) -> Result<GatewayRollout, RepositoryError> {
    let rollout = state
        .rollouts
        .get(&rollout_id)
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    let scope = state
        .gateway_scopes
        .get(&rollout.gateway_scope_id)
        .ok_or_else(|| RepositoryError::Storage("Gateway rollout scope disappeared".into()))?;
    if scope.organization_id != organization_id {
        return Err(RepositoryError::NotFound);
    }
    Ok(rollout)
}

pub(in super::super) fn find_rollback(
    state: &State,
    organization_id: OrganizationId,
    failed_rollout_id: GatewayRolloutId,
) -> Result<GatewayRolloutRollback, RepositoryError> {
    let rollback = state
        .rollout_rollbacks
        .get(&failed_rollout_id)
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    let scope = state
        .gateway_scopes
        .get(&rollback.gateway_scope_id)
        .ok_or_else(|| RepositoryError::Storage("Gateway rollback scope disappeared".into()))?;
    if scope.organization_id != organization_id {
        return Err(RepositoryError::NotFound);
    }
    rollback.validate().map_err(RepositoryError::Storage)?;
    Ok(rollback)
}

pub(in super::super) fn pending_rollbacks(
    state: &State,
    limit: usize,
) -> Result<Vec<GatewayRolloutRollbackTarget>, RepositoryError> {
    validate_batch_limit(limit)?;
    let mut rollbacks = state
        .rollout_rollbacks
        .values()
        .filter(|rollback| rollback.state == GatewayRolloutRollbackState::Required)
        .collect::<Vec<_>>();
    rollbacks.sort_by_key(|rollback| (rollback.required_at, rollback.failed_rollout_id));
    let mut targets = Vec::new();
    for rollback in rollbacks {
        let failed_rollout = state
            .rollouts
            .get(&rollback.failed_rollout_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("required Gateway rollback lost its failed rollout".into())
            })?;
        if failed_rollout.replicas.iter().any(|replica| {
            replica.state == GatewayReplicaRolloutState::Unavailable
                && replica
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.state == GatewayReplicaRecoveryState::Diverged)
        }) {
            return Err(RepositoryError::Storage(
                "diverged Gateway recovery retained a required rollback".into(),
            ));
        }
        if failed_rollout.replicas.iter().any(|replica| {
            replica.state == GatewayReplicaRolloutState::Unavailable
                && replica
                    .recovery
                    .as_ref()
                    .is_none_or(|recovery| recovery.state != GatewayReplicaRecoveryState::Observed)
        }) {
            continue;
        }
        let scope = state
            .gateway_scopes
            .get(&rollback.gateway_scope_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("required Gateway rollback lost its scope".into())
            })?;
        let target = GatewayRolloutRollbackTarget {
            scope,
            failed_rollout,
            rollback: rollback.clone(),
        };
        target.validate().map_err(RepositoryError::Storage)?;
        targets.push(target);
        if targets.len() == limit {
            break;
        }
    }
    Ok(targets)
}

pub(in super::super) fn certificate_binding(
    state: &State,
    rollout: &GatewayRollout,
    publication: &GatewayPublication,
    valid_at: DateTime<Utc>,
) -> Result<Option<(GatewayCertificate, bool)>, RepositoryError> {
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == publication.node_id)
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway rollout publication omitted its replica certificate binding".into(),
            )
        })?;
    if replica.revision != publication.revision
        || replica.command_id != publication.command_id
        || replica.snapshot_digest != publication.snapshot_digest
    {
        return Err(RepositoryError::Storage(
            "Gateway rollout publication and replica certificate binding diverged".into(),
        ));
    }
    let current_certificate_ids = state
        .certificates
        .values()
        .filter(|certificate| {
            certificate.node_id == publication.node_id
                && certificate.gateway_revision == publication.revision
                && certificate.gateway_command_id == publication.command_id
        })
        .map(|certificate| certificate.id)
        .collect::<BTreeSet<_>>();
    let Some(expected_certificate_id) = replica.gateway_certificate_id else {
        if publication.certificate_request.is_some() || !current_certificate_ids.is_empty() {
            return Err(RepositoryError::Storage(
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
            RepositoryError::Storage(
                "Gateway rollout certificate request changed after staging".into(),
            )
        })?;
    let certificate = state
        .certificates
        .get(&expected_certificate_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Gateway rollout certificate disappeared".into())
        })?;
    let scope = state
        .gateway_scopes
        .get(&rollout.gateway_scope_id)
        .ok_or_else(|| RepositoryError::Storage("Gateway rollout scope disappeared".into()))?;
    if certificate.organization_id != scope.organization_id
        || certificate.node_id != publication.node_id
        || certificate.request != *request
    {
        return Err(RepositoryError::Storage(
            "Gateway rollout certificate identity or request changed".into(),
        ));
    }
    let is_new = certificate.gateway_revision == publication.revision
        && certificate.gateway_command_id == publication.command_id
        && certificate.snapshot_digest == publication.snapshot_digest;
    if is_new {
        if current_certificate_ids != BTreeSet::from([expected_certificate_id]) {
            return Err(RepositoryError::Storage(
                "Gateway rollout has inconsistent newly staged certificate material".into(),
            ));
        }
        return Ok(Some((certificate, false)));
    }
    if !current_certificate_ids.is_empty()
        || !state.rollout_rollbacks.values().any(|rollback| {
            rollback.rollback_rollout_id == rollout.id
                && rollback.state == GatewayRolloutRollbackState::Staged
        })
        || certificate.state != GatewayCertificateState::Ready
        || certificate.material.as_ref().is_none_or(|material| {
            material.validate().is_err()
                || material.issued_at > valid_at
                || material.expires_at <= valid_at
        })
    {
        return Err(RepositoryError::Storage(
            "Gateway rollback reused certificate is no longer exact and valid".into(),
        ));
    }
    Ok(Some((certificate, true)))
}

pub(in super::super) fn pending_dispatches(
    state: &State,
    limit: usize,
) -> Result<Vec<GatewayRolloutDispatchTarget>, RepositoryError> {
    validate_batch_limit(limit)?;
    let mut rollouts = state
        .rollouts
        .values()
        .filter(|rollout| {
            matches!(
                rollout.state,
                GatewayRolloutState::Pending | GatewayRolloutState::Ready
            )
        })
        .collect::<Vec<_>>();
    rollouts.sort_by_key(|rollout| (rollout.started_at, rollout.id));
    rollouts.truncate(limit);

    let mut targets = Vec::with_capacity(rollouts.len());
    for rollout in rollouts {
        let scope = state
            .gateway_scopes
            .get(&rollout.gateway_scope_id)
            .ok_or_else(|| RepositoryError::Storage("Gateway rollout scope disappeared".into()))?;
        let mut publications = Vec::new();
        for replica in rollout
            .replicas
            .iter()
            .filter(|replica| replica.state == GatewayReplicaRolloutState::Pending)
        {
            let publication = state
                .publications
                .get(&(replica.node_id, replica.revision))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Gateway rollout pending publication disappeared".into(),
                    )
                })?;
            if state
                .rollout_publications
                .get(&(replica.node_id, replica.revision))
                != Some(&rollout.id)
            {
                return Err(RepositoryError::Storage(
                    "Gateway rollout publication ownership is inconsistent".into(),
                ));
            }
            publications.push(publication);
        }
        publications.sort_by_key(|publication| publication.node_id);
        let target = GatewayRolloutDispatchTarget {
            organization_id: scope.organization_id,
            rollout: rollout.clone(),
            publications,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        targets.push(target);
    }
    Ok(targets)
}
