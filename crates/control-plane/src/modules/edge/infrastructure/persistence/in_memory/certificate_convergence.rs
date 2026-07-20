use super::State;
use crate::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, StageGatewayCertificateConvergence,
};
use crate::modules::edge::domain::{
    DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceState, GatewayCertificateState, GatewayPublicationState,
    GatewayRouteVersion, GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeCommandId, NodeId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::NodeGatewayAck;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn targets(
    state: &State,
    renew_before: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<GatewayCertificateConvergenceTarget>, RepositoryError> {
    validate_batch_limit(limit)?;
    let mut targets = Vec::new();
    for scope in state.scopes.values() {
        let Some(installed_revision) = scope.installed_revision else {
            continue;
        };
        if state.publications.values().any(|publication| {
            publication.node_id == scope.node_id
                && publication.state == GatewayPublicationState::Pending
        }) {
            continue;
        }
        let Some(certificate) = state.certificates.values().find(|certificate| {
            certificate.node_id == scope.node_id
                && certificate.gateway_revision == installed_revision
        }) else {
            return Err(RepositoryError::Storage(
                "installed Gateway scope has no certificate".into(),
            ));
        };
        if !matches!(
            certificate.state,
            GatewayCertificateState::Ready | GatewayCertificateState::Revoked
        ) {
            return Err(RepositoryError::Storage(
                "installed Gateway certificate is not terminally usable".into(),
            ));
        }
        let mut routes = state
            .routes
            .values()
            .filter(|route| {
                route.gateway_node_id == scope.node_id && route.state == RouteState::Active
            })
            .cloned()
            .map(|route| {
                let domain_claim_state = route
                    .domain_claim_id
                    .and_then(|claim_id| state.domain_claims.get(&claim_id))
                    .map(|claim| claim.state)
                    .unwrap_or(DomainClaimState::Revoked);
                GatewayCertificateRouteStatus {
                    route,
                    domain_claim_state,
                }
            })
            .collect::<Vec<_>>();
        routes.sort_by_key(|status| status.route.id);
        if routes.is_empty()
            || !needs_certificate_convergence(scope, certificate, &routes, renew_before)?
        {
            continue;
        }
        let target = GatewayCertificateConvergenceTarget {
            scope: scope.clone(),
            certificate: certificate.clone(),
            routes,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        targets.push(target);
        if targets.len() == limit {
            break;
        }
    }
    Ok(targets)
}

pub(super) fn pending(
    state: &State,
    limit: usize,
) -> Result<Vec<GatewayCertificateConvergenceResult>, RepositoryError> {
    validate_batch_limit(limit)?;
    state
        .certificate_convergences
        .values()
        .filter(|convergence| convergence.state == GatewayCertificateConvergenceState::Pending)
        .take(limit)
        .map(|convergence| convergence_result(state, convergence))
        .collect()
}

pub(super) fn stage(
    state: &mut State,
    bundle: StageGatewayCertificateConvergence,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    let convergence = &bundle.convergence;
    let current = state
        .scopes
        .get(&convergence.node_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "Gateway certificate convergence requires an installed scope".into(),
            )
        })?;
    if current.aggregate_version != bundle.expected_scope_version
        || current.installed_revision != bundle.publication.expected_revision
        || bundle.publication.revision
            != current.next_revision().map_err(RepositoryError::Conflict)?
    {
        return Err(RepositoryError::Conflict(
            "Gateway scope changed while compiling certificate convergence".into(),
        ));
    }
    if state.publications.values().any(|publication| {
        publication.node_id == convergence.node_id
            && publication.state == GatewayPublicationState::Pending
    }) {
        return Err(RepositoryError::Conflict(
            "Gateway scope already has a pending complete snapshot".into(),
        ));
    }
    if state
        .certificate_convergences
        .contains_key(&(convergence.node_id, convergence.gateway_revision))
    {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence identity already exists".into(),
        ));
    }
    let previous = state
        .certificates
        .get(&convergence.previous_certificate_id)
        .ok_or(RepositoryError::NotFound)?;
    if previous.organization_id != convergence.organization_id
        || previous.node_id != convergence.node_id
        || Some(previous.gateway_revision) != current.installed_revision
        || !matches!(
            previous.state,
            GatewayCertificateState::Ready | GatewayCertificateState::Revoked
        )
    {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence previous certificate is not authoritative".into(),
        ));
    }
    validate_convergence_routes(state, convergence)?;
    if let Some(certificate) = &bundle.certificate {
        if state.certificates.contains_key(&certificate.id) {
            return Err(RepositoryError::Conflict(
                "Gateway replacement certificate identity already exists".into(),
            ));
        }
        validate_replacement_claims(state, convergence, certificate)?;
    }

    let result = GatewayCertificateConvergenceResult {
        convergence: bundle.convergence.clone(),
        certificate: bundle.certificate.clone(),
        publication: bundle.publication.clone(),
    };
    if let Some(certificate) = bundle.certificate {
        state.certificates.insert(certificate.id, certificate);
    }
    state.publications.insert(
        (bundle.publication.node_id, bundle.publication.revision),
        bundle.publication.clone(),
    );
    state.commands.insert(
        (bundle.publication.node_id, bundle.publication.command_id),
        bundle.publication.revision,
    );
    state.certificate_convergences.insert(
        (
            bundle.convergence.node_id,
            bundle.convergence.gateway_revision,
        ),
        bundle.convergence,
    );
    state.scopes.insert(
        bundle.publication.node_id,
        GatewayScopeState {
            node_id: bundle.publication.node_id,
            last_issued_revision: bundle.publication.revision,
            installed_revision: current.installed_revision,
            aggregate_version: current.aggregate_version + 1,
        },
    );
    state.outbox.push(bundle.event);
    Ok(result)
}

pub(super) fn find(
    state: &State,
    node_id: NodeId,
    gateway_revision: u64,
) -> Option<GatewayCertificateConvergence> {
    state
        .certificate_convergences
        .get(&(node_id, gateway_revision))
        .cloned()
}

pub(super) fn obsolete(
    state: &State,
    limit: usize,
) -> Result<Vec<GatewayCertificate>, RepositoryError> {
    validate_batch_limit(limit)?;
    Ok(state
        .certificates
        .values()
        .filter(|certificate| {
            certificate.state == GatewayCertificateState::Ready
                && state
                    .scopes
                    .get(&certificate.node_id)
                    .and_then(|scope| scope.installed_revision)
                    .is_some_and(|installed| installed > certificate.gateway_revision)
                && !state.routes.values().any(|route| {
                    route.state == RouteState::Active
                        && route.gateway_certificate_id == Some(certificate.id)
                })
        })
        .take(limit)
        .cloned()
        .collect())
}

pub(super) fn apply(
    state: &mut State,
    convergence: &GatewayCertificateConvergence,
    acknowledgement: &NodeGatewayAck,
) -> Result<(), RepositoryError> {
    for version in &convergence.retained_routes {
        let route = state
            .routes
            .get_mut(&version.route_id)
            .ok_or(RepositoryError::NotFound)?;
        if route.aggregate_version != version.aggregate_version {
            return Err(RepositoryError::Conflict(
                "retained route changed before certificate convergence applied".into(),
            ));
        }
        route
            .bind_gateway_certificate(
                convergence.gateway_revision,
                convergence.gateway_command_id,
                convergence.snapshot_digest.clone(),
                convergence.replacement_certificate_id.ok_or_else(|| {
                    RepositoryError::Storage(
                        "retained convergence route has no replacement certificate".into(),
                    )
                })?,
                acknowledgement.acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
    }
    for version in &convergence.rejected_routes {
        let route = state
            .routes
            .get_mut(&version.route_id)
            .ok_or(RepositoryError::NotFound)?;
        if route.aggregate_version != version.aggregate_version {
            return Err(RepositoryError::Conflict(
                "rejected route changed before certificate convergence applied".into(),
            ));
        }
        route
            .reject_for_domain_revocation(
                convergence.gateway_revision,
                convergence.gateway_command_id,
                convergence.snapshot_digest.clone(),
                acknowledgement.acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
        state.ownership.remove(&(
            route.gateway_node_id,
            route.hostname.as_str().to_owned(),
            route.path_prefix.as_str().to_owned(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn bind_active_routes(
    state: &mut State,
    node_id: NodeId,
    revision: u64,
    command_id: NodeCommandId,
    snapshot_digest: &str,
    certificate_id: GatewayCertificateId,
    acknowledged_at: DateTime<Utc>,
) -> Result<(), RepositoryError> {
    let route_ids = state
        .routes
        .values()
        .filter(|route| route.gateway_node_id == node_id && route.state == RouteState::Active)
        .map(|route| route.id)
        .collect::<Vec<_>>();
    for route_id in route_ids {
        state
            .routes
            .get_mut(&route_id)
            .ok_or_else(|| RepositoryError::Storage("active route disappeared".into()))?
            .bind_gateway_certificate(
                revision,
                command_id,
                snapshot_digest.into(),
                certificate_id,
                acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
    }
    Ok(())
}

fn validate_batch_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence batch limit is invalid".into(),
        ));
    }
    Ok(())
}

fn needs_certificate_convergence(
    scope: &GatewayScopeState,
    certificate: &GatewayCertificate,
    routes: &[GatewayCertificateRouteStatus],
    renew_before: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    let expires_at = certificate
        .material
        .as_ref()
        .map(|material| material.expires_at)
        .ok_or_else(|| {
            RepositoryError::Storage("installed Gateway certificate has no material".into())
        })?;
    Ok(certificate.state == GatewayCertificateState::Revoked
        || expires_at <= canonical_timestamp(renew_before)
        || routes.iter().any(|status| {
            status.domain_claim_state != DomainClaimState::Verified
                || status.route.gateway_revision != scope.installed_revision
                || status.route.gateway_command_id != Some(certificate.gateway_command_id)
                || status.route.snapshot_digest.as_deref()
                    != Some(certificate.snapshot_digest.as_str())
                || status.route.gateway_certificate_id != Some(certificate.id)
        }))
}

fn convergence_result(
    state: &State,
    convergence: &GatewayCertificateConvergence,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    let publication = state
        .publications
        .get(&(convergence.node_id, convergence.gateway_revision))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway certificate convergence publication disappeared".into(),
            )
        })?;
    let certificate = convergence
        .replacement_certificate_id
        .map(|certificate_id| {
            state
                .certificates
                .get(&certificate_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Gateway convergence replacement certificate disappeared".into(),
                    )
                })
        })
        .transpose()?;
    Ok(GatewayCertificateConvergenceResult {
        convergence: convergence.clone(),
        certificate,
        publication,
    })
}

fn validate_convergence_routes(
    state: &State,
    convergence: &GatewayCertificateConvergence,
) -> Result<(), RepositoryError> {
    let active = state
        .routes
        .values()
        .filter(|route| {
            route.gateway_node_id == convergence.node_id && route.state == RouteState::Active
        })
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    let planned = convergence
        .retained_routes
        .iter()
        .chain(&convergence.rejected_routes)
        .map(|version| version.route_id)
        .collect::<BTreeSet<_>>();
    if active.keys().copied().collect::<BTreeSet<_>>() != planned {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence must classify every active route".into(),
        ));
    }
    validate_route_versions_and_claims(state, &active, &convergence.retained_routes, true)?;
    validate_route_versions_and_claims(state, &active, &convergence.rejected_routes, false)
}

fn validate_route_versions_and_claims(
    state: &State,
    active: &BTreeMap<RouteId, &Route>,
    versions: &[GatewayRouteVersion],
    must_be_verified: bool,
) -> Result<(), RepositoryError> {
    for version in versions {
        let route = active
            .get(&version.route_id)
            .ok_or(RepositoryError::NotFound)?;
        let claim_state = route
            .domain_claim_id
            .and_then(|claim_id| state.domain_claims.get(&claim_id))
            .map(|claim| claim.state)
            .unwrap_or(DomainClaimState::Revoked);
        if route.aggregate_version != version.aggregate_version
            || (claim_state == DomainClaimState::Verified) != must_be_verified
        {
            return Err(RepositoryError::Conflict(
                "active route or domain ownership changed during certificate convergence".into(),
            ));
        }
    }
    Ok(())
}

fn validate_replacement_claims(
    state: &State,
    convergence: &GatewayCertificateConvergence,
    certificate: &GatewayCertificate,
) -> Result<(), RepositoryError> {
    let mut expected_claims = convergence
        .retained_routes
        .iter()
        .filter_map(|version| {
            state
                .routes
                .get(&version.route_id)
                .and_then(|route| route.domain_claim_id)
        })
        .collect::<Vec<_>>();
    expected_claims.sort();
    expected_claims.dedup();
    if expected_claims != certificate.domain_claim_ids {
        return Err(RepositoryError::Conflict(
            "Gateway replacement certificate does not cover retained route claims".into(),
        ));
    }
    Ok(())
}
