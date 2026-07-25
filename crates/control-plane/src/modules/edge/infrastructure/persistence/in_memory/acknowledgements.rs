use super::{certificate_convergence, validate_applied_cutover_routes, State};
use crate::modules::edge::domain::{
    GatewayCertificateConvergenceState, GatewayPublicationState, GatewayRollout,
    GatewayRouteCutoverState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

pub(super) async fn project(
    state: &RwLock<State>,
    acknowledgement: &NodeGatewayAck,
    received_at: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    let mut acknowledgement = acknowledgement.clone();
    acknowledgement.acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
    let received_at = canonical_timestamp(received_at);
    acknowledgement
        .validate()
        .map_err(RepositoryError::Conflict)?;
    if received_at < acknowledgement.acknowledged_at {
        return Err(RepositoryError::Conflict(
            "Gateway acknowledgement receipt predates its node timestamp".into(),
        ));
    }
    let node_id = NodeId::from_uuid(acknowledgement.node_id);
    let command_id = NodeCommandId::from_uuid(acknowledgement.command_id);
    let mut stored_state = state.write().await;
    let mut state = stored_state.clone();
    let Some(revision) = state.commands.get(&(node_id, command_id)).copied() else {
        return Ok(false);
    };
    let mut publication = state
        .publications
        .get(&(node_id, revision))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway publication command references missing desired state".into(),
            )
        })?;
    let was_pending = publication.state == GatewayPublicationState::Pending;
    publication
        .acknowledge(&acknowledgement)
        .map_err(RepositoryError::Conflict)?;
    if !was_pending {
        return Ok(true);
    }
    if let Some(rollout_id) = state
        .rollout_publications
        .get(&(node_id, revision))
        .copied()
    {
        let mut rollout = state.rollouts.get(&rollout_id).cloned().ok_or_else(|| {
            RepositoryError::Storage("Gateway rollout publication lost its aggregate".into())
        })?;
        rollout
            .acknowledge(&acknowledgement)
            .map_err(RepositoryError::Conflict)?;
        let expected_certificate_id = rollout
            .replicas
            .iter()
            .find(|replica| {
                replica.node_id == node_id
                    && replica.command_id == command_id
                    && replica.revision == revision
            })
            .and_then(|replica| replica.gateway_certificate_id);
        let certificate_valid_at = match acknowledgement.state {
            GatewayAckState::Applied => acknowledgement.acknowledged_at,
            GatewayAckState::Rejected => rollout.started_at,
        };
        if let Some((mut certificate, reused)) = super::rollouts::certificate_binding(
            &state,
            &rollout,
            &publication,
            certificate_valid_at,
        )? {
            if !reused {
                certificate
                    .apply_gateway_acknowledgement(&acknowledgement)
                    .map_err(RepositoryError::Conflict)?;
                state.certificates.insert(certificate.id, certificate);
            }
        }
        project_rollout_route(&mut state, &rollout, &acknowledgement)?;
        super::rollouts::project_terminal_rollback(&mut state, &rollout)?;
        if acknowledgement.state == GatewayAckState::Applied {
            if let Some(certificate_id) = expected_certificate_id {
                certificate_convergence::bind_active_routes(
                    &mut state,
                    node_id,
                    revision,
                    command_id,
                    &acknowledgement.snapshot_digest,
                    certificate_id,
                    acknowledgement.acknowledged_at,
                )?;
            } else if certificate_convergence::has_active_routes(&state, node_id) {
                return Err(RepositoryError::Storage(
                    "certificate-free Gateway rollout retained active routes".into(),
                ));
            }
        }
        state.publications.insert((node_id, revision), publication);
        if acknowledgement.state == GatewayAckState::Applied {
            let scope = state.scopes.get_mut(&node_id).ok_or_else(|| {
                RepositoryError::Storage(
                    "physical Gateway scope disappeared during rollout acknowledgement".into(),
                )
            })?;
            scope.installed_revision = Some(revision);
            scope.aggregate_version = scope.aggregate_version.checked_add(1).ok_or_else(|| {
                RepositoryError::Storage(
                    "physical Gateway scope aggregate version space is exhausted".into(),
                )
            })?;
        }
        state.rollouts.insert(rollout_id, rollout);
        *stored_state = state;
        return Ok(true);
    }
    let certificate_ids = state
        .certificates
        .values()
        .filter(|certificate| {
            certificate.node_id == node_id
                && certificate.gateway_revision == revision
                && certificate.gateway_command_id == command_id
        })
        .map(|certificate| certificate.id)
        .collect::<Vec<_>>();
    let convergence_key = (node_id, revision);
    let mut convergence = state
        .certificate_convergences
        .get(&convergence_key)
        .cloned();
    let staged_certificate_id = match &convergence {
        Some(convergence) => convergence.replacement_certificate_id,
        None if certificate_ids.len() == 1 => Some(certificate_ids[0]),
        None => {
            return Err(RepositoryError::Storage(
                "Gateway publication must have exactly one staged certificate".into(),
            ))
        }
    };
    let active_certificate_id = convergence
        .as_ref()
        .and_then(|convergence| convergence.active_certificate_id())
        .or(staged_certificate_id);
    if certificate_ids.len() != usize::from(staged_certificate_id.is_some())
        || certificate_ids.first().copied() != staged_certificate_id
    {
        return Err(RepositoryError::Storage(
            "Gateway publication has inconsistent staged certificate material".into(),
        ));
    }
    let mut certificate = staged_certificate_id
        .map(|certificate_id| {
            state
                .certificates
                .get(&certificate_id)
                .cloned()
                .ok_or_else(|| RepositoryError::Storage("staged certificate disappeared".into()))
        })
        .transpose()?;
    if let Some(certificate) = &mut certificate {
        certificate
            .apply_gateway_acknowledgement(&acknowledgement)
            .map_err(RepositoryError::Conflict)?;
    }
    let route_ids = state
        .routes
        .values()
        .filter(|route| {
            route.gateway_node_id == node_id
                && route.gateway_revision == Some(revision)
                && route.gateway_command_id == Some(command_id)
        })
        .map(|route| route.id)
        .collect::<Vec<_>>();
    let cutover_id = state
        .cutovers
        .values()
        .find(|cutover| {
            cutover.node_id == node_id
                && cutover.gateway_revision == revision
                && cutover.gateway_command_id == command_id
        })
        .map(|cutover| cutover.deployment_id);
    let publication_kinds = usize::from(!route_ids.is_empty())
        + usize::from(cutover_id.is_some())
        + usize::from(convergence.is_some());
    if publication_kinds != 1 {
        return Err(RepositoryError::Storage(
            "Gateway publication must select one route publication kind".into(),
        ));
    }
    if let Some(convergence) = &mut convergence {
        convergence
            .acknowledge(&acknowledgement)
            .map_err(RepositoryError::Conflict)?;
        if convergence.state == GatewayCertificateConvergenceState::Applied {
            certificate_convergence::apply(&mut state, convergence, &acknowledgement)?;
        }
        state
            .certificate_convergences
            .insert(convergence_key, convergence.clone());
    } else if let Some(cutover_id) = cutover_id {
        let mut cutover = state
            .cutovers
            .get(&cutover_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("route cutover disappeared".into()))?;
        cutover
            .acknowledge(&acknowledgement)
            .map_err(RepositoryError::Conflict)?;
        if cutover.state == GatewayRouteCutoverState::Applied {
            validate_applied_cutover_routes(&state.routes, &cutover)?;
            for route in &cutover.routes {
                state.routes.insert(route.id, route.clone());
            }
        }
        state.cutovers.insert(cutover_id, cutover);
    } else {
        for route_id in route_ids {
            let ownership = {
                let route = state
                    .routes
                    .get_mut(&route_id)
                    .ok_or_else(|| RepositoryError::Storage("staged route disappeared".into()))?;
                route
                    .apply_gateway_acknowledgement(&acknowledgement)
                    .map_err(RepositoryError::Conflict)?;
                (route.state == RouteState::Rejected).then(|| {
                    (
                        route.gateway_node_id,
                        route.hostname.as_str().to_owned(),
                        route.path_prefix.as_str().to_owned(),
                    )
                })
            };
            if let Some(ownership) = ownership {
                state.ownership.remove(&ownership);
            }
        }
    }
    if let Some(certificate) = certificate {
        state.certificates.insert(certificate.id, certificate);
    }
    state.publications.insert((node_id, revision), publication);
    if acknowledgement.state == GatewayAckState::Applied {
        if let Some(certificate_id) = active_certificate_id {
            certificate_convergence::bind_active_routes(
                &mut state,
                node_id,
                revision,
                command_id,
                &acknowledgement.snapshot_digest,
                certificate_id,
                acknowledgement.acknowledged_at,
            )?;
        } else if certificate_convergence::has_active_routes(&state, node_id) {
            return Err(RepositoryError::Storage(
                "certificate-free Gateway snapshot retained active routes".into(),
            ));
        }
        let scope = state.scopes.get_mut(&node_id).ok_or_else(|| {
            RepositoryError::Storage("Gateway scope disappeared during acknowledgement".into())
        })?;
        scope.installed_revision = Some(revision);
        scope.aggregate_version = scope.aggregate_version.checked_add(1).ok_or_else(|| {
            RepositoryError::Storage("Gateway scope aggregate version space is exhausted".into())
        })?;
    }
    *stored_state = state;
    Ok(true)
}

fn project_rollout_route(
    state: &mut State,
    rollout: &GatewayRollout,
    acknowledgement: &NodeGatewayAck,
) -> Result<(), RepositoryError> {
    let node_id = NodeId::from_uuid(acknowledgement.node_id);
    let key = (rollout.id, node_id);
    let Some(mut projection) = state.rollout_route_projections.get(&key).cloned() else {
        if state
            .rollout_route_projections
            .keys()
            .any(|(rollout_id, _)| *rollout_id == rollout.id)
        {
            return Err(RepositoryError::Storage(
                "Gateway Route rollout omitted a member projection".into(),
            ));
        }
        return Ok(());
    };
    projection
        .apply_gateway_acknowledgement(acknowledgement)
        .map_err(RepositoryError::Conflict)?;
    let mut logical = state.routes.get(&projection.id).cloned().ok_or_else(|| {
        RepositoryError::Storage("Gateway Route rollout logical Route disappeared".into())
    })?;
    validate_logical_projection(&logical, &projection, rollout)?;
    let observed_at = rollout
        .replicas
        .iter()
        .filter_map(|replica| replica.acknowledged_at)
        .max()
        .unwrap_or(rollout.started_at);
    if rollout.serves_traffic().map_err(RepositoryError::Storage)? {
        logical
            .activate_from_gateway_rollout(observed_at)
            .map_err(RepositoryError::Conflict)?;
    } else if rollout.state.terminal() {
        logical
            .reject_from_gateway_rollout(
                "Gateway rollout did not reach its readiness threshold",
                observed_at,
            )
            .map_err(RepositoryError::Conflict)?;
    }
    state.rollout_route_projections.insert(key, projection);
    state.routes.insert(logical.id, logical);
    Ok(())
}

fn validate_logical_projection(
    logical: &Route,
    projection: &Route,
    rollout: &GatewayRollout,
) -> Result<(), RepositoryError> {
    if logical.id != projection.id
        || logical.organization_id != projection.organization_id
        || logical.project_id != projection.project_id
        || logical.environment_id != projection.environment_id
        || logical.gateway_scope_id != rollout.gateway_scope_id
        || projection.gateway_scope_id != rollout.gateway_scope_id
        || logical.hostname != projection.hostname
        || logical.path_prefix != projection.path_prefix
        || logical.domain_claim_id != projection.domain_claim_id
        || logical.domain_pattern != projection.domain_pattern
        || logical.workload_id != projection.workload_id
        || logical.target.workload_revision_id != projection.target.workload_revision_id
        || logical.target.runtime_unit_id != projection.target.runtime_unit_id
        || logical.target.runtime_generation != projection.target.runtime_generation
        || logical.target.port_name != projection.target.port_name
        || logical.created_at != projection.created_at
    {
        return Err(RepositoryError::Storage(
            "Gateway rollout logical and physical Route projections diverged".into(),
        ));
    }
    Ok(())
}
