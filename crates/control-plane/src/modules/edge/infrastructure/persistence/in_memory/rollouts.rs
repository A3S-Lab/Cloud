use super::State;
use crate::modules::edge::domain::repositories::{GatewayRolloutResult, StageGatewayRollout};
use crate::modules::edge::domain::{
    GatewayPublicationState, GatewayRollout, GatewayRolloutState, GatewayScopeState,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, NodeId, OrganizationId, RepositoryError,
};
use chrono::{DateTime, Utc};

pub(super) fn stage(
    state: &mut State,
    bundle: StageGatewayRollout,
) -> Result<GatewayRolloutResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    let idempotency_key = (
        bundle.idempotency.scope.clone(),
        bundle.idempotency.key.clone(),
    );
    if let Some((digest, existing)) = state.rollout_idempotency.get(&idempotency_key) {
        if digest != &bundle.idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        let mut replay = existing.clone();
        replay.replayed = true;
        return Ok(replay);
    }
    let stored_scope = state
        .gateway_scopes
        .get(&bundle.scope.id)
        .ok_or(RepositoryError::NotFound)?;
    if stored_scope != &bundle.scope {
        return Err(RepositoryError::Conflict(
            "Gateway scope changed while staging its rollout".into(),
        ));
    }
    if state.rollouts.values().any(|rollout| {
        rollout.gateway_scope_id == bundle.scope.id
            && matches!(
                rollout.state,
                GatewayRolloutState::Pending | GatewayRolloutState::Ready
            )
    }) {
        return Err(RepositoryError::Conflict(
            "Gateway scope already has an active rollout".into(),
        ));
    }
    if state.rollouts.contains_key(&bundle.rollout.id)
        || state.rollouts.values().any(|rollout| {
            rollout.gateway_scope_id == bundle.rollout.gateway_scope_id
                && rollout.generation == bundle.rollout.generation
        })
    {
        return Err(RepositoryError::Conflict(
            "Gateway rollout identity or generation already exists".into(),
        ));
    }

    let mut physical_scopes = Vec::with_capacity(bundle.publications.len());
    for publication in &bundle.publications {
        let current = state
            .scopes
            .get(&publication.node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(publication.node_id));
        let expected_version = bundle
            .expected_scope_versions
            .get(&publication.node_id)
            .copied()
            .ok_or_else(|| {
                RepositoryError::Conflict("Gateway rollout omitted a physical scope version".into())
            })?;
        if current.aggregate_version != expected_version {
            return Err(RepositoryError::Conflict(
                "physical Gateway scope changed while staging its rollout".into(),
            ));
        }
        if state.publications.values().any(|existing| {
            existing.node_id == publication.node_id
                && existing.state == GatewayPublicationState::Pending
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway rollout member already has a pending complete snapshot".into(),
            ));
        }
        if publication.revision != current.next_revision().map_err(RepositoryError::Conflict)?
            || publication.expected_revision != current.installed_revision
        {
            return Err(RepositoryError::Conflict(
                "Gateway rollout publication does not advance its physical revision".into(),
            ));
        }
        let next_aggregate_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
            RepositoryError::Conflict(
                "physical Gateway scope aggregate version space is exhausted".into(),
            )
        })?;
        if state
            .publications
            .contains_key(&(publication.node_id, publication.revision))
            || state
                .commands
                .contains_key(&(publication.node_id, publication.command_id))
        {
            return Err(RepositoryError::Conflict(
                "Gateway rollout publication identity already exists".into(),
            ));
        }
        physical_scopes.push((current, next_aggregate_version));
    }
    if bundle
        .certificates
        .iter()
        .any(|certificate| state.certificates.contains_key(&certificate.id))
    {
        return Err(RepositoryError::Conflict(
            "Gateway rollout certificate identity already exists".into(),
        ));
    }

    for (publication, (current, next_aggregate_version)) in
        bundle.publications.iter().zip(physical_scopes)
    {
        state.publications.insert(
            (publication.node_id, publication.revision),
            publication.clone(),
        );
        state.commands.insert(
            (publication.node_id, publication.command_id),
            publication.revision,
        );
        state.rollout_publications.insert(
            (publication.node_id, publication.revision),
            bundle.rollout.id,
        );
        state.scopes.insert(
            publication.node_id,
            GatewayScopeState {
                node_id: publication.node_id,
                last_issued_revision: publication.revision,
                installed_revision: current.installed_revision,
                aggregate_version: next_aggregate_version,
            },
        );
    }
    for certificate in &bundle.certificates {
        state
            .certificates
            .insert(certificate.id, certificate.clone());
    }
    state
        .rollouts
        .insert(bundle.rollout.id, bundle.rollout.clone());
    let result = GatewayRolloutResult {
        rollout: bundle.rollout,
        publications: bundle.publications,
        certificates: bundle.certificates,
        replayed: false,
    };
    state.rollout_idempotency.insert(
        idempotency_key,
        (bundle.idempotency.request_digest, result.clone()),
    );
    state.outbox.push(bundle.event);
    Ok(result)
}

pub(super) fn find(
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

pub(super) fn mark_unavailable(
    state: &mut State,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    let mut rollout = find(state, organization_id, rollout_id)?;
    if rollout.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(
            "Gateway rollout changed before unavailability was recorded".into(),
        ));
    }
    rollout
        .mark_unavailable(node_id, failure, observed_at)
        .map_err(RepositoryError::Conflict)?;
    state.rollouts.insert(rollout_id, rollout.clone());
    Ok(rollout)
}
