use super::{certificate_convergence::active_routes_for_node, State};
use crate::modules::edge::domain::events::{
    expiry_risk_subject_id, GatewayCertificateExpiryRiskChanged,
};
use crate::modules::edge::domain::repositories::GatewayCertificateExpiryRiskTarget;
use crate::modules::edge::domain::{
    expiry_risk_deadline, GatewayCertificateExpiryRisk, GatewayCertificateExpiryRiskState,
    GatewayCertificateState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeId, OrganizationId, RepositoryError, RouteId,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) fn targets(
    state: &State,
    risk_before: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<GatewayCertificateExpiryRiskTarget>, RepositoryError> {
    validate_limit(limit)?;
    let risk_before = canonical_timestamp(risk_before);
    let mut targets = Vec::new();
    for node_id in state.scopes.keys().copied() {
        for route in active_routes_for_node(state, node_id) {
            let Some(certificate_id) = route.gateway_certificate_id else {
                return Err(RepositoryError::Storage(
                    "active Gateway expiry-risk Route omitted its certificate".into(),
                ));
            };
            let certificate = state
                .certificates
                .get(&certificate_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Gateway expiry-risk certificate disappeared".into(),
                    )
                })?;
            let expires_at = certificate
                .material
                .as_ref()
                .map(|material| material.expires_at)
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "active Gateway expiry-risk certificate omitted material".into(),
                    )
                })?;
            if certificate.state != GatewayCertificateState::Ready || expires_at > risk_before {
                continue;
            }
            if state
                .certificate_expiry_risks
                .get(&(route.id, node_id))
                .is_some_and(|risk| {
                    risk.state == GatewayCertificateExpiryRiskState::AtRisk
                        && risk.active_certificate_id == certificate_id
                        && risk.active_certificate_expires_at == expires_at
                })
            {
                continue;
            }
            let target = GatewayCertificateExpiryRiskTarget { route, certificate };
            target
                .validate(risk_before)
                .map_err(RepositoryError::Storage)?;
            targets.push(target);
        }
    }
    targets.sort_by_key(|target| {
        (
            target
                .certificate
                .material
                .as_ref()
                .map(|material| material.expires_at),
            target.route.id,
            target.route.gateway_node_id,
        )
    });
    targets.truncate(limit);
    Ok(targets)
}

pub(super) fn mark_at_risk(
    state: &mut State,
    organization_id: OrganizationId,
    route_id: RouteId,
    node_id: NodeId,
    certificate_id: GatewayCertificateId,
    observed_at: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    let observed_at = canonical_timestamp(observed_at);
    let risk_before = expiry_risk_deadline(observed_at).map_err(RepositoryError::Conflict)?;
    let Some(route) = active_routes_for_node(state, node_id)
        .into_iter()
        .find(|route| {
            route.id == route_id
                && route.organization_id == organization_id
                && route.gateway_certificate_id == Some(certificate_id)
        })
    else {
        return Ok(false);
    };
    let Some(certificate) = state.certificates.get(&certificate_id).cloned() else {
        return Ok(false);
    };
    if certificate.state != GatewayCertificateState::Ready
        || certificate
            .material
            .as_ref()
            .is_none_or(|material| material.expires_at > risk_before)
    {
        return Ok(false);
    }
    let previous = state
        .certificate_expiry_risks
        .get(&(route_id, node_id))
        .cloned();
    let Some(risk) =
        GatewayCertificateExpiryRisk::observe(previous.as_ref(), &route, &certificate, observed_at)
            .map_err(RepositoryError::Conflict)?
    else {
        return Ok(false);
    };
    if risk.state != GatewayCertificateExpiryRiskState::AtRisk {
        return Err(RepositoryError::Conflict(
            "Gateway certificate expiry-risk scan cannot infer recovery".into(),
        ));
    }
    let event = GatewayCertificateExpiryRiskChanged::envelope(
        previous.as_ref(),
        &risk,
        &route,
        expiry_risk_subject_id(route_id, node_id),
    )
    .map_err(RepositoryError::Storage)?;
    state
        .certificate_expiry_risks
        .insert((route_id, node_id), risk);
    state.outbox.push(event);
    Ok(true)
}

pub(super) fn observe_applied_certificate(
    state: &mut State,
    node_id: NodeId,
    certificate_id: GatewayCertificateId,
    observed_at: DateTime<Utc>,
    correlation_id: Uuid,
) -> Result<usize, RepositoryError> {
    let certificate = state
        .certificates
        .get(&certificate_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("applied Gateway expiry-risk certificate disappeared".into())
        })?;
    let mut transitions = Vec::new();
    for route in active_routes_for_node(state, node_id)
        .into_iter()
        .filter(|route| route.gateway_certificate_id == Some(certificate_id))
    {
        let previous = state
            .certificate_expiry_risks
            .get(&(route.id, node_id))
            .cloned();
        let Some(risk) = GatewayCertificateExpiryRisk::observe(
            previous.as_ref(),
            &route,
            &certificate,
            observed_at,
        )
        .map_err(RepositoryError::Conflict)?
        else {
            continue;
        };
        let event = GatewayCertificateExpiryRiskChanged::envelope(
            previous.as_ref(),
            &risk,
            &route,
            correlation_id,
        )
        .map_err(RepositoryError::Storage)?;
        transitions.push(((route.id, node_id), risk, event));
    }
    let count = transitions.len();
    for (key, risk, event) in transitions {
        state.certificate_expiry_risks.insert(key, risk);
        state.outbox.push(event);
    }
    Ok(count)
}

fn validate_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway certificate expiry-risk batch limit is invalid".into(),
        ));
    }
    Ok(())
}
