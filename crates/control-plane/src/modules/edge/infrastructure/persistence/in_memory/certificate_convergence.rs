use super::State;
use crate::modules::edge::domain::events::{
    GatewayCertificateExpiryChanged, GatewayCertificateRenewalChanged,
};
use crate::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, StageGatewayCertificateConvergence,
};
use crate::modules::edge::domain::{
    DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceReason, GatewayCertificateConvergenceState,
    GatewayCertificateState, GatewayPublication, GatewayPublicationState, GatewayRouteVersion,
    GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, GatewayRolloutId, NodeCommandId, NodeId,
    OrganizationId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::NodeGatewayAck;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn targets(
    state: &State,
    certificate_renew_before: DateTime<Utc>,
    snapshot_renew_before: DateTime<Utc>,
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
        let publication = state
            .publications
            .get(&(scope.node_id, installed_revision))
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "installed Gateway scope has no applied publication".into(),
                )
            })?;
        let mut routes = active_routes_for_node(state, scope.node_id)
            .into_iter()
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
        if routes.is_empty() {
            continue;
        }
        let certificate_id = routes
            .first()
            .and_then(|status| status.route.gateway_certificate_id)
            .ok_or_else(|| {
                RepositoryError::Storage("active Gateway route has no certificate".into())
            })?;
        if routes
            .iter()
            .any(|status| status.route.gateway_certificate_id != Some(certificate_id))
        {
            return Err(RepositoryError::Storage(
                "active Gateway routes disagree on their certificate".into(),
            ));
        }
        let certificate = state.certificates.get(&certificate_id).ok_or_else(|| {
            RepositoryError::Storage("active Gateway certificate disappeared".into())
        })?;
        let target = GatewayCertificateConvergenceTarget {
            scope: scope.clone(),
            publication,
            certificate: certificate.clone(),
            routes,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        if !needs_certificate_convergence(&target, certificate_renew_before, snapshot_renew_before)?
        {
            continue;
        }
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
    validate_active_certificate(state, convergence, previous.id)?;
    let retained_routes = retained_routes(state, convergence)?;
    let expected_expiry_events = GatewayCertificateExpiryChanged::envelopes(
        convergence,
        &bundle.publication,
        previous,
        &retained_routes,
    )
    .map_err(RepositoryError::Conflict)?;
    if bundle.expiry_events != expected_expiry_events {
        return Err(RepositoryError::Conflict(
            "Gateway certificate expiry firing facts are inconsistent".into(),
        ));
    }
    for candidate in &bundle.expiry_events {
        if let Some(existing) = state
            .outbox
            .iter()
            .find(|event| event.event_id == candidate.event_id)
        {
            let matches =
                GatewayCertificateExpiryChanged::same_firing_identity(existing, candidate)
                    .map_err(RepositoryError::Conflict)?;
            if !matches {
                return Err(RepositoryError::Conflict(
                    "Gateway certificate expiry firing event identity already exists".into(),
                ));
            }
        }
    }
    if convergence.reason
        == crate::modules::edge::domain::GatewayCertificateConvergenceReason::SnapshotRenewal
    {
        let installed_revision = current.installed_revision.ok_or_else(|| {
            RepositoryError::Storage("Gateway snapshot renewal has no installed revision".into())
        })?;
        let current_publication = state
            .publications
            .get(&(convergence.node_id, installed_revision))
            .ok_or_else(|| {
                RepositoryError::Storage("Gateway snapshot renewal publication disappeared".into())
            })?;
        if current_publication.state != GatewayPublicationState::Applied
            || current_publication
                .acknowledged_at
                .is_none_or(|acknowledged_at| {
                    bundle.publication.command_issued_at < acknowledged_at
                })
            || bundle.publication.acl != current_publication.acl
            || bundle.publication.snapshot_digest != current_publication.snapshot_digest
            || bundle.publication.certificate_request.is_some()
        {
            return Err(RepositoryError::Conflict(
                "Gateway snapshot renewal changed the installed policy".into(),
            ));
        }
    }
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
    for event in bundle.expiry_events {
        if !state
            .outbox
            .iter()
            .any(|existing| existing.event_id == event.event_id)
        {
            state.outbox.push(event);
        }
    }
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

#[allow(clippy::too_many_arguments)]
pub(super) fn mark_unavailable(
    state: &mut State,
    organization_id: OrganizationId,
    node_id: NodeId,
    gateway_revision: u64,
    gateway_command_id: NodeCommandId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    let key = (node_id, gateway_revision);
    let mut convergence = state
        .certificate_convergences
        .get(&key)
        .cloned()
        .ok_or(RepositoryError::NotFound)?;
    if convergence.organization_id != organization_id
        || convergence.gateway_command_id != gateway_command_id
    {
        return Err(RepositoryError::NotFound);
    }
    let mut publication = state.publications.get(&key).cloned().ok_or_else(|| {
        RepositoryError::Storage("Gateway certificate convergence publication disappeared".into())
    })?;
    if publication.command_id != gateway_command_id {
        return Err(RepositoryError::Storage(
            "Gateway certificate convergence command identity diverged".into(),
        ));
    }
    let publication_changed = publication
        .mark_unavailable(failure, observed_at)
        .map_err(RepositoryError::Conflict)?;
    let convergence_changed = convergence
        .mark_unavailable(failure, observed_at)
        .map_err(RepositoryError::Conflict)?;
    if publication_changed != convergence_changed {
        return Err(RepositoryError::Storage(
            "Gateway convergence terminal projections diverged".into(),
        ));
    }
    let certificate = convergence
        .replacement_certificate_id
        .map(|certificate_id| {
            let mut certificate = state
                .certificates
                .get(&certificate_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Gateway convergence replacement certificate disappeared".into(),
                    )
                })?;
            if certificate.organization_id != organization_id
                || certificate.node_id != node_id
                || certificate.gateway_revision != gateway_revision
                || certificate.gateway_command_id != gateway_command_id
            {
                return Err(RepositoryError::Storage(
                    "Gateway convergence replacement certificate identity diverged".into(),
                ));
            }
            certificate
                .mark_delivery_unavailable(failure, observed_at)
                .map_err(RepositoryError::Conflict)?;
            Ok(certificate)
        })
        .transpose()?;
    let events = if convergence_changed
        && convergence.reason == GatewayCertificateConvergenceReason::Renewal
    {
        let active_certificate = state
            .certificates
            .get(&convergence.previous_certificate_id)
            .ok_or_else(|| {
                RepositoryError::Storage("active Gateway renewal certificate disappeared".into())
            })?;
        certificate_events(state, &convergence, &publication, active_certificate)?
    } else {
        Vec::new()
    };
    state.publications.insert(key, publication.clone());
    state
        .certificate_convergences
        .insert(key, convergence.clone());
    if let Some(certificate) = &certificate {
        state
            .certificates
            .insert(certificate.id, certificate.clone());
    }
    state.outbox.extend(events);
    Ok(GatewayCertificateConvergenceResult {
        convergence,
        certificate,
        publication,
    })
}

pub(super) fn certificate_events(
    state: &State,
    convergence: &GatewayCertificateConvergence,
    publication: &GatewayPublication,
    active_certificate: &GatewayCertificate,
) -> Result<Vec<a3s_cloud_contracts::DomainEventEnvelope>, RepositoryError> {
    let routes = retained_routes(state, convergence)?;
    let mut events = GatewayCertificateRenewalChanged::envelopes(
        convergence,
        publication,
        active_certificate,
        &routes,
    )
    .map_err(RepositoryError::Storage)?;
    events.extend(
        GatewayCertificateExpiryChanged::envelopes(
            convergence,
            publication,
            active_certificate,
            &routes,
        )
        .map_err(RepositoryError::Storage)?,
    );
    Ok(events)
}

fn retained_routes(
    state: &State,
    convergence: &GatewayCertificateConvergence,
) -> Result<Vec<Route>, RepositoryError> {
    let active = active_routes_for_node(state, convergence.node_id)
        .into_iter()
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    convergence
        .retained_routes
        .iter()
        .map(|version| {
            active.get(&version.route_id).cloned().ok_or_else(|| {
                RepositoryError::Storage(
                    "Gateway certificate convergence retained Route disappeared".into(),
                )
            })
        })
        .collect()
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
                && !active_route_uses_certificate(state, certificate.id)
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
    let active_certificate_id = convergence.active_certificate_id();
    for version in &convergence.retained_routes {
        if let Some(key) = projection_key(state, version.route_id, convergence.node_id)? {
            let projection = state
                .rollout_route_projections
                .get_mut(&key)
                .ok_or_else(|| {
                    RepositoryError::Storage("retained Gateway Route projection disappeared".into())
                })?;
            if projection.aggregate_version != version.aggregate_version {
                return Err(RepositoryError::Conflict(
                    "retained Gateway Route projection changed before convergence applied".into(),
                ));
            }
            projection
                .bind_gateway_certificate(
                    convergence.gateway_revision,
                    convergence.gateway_command_id,
                    convergence.snapshot_digest.clone(),
                    active_certificate_id.ok_or_else(|| {
                        RepositoryError::Storage(
                            "retained convergence route has no active certificate".into(),
                        )
                    })?,
                    acknowledgement.acknowledged_at,
                )
                .map_err(RepositoryError::Conflict)?;
            let logical = state
                .routes
                .get_mut(&version.route_id)
                .ok_or(RepositoryError::NotFound)?;
            if logical.gateway_node_id == convergence.node_id {
                logical
                    .bind_gateway_certificate(
                        convergence.gateway_revision,
                        convergence.gateway_command_id,
                        convergence.snapshot_digest.clone(),
                        active_certificate_id.ok_or_else(|| {
                            RepositoryError::Storage(
                                "retained convergence route has no active certificate".into(),
                            )
                        })?,
                        acknowledgement.acknowledged_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
            }
            continue;
        }
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
                active_certificate_id.ok_or_else(|| {
                    RepositoryError::Storage(
                        "retained convergence route has no active certificate".into(),
                    )
                })?,
                acknowledgement.acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
    }
    for version in &convergence.rejected_routes {
        if let Some(key) = projection_key(state, version.route_id, convergence.node_id)? {
            let ownership_key = {
                let projection =
                    state
                        .rollout_route_projections
                        .get_mut(&key)
                        .ok_or_else(|| {
                            RepositoryError::Storage(
                                "rejected Gateway Route projection disappeared".into(),
                            )
                        })?;
                if projection.aggregate_version != version.aggregate_version {
                    return Err(RepositoryError::Conflict(
                        "rejected Gateway Route projection changed before convergence applied"
                            .into(),
                    ));
                }
                projection
                    .reject_for_domain_revocation(
                        convergence.gateway_revision,
                        convergence.gateway_command_id,
                        convergence.snapshot_digest.clone(),
                        acknowledgement.acknowledged_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                (
                    convergence.node_id,
                    projection.hostname.as_str().to_owned(),
                    projection.path_prefix.as_str().to_owned(),
                )
            };
            match state.ownership.remove(&ownership_key) {
                Some(route_id) if route_id == version.route_id => {}
                Some(_) => {
                    return Err(RepositoryError::Storage(
                        "domain revocation Route ownership changed identity".into(),
                    ))
                }
                None => {
                    return Err(RepositoryError::Storage(
                        "domain revocation Route ownership disappeared before acknowledgement"
                            .into(),
                    ))
                }
            }
            let has_active_projection =
                state.rollout_route_projections.values().any(|projection| {
                    projection.id == version.route_id && projection.state == RouteState::Active
                });
            if !has_active_projection {
                let primary_node_id = state
                    .routes
                    .get(&version.route_id)
                    .ok_or(RepositoryError::NotFound)?
                    .gateway_node_id;
                let primary_key = projection_key(state, version.route_id, primary_node_id)?
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "replicated domain revocation lost its primary Route projection".into(),
                        )
                    })?;
                let primary = state
                    .rollout_route_projections
                    .get(&primary_key)
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "replicated domain revocation primary projection disappeared".into(),
                        )
                    })?;
                if primary.state != RouteState::Rejected {
                    return Err(RepositoryError::Storage(
                        "replicated domain revocation completed before its primary member".into(),
                    ));
                }
                let logical = state
                    .routes
                    .get_mut(&version.route_id)
                    .ok_or(RepositoryError::NotFound)?;
                logical
                    .reject_for_domain_revocation(
                        primary.gateway_revision.ok_or_else(|| {
                            RepositoryError::Storage(
                                "rejected primary Route projection omitted its revision".into(),
                            )
                        })?,
                        primary.gateway_command_id.ok_or_else(|| {
                            RepositoryError::Storage(
                                "rejected primary Route projection omitted its command".into(),
                            )
                        })?,
                        primary.snapshot_digest.ok_or_else(|| {
                            RepositoryError::Storage(
                                "rejected primary Route projection omitted its digest".into(),
                            )
                        })?,
                        acknowledgement.acknowledged_at.max(primary.updated_at),
                    )
                    .map_err(RepositoryError::Conflict)?;
            }
            continue;
        }
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
        let route_id = route.id;
        state
            .ownership
            .retain(|_, owned_route_id| *owned_route_id != route_id);
    }
    Ok(())
}

fn projection_key(
    state: &State,
    route_id: RouteId,
    node_id: NodeId,
) -> Result<Option<(GatewayRolloutId, NodeId)>, RepositoryError> {
    let mut keys = state
        .rollout_route_projections
        .iter()
        .filter(|(_, route)| route.id == route_id && route.gateway_node_id == node_id)
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    keys.sort();
    match keys.as_slice() {
        [] => Ok(None),
        [key] => Ok(Some(*key)),
        _ => Err(RepositoryError::Storage(
            "one logical Route has duplicate physical projections on a Gateway member".into(),
        )),
    }
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
    let logical_active_route_ids = state
        .routes
        .values()
        .filter(|route| route.state == RouteState::Active)
        .map(|route| route.id)
        .collect::<std::collections::BTreeSet<_>>();
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
    let projection_keys = state
        .rollout_route_projections
        .iter()
        .filter(|(_, route)| {
            route.gateway_node_id == node_id
                && route.state == RouteState::Active
                && logical_active_route_ids.contains(&route.id)
        })
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    for key in projection_keys {
        state
            .rollout_route_projections
            .get_mut(&key)
            .ok_or_else(|| {
                RepositoryError::Storage("active Gateway Route projection disappeared".into())
            })?
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

pub(super) fn has_active_routes(state: &State, node_id: NodeId) -> bool {
    let projected_route_ids = state
        .rollout_route_projections
        .values()
        .map(|route| route.id)
        .collect::<BTreeSet<_>>();
    state.routes.values().any(|route| {
        route.gateway_node_id == node_id
            && route.state == RouteState::Active
            && !projected_route_ids.contains(&route.id)
    }) || state.rollout_route_projections.values().any(|projection| {
        projection.gateway_node_id == node_id
            && projection.state == RouteState::Active
            && state
                .routes
                .get(&projection.id)
                .is_some_and(|logical| logical.state == RouteState::Active)
    })
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
    target: &GatewayCertificateConvergenceTarget,
    certificate_renew_before: DateTime<Utc>,
    snapshot_renew_before: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    let expires_at = target
        .certificate
        .material
        .as_ref()
        .map(|material| material.expires_at)
        .ok_or_else(|| {
            RepositoryError::Storage("installed Gateway certificate has no material".into())
        })?;
    Ok(target.certificate.state == GatewayCertificateState::Revoked
        || expires_at <= canonical_timestamp(certificate_renew_before)
        || target.publication.snapshot_expires_at <= canonical_timestamp(snapshot_renew_before)
        || target.routes.iter().any(|status| {
            status.domain_claim_state != DomainClaimState::Verified
                || status.route.gateway_revision != target.scope.installed_revision
                || status.route.gateway_command_id != Some(target.publication.command_id)
                || status.route.snapshot_digest.as_deref()
                    != Some(target.publication.snapshot_digest.as_str())
                || status.route.gateway_certificate_id != Some(target.certificate.id)
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
    let active_routes = active_routes_for_node(state, convergence.node_id);
    let active = active_routes
        .iter()
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

fn validate_active_certificate(
    state: &State,
    convergence: &GatewayCertificateConvergence,
    certificate_id: GatewayCertificateId,
) -> Result<(), RepositoryError> {
    if active_routes_for_node(state, convergence.node_id)
        .iter()
        .any(|route| route.gateway_certificate_id != Some(certificate_id))
    {
        return Err(RepositoryError::Conflict(
            "active Gateway routes changed certificate during convergence".into(),
        ));
    }
    Ok(())
}

fn active_routes_for_node(state: &State, node_id: NodeId) -> Vec<Route> {
    let projected_route_ids = state
        .rollout_route_projections
        .values()
        .map(|route| route.id)
        .collect::<BTreeSet<_>>();
    let mut routes = state
        .routes
        .values()
        .filter(|route| {
            route.gateway_node_id == node_id
                && route.state == RouteState::Active
                && !projected_route_ids.contains(&route.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    routes.extend(
        state
            .rollout_route_projections
            .values()
            .filter(|projection| {
                projection.gateway_node_id == node_id
                    && projection.state == RouteState::Active
                    && state
                        .routes
                        .get(&projection.id)
                        .is_some_and(|logical| logical.state == RouteState::Active)
            })
            .cloned(),
    );
    routes.sort_by_key(|route| route.id);
    routes
}

fn active_route_uses_certificate(state: &State, certificate_id: GatewayCertificateId) -> bool {
    state.routes.values().any(|route| {
        route.state == RouteState::Active && route.gateway_certificate_id == Some(certificate_id)
    }) || state.rollout_route_projections.values().any(|projection| {
        projection.state == RouteState::Active
            && projection.gateway_certificate_id == Some(certificate_id)
            && state
                .routes
                .get(&projection.id)
                .is_some_and(|logical| logical.state == RouteState::Active)
    })
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
