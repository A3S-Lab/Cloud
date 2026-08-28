use super::State;
use crate::modules::edge::domain::repositories::CreateGatewayScopeWrite;
use crate::modules::edge::domain::{GatewayScope, Route};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayScopeId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};

pub(super) fn create(
    state: &mut State,
    bundle: CreateGatewayScopeWrite,
) -> Result<IdempotentWrite<GatewayScope>, RepositoryError> {
    bundle.scope.validate().map_err(RepositoryError::Conflict)?;
    validate_event(&bundle)?;
    let key = (
        bundle.idempotency.scope.clone(),
        bundle.idempotency.key.clone(),
    );
    if let Some((digest, scope)) = state.gateway_scope_idempotency.get(&key) {
        if digest != &bundle.idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        return Ok(IdempotentWrite {
            value: scope.clone(),
            replayed: true,
        });
    }

    let bindings = bundle
        .scope
        .member_node_ids
        .iter()
        .map(|node_id| {
            (
                bundle.scope.organization_id,
                bundle.scope.project_id,
                bundle.scope.environment_id,
                *node_id,
            )
        })
        .collect::<Vec<_>>();
    if bindings
        .iter()
        .any(|binding| state.gateway_scope_bindings.contains_key(binding))
    {
        return Err(RepositoryError::Conflict(
            "Gateway node is already bound to this environment scope".into(),
        ));
    }
    if state.gateway_scopes.contains_key(&bundle.scope.id) {
        return Err(RepositoryError::Conflict(
            "Gateway scope identity already exists".into(),
        ));
    }

    for binding in bindings {
        state
            .gateway_scope_bindings
            .insert(binding, bundle.scope.id);
    }
    state
        .gateway_scopes
        .insert(bundle.scope.id, bundle.scope.clone());
    state.gateway_scope_idempotency.insert(
        key,
        (bundle.idempotency.request_digest, bundle.scope.clone()),
    );
    state.outbox.push(bundle.event);
    Ok(IdempotentWrite {
        value: bundle.scope,
        replayed: false,
    })
}

pub(super) fn find(
    state: &State,
    organization_id: OrganizationId,
    scope_id: GatewayScopeId,
) -> Result<GatewayScope, RepositoryError> {
    state
        .gateway_scopes
        .get(&scope_id)
        .filter(|scope| scope.organization_id == organization_id)
        .cloned()
        .ok_or(RepositoryError::NotFound)
}

pub(super) fn list(
    state: &State,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Vec<GatewayScope> {
    let mut scopes = state
        .gateway_scopes
        .values()
        .filter(|scope| {
            scope.organization_id == organization_id
                && scope.project_id == project_id
                && scope.environment_id == environment_id
        })
        .cloned()
        .collect::<Vec<_>>();
    scopes.sort_by_key(|scope| (scope.created_at, scope.id));
    scopes
}

pub(super) fn validate_route_binding(
    state: &State,
    expected: &GatewayScope,
    route: &Route,
) -> Result<(), RepositoryError> {
    let stored = state
        .gateway_scopes
        .get(&route.gateway_scope_id)
        .ok_or(RepositoryError::NotFound)?;
    if stored != expected
        || !stored.owns(
            route.organization_id,
            route.project_id,
            route.environment_id,
            route.gateway_node_id,
        )
    {
        return Err(RepositoryError::Conflict(
            "route does not belong to the selected Gateway scope".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_cutover_bindings(
    state: &State,
    routes: &[Route],
) -> Result<(), RepositoryError> {
    for route in routes {
        let scope = state
            .gateway_scopes
            .get(&route.gateway_scope_id)
            .ok_or(RepositoryError::NotFound)?;
        if !scope.owns(
            route.organization_id,
            route.project_id,
            route.environment_id,
            route.gateway_node_id,
        ) {
            return Err(RepositoryError::Conflict(
                "route cutover crossed its Gateway scope boundary".into(),
            ));
        }
    }
    Ok(())
}

fn validate_event(bundle: &CreateGatewayScopeWrite) -> Result<(), RepositoryError> {
    let scope = &bundle.scope;
    let event = &bundle.event;
    if scope.aggregate_version != 1
        || scope.updated_at != scope.created_at
        || event.event_key != "edge.gateway-scope.created"
        || event.schema_version != 2
        || event.organization_id() != Some(scope.organization_id.as_uuid())
        || event.aggregate_id != scope.id.as_uuid()
        || event.aggregate_version != scope.aggregate_version
        || event.occurred_at != scope.created_at
        || event.correlation_id.is_nil()
        || event.event_id.is_nil()
    {
        return Err(RepositoryError::Conflict(
            "Gateway scope event does not match its aggregate".into(),
        ));
    }
    Ok(())
}
