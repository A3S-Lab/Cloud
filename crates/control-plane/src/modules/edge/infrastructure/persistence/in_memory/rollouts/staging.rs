use super::super::State;
use crate::modules::edge::domain::repositories::{
    GatewayRolloutResult, GatewayRolloutRollbackResult, StageGatewayRollout,
    StageGatewayRolloutRollback,
};
use crate::modules::edge::domain::{
    GatewayPublicationState, GatewayRolloutRollbackState, GatewayRolloutState, GatewayScopeState,
};
use crate::modules::shared_kernel::domain::RepositoryError;

pub(in super::super) fn stage(
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
    if state
        .rollout_rollbacks
        .values()
        .any(|rollback| rollback.gateway_scope_id == bundle.scope.id && rollback.blocks_scope())
    {
        return Err(RepositoryError::Conflict(
            "Gateway scope has an unresolved exact rollback".into(),
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
    let primary_route = bundle
        .route_replicas
        .iter()
        .find(|route| route.gateway_node_id == bundle.scope.node_id)
        .cloned();
    if let Some(primary_route) = &primary_route {
        if state.routes.contains_key(&primary_route.id)
            || state.routes.values().any(|route| {
                route.gateway_scope_id == primary_route.gateway_scope_id
                    && route.hostname == primary_route.hostname
                    && route.path_prefix == primary_route.path_prefix
                    && matches!(
                        route.state,
                        crate::modules::edge::domain::RouteState::Publishing
                            | crate::modules::edge::domain::RouteState::Active
                    )
            })
        {
            return Err(RepositoryError::Conflict(
                "hostname and path are already owned in this Gateway scope".into(),
            ));
        }
        for route in &bundle.route_replicas {
            let ownership = (
                route.gateway_node_id,
                route.hostname.as_str().to_owned(),
                route.path_prefix.as_str().to_owned(),
            );
            if state.ownership.contains_key(&ownership)
                || state
                    .rollout_route_projections
                    .contains_key(&(bundle.rollout.id, route.gateway_node_id))
            {
                return Err(RepositoryError::Conflict(
                    "Gateway Route rollout projection identity already exists".into(),
                ));
            }
        }
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
    if let Some(primary_route) = primary_route {
        state.routes.insert(primary_route.id, primary_route);
        for route in &bundle.route_replicas {
            state.ownership.insert(
                (
                    route.gateway_node_id,
                    route.hostname.as_str().to_owned(),
                    route.path_prefix.as_str().to_owned(),
                ),
                route.id,
            );
            state
                .rollout_route_projections
                .insert((bundle.rollout.id, route.gateway_node_id), route.clone());
        }
    }
    let result = GatewayRolloutResult {
        rollout: bundle.rollout,
        route_replicas: bundle.route_replicas,
        publications: bundle.publications,
        certificates: bundle.certificates,
        replayed: false,
    };
    state.rollout_idempotency.insert(
        idempotency_key,
        (bundle.idempotency.request_digest, result.clone()),
    );
    state.outbox.push(bundle.event);
    if let Some(route_event) = bundle.route_event {
        state.outbox.push(route_event);
    }
    Ok(result)
}

pub(in super::super) fn stage_rollback(
    state: &mut State,
    bundle: StageGatewayRolloutRollback,
) -> Result<GatewayRolloutRollbackResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    if let Some(stored) = state.rollout_rollbacks.get(&bundle.failed_rollout.id) {
        if stored == &bundle.rollback {
            let rollout = state
                .rollouts
                .get(&bundle.rollout.id)
                .filter(|rollout| *rollout == &bundle.rollout)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "staged Gateway rollback intent lost its exact rollout".into(),
                    )
                })?;
            for publication in &bundle.publications {
                if state
                    .publications
                    .get(&(publication.node_id, publication.revision))
                    != Some(publication)
                    || state
                        .rollout_publications
                        .get(&(publication.node_id, publication.revision))
                        != Some(&rollout.id)
                {
                    return Err(RepositoryError::Storage(
                        "staged Gateway rollback lost an exact publication".into(),
                    ));
                }
            }
            for certificate in bundle
                .certificates
                .iter()
                .chain(&bundle.reused_certificates)
            {
                if state.certificates.get(&certificate.id) != Some(certificate) {
                    return Err(RepositoryError::Storage(
                        "staged Gateway rollback certificate evidence changed".into(),
                    ));
                }
            }
            return Ok(GatewayRolloutRollbackResult {
                rollback: stored.clone(),
                rollout,
                publications: bundle.publications,
                certificates: bundle.certificates,
                reused_certificates: bundle.reused_certificates,
                replayed: true,
            });
        }
    }
    let stored_scope = state
        .gateway_scopes
        .get(&bundle.scope.id)
        .ok_or(RepositoryError::NotFound)?;
    if stored_scope != &bundle.scope
        || state.rollouts.get(&bundle.failed_rollout.id) != Some(&bundle.failed_rollout)
    {
        return Err(RepositoryError::Conflict(
            "Gateway rollback source changed before exact staging".into(),
        ));
    }
    let stored_rollback = state
        .rollout_rollbacks
        .get(&bundle.failed_rollout.id)
        .ok_or_else(|| {
            RepositoryError::Conflict("Gateway rollback intent is not durable".into())
        })?;
    if stored_rollback.aggregate_version != bundle.expected_rollback_version
        || stored_rollback.state != GatewayRolloutRollbackState::Required
    {
        return Err(RepositoryError::Conflict(
            "Gateway rollback intent changed before staging".into(),
        ));
    }
    let mut expected_staged = stored_rollback.clone();
    expected_staged
        .stage(&bundle.rollout)
        .map_err(RepositoryError::Conflict)?;
    if expected_staged != bundle.rollback {
        return Err(RepositoryError::Conflict(
            "Gateway rollback staged projection changed".into(),
        ));
    }
    if state.rollouts.values().any(|rollout| {
        rollout.gateway_scope_id == bundle.scope.id
            && matches!(
                rollout.state,
                GatewayRolloutState::Pending | GatewayRolloutState::Ready
            )
    }) || state.rollouts.contains_key(&bundle.rollout.id)
        || state.rollouts.values().any(|rollout| {
            rollout.gateway_scope_id == bundle.rollout.gateway_scope_id
                && rollout.generation == bundle.rollout.generation
        })
    {
        return Err(RepositoryError::Conflict(
            "Gateway rollback rollout identity or generation is unavailable".into(),
        ));
    }

    let mut physical_scopes = Vec::with_capacity(bundle.publications.len());
    for publication in &bundle.publications {
        let current = state
            .scopes
            .get(&publication.node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(publication.node_id));
        if bundle
            .expected_scope_versions
            .get(&publication.node_id)
            .copied()
            != Some(current.aggregate_version)
            || publication.revision != current.next_revision().map_err(RepositoryError::Conflict)?
            || publication.expected_revision != current.installed_revision
        {
            return Err(RepositoryError::Conflict(
                "physical Gateway state changed before exact rollback staging".into(),
            ));
        }
        if state.publications.values().any(|existing| {
            existing.node_id == publication.node_id
                && existing.state == GatewayPublicationState::Pending
        }) || state
            .publications
            .contains_key(&(publication.node_id, publication.revision))
            || state
                .commands
                .contains_key(&(publication.node_id, publication.command_id))
        {
            return Err(RepositoryError::Conflict(
                "Gateway rollback member already has a pending or conflicting snapshot".into(),
            ));
        }
        let next_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
            RepositoryError::Conflict("physical Gateway scope version space is exhausted".into())
        })?;
        physical_scopes.push((current, next_version));
    }
    for certificate in &bundle.certificates {
        if state.certificates.contains_key(&certificate.id) {
            return Err(RepositoryError::Conflict(
                "new Gateway rollback certificate identity already exists".into(),
            ));
        }
    }
    for certificate in &bundle.reused_certificates {
        if state.certificates.get(&certificate.id) != Some(certificate) {
            return Err(RepositoryError::Conflict(
                "reused Gateway rollback certificate changed before staging".into(),
            ));
        }
    }

    for (publication, (current, next_version)) in bundle.publications.iter().zip(physical_scopes) {
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
                aggregate_version: next_version,
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
    state
        .rollout_rollbacks
        .insert(bundle.failed_rollout.id, bundle.rollback.clone());
    state.outbox.push(bundle.event);
    Ok(GatewayRolloutRollbackResult {
        rollback: bundle.rollback,
        rollout: bundle.rollout,
        publications: bundle.publications,
        certificates: bundle.certificates,
        reused_certificates: bundle.reused_certificates,
        replayed: false,
    })
}
