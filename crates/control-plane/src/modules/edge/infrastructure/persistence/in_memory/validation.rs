use super::*;

pub(super) fn validate_pending_cutover_routes(
    routes: &BTreeMap<RouteId, Route>,
    cutover: &GatewayRouteCutover,
) -> Result<(), RepositoryError> {
    let active_ids = routes
        .values()
        .filter(|route| {
            route.state == RouteState::Active
                && route.organization_id == cutover.organization_id
                && route.workload_id == cutover.workload_id
        })
        .map(|route| route.id)
        .collect::<BTreeSet<_>>();
    let candidate_ids = cutover
        .routes
        .iter()
        .map(|route| route.id)
        .collect::<BTreeSet<_>>();
    if active_ids.is_empty() || active_ids != candidate_ids {
        return Err(RepositoryError::Conflict(
            "Gateway route cutover must replace every active route for the previous revision"
                .into(),
        ));
    }
    for candidate in &cutover.routes {
        let current = routes.get(&candidate.id).ok_or(RepositoryError::NotFound)?;
        if !same_route_ownership(current, candidate)
            || current.state != RouteState::Active
            || current.target.workload_revision_id != cutover.previous_revision_id
            || current.target.runtime_generation != cutover.previous_generation
            || current.gateway_node_id != cutover.node_id
            || candidate.state != RouteState::Publishing
            || candidate.target.workload_revision_id != cutover.candidate_revision_id
            || candidate.target.runtime_generation != cutover.candidate_generation
            || candidate.gateway_certificate_id == current.gateway_certificate_id
            || candidate.aggregate_version != current.aggregate_version.saturating_add(1)
            || candidate.updated_at < current.updated_at
        {
            return Err(RepositoryError::Conflict(
                "active route changed while staging its Gateway cutover".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_applied_cutover_routes(
    routes: &BTreeMap<RouteId, Route>,
    cutover: &GatewayRouteCutover,
) -> Result<(), RepositoryError> {
    for candidate in &cutover.routes {
        let current = routes
            .get(&candidate.id)
            .ok_or_else(|| RepositoryError::Storage("cutover route disappeared".into()))?;
        if !same_route_ownership(current, candidate)
            || current.state != RouteState::Active
            || current.target.workload_revision_id != cutover.previous_revision_id
            || current.target.runtime_generation != cutover.previous_generation
            || candidate.state != RouteState::Active
            || candidate.target.workload_revision_id != cutover.candidate_revision_id
            || candidate.target.runtime_generation != cutover.candidate_generation
            || candidate.aggregate_version != current.aggregate_version.saturating_add(2)
            || candidate.updated_at < current.updated_at
        {
            return Err(RepositoryError::Conflict(
                "active route changed before applying its Gateway cutover".into(),
            ));
        }
    }
    Ok(())
}

fn same_route_ownership(current: &Route, candidate: &Route) -> bool {
    current.id == candidate.id
        && current.organization_id == candidate.organization_id
        && current.project_id == candidate.project_id
        && current.environment_id == candidate.environment_id
        && current.gateway_scope_id == candidate.gateway_scope_id
        && current.gateway_node_id == candidate.gateway_node_id
        && current.hostname == candidate.hostname
        && current.path_prefix == candidate.path_prefix
        && current.domain_claim_id == candidate.domain_claim_id
        && current.domain_pattern == candidate.domain_pattern
        && current.workload_id == candidate.workload_id
        && current.target.port_name == candidate.target.port_name
        && current.created_at == candidate.created_at
}

pub(super) fn validate_domain_event(
    claim: &DomainClaim,
    event: &DomainEventEnvelope,
) -> Result<(), RepositoryError> {
    if event.organization_id() != Some(claim.organization_id.as_uuid())
        || event.aggregate_id != claim.id.as_uuid()
        || event.aggregate_version != claim.aggregate_version
        || event.correlation_id.is_nil()
        || event.event_id.is_nil()
        || event.schema_version == 0
        || event.event_key.trim().is_empty()
    {
        return Err(RepositoryError::Conflict(
            "domain claim event does not match its aggregate".into(),
        ));
    }
    Ok(())
}
