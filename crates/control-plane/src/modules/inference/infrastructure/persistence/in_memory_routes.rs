use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::repositories::{
    IInferenceRouteRepository, InferenceRouteWriteReference, PublishInferenceRouteWrite,
    RetireInferenceRouteWrite,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, InferenceRouteId, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    routes: HashMap<InferenceRouteId, InferenceRoute>,
    idempotency: HashMap<(String, String), (String, InferenceRouteWriteReference)>,
}

/// Standalone in-memory authority for Inference route catalog heads.
#[derive(Clone, Default)]
pub struct InMemoryInferenceRouteRepository {
    state: Arc<RwLock<State>>,
}

#[async_trait]
impl IInferenceRouteRepository for InMemoryInferenceRouteRepository {
    async fn find_inference_route(
        &self,
        organization_id: OrganizationId,
        route_id: InferenceRouteId,
    ) -> Result<Option<InferenceRoute>, RepositoryError> {
        let state = self.state.read().await;
        Ok(state
            .routes
            .get(&route_id)
            .filter(|route| route.organization_id == organization_id)
            .cloned())
    }

    async fn list_inference_routes_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceRoute>, RepositoryError> {
        let state = self.state.read().await;
        let mut routes = state
            .routes
            .values()
            .filter(|route| {
                route.organization_id == organization_id
                    && route.project_id == project_id
                    && route.environment_id == environment_id
                    && !route.is_retired()
            })
            .cloned()
            .collect::<Vec<_>>();
        routes.sort_by_key(|route| route.id);
        Ok(routes)
    }

    async fn replay_inference_route_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<InferenceRoute>>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, organization_id, idempotency)
    }

    async fn publish_inference_route(
        &self,
        write: PublishInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(&state, write.route.organization_id, &write.idempotency)? {
            return Ok(replayed);
        }
        if state.routes.contains_key(&write.route.id) {
            return Err(RepositoryError::Conflict(
                "inference route identity is already in use".into(),
            ));
        }
        state.routes.insert(write.route.id, write.route.clone());
        remember(
            &mut state,
            write.idempotency,
            InferenceRouteWriteReference::from_route(&write.route),
        );
        Ok(IdempotentWrite {
            value: write.route,
            replayed: false,
        })
    }

    async fn retire_inference_route(
        &self,
        write: RetireInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        if let Some(replayed) = replay(&state, write.route.organization_id, &write.idempotency)? {
            return Ok(replayed);
        }
        let existing = state
            .routes
            .get(&write.route.id)
            .filter(|existing| existing.organization_id == write.route.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if write.is_noop() {
            if existing != write.route {
                return Err(RepositoryError::Conflict(
                    "inference route changed while applying retirement".into(),
                ));
            }
        } else {
            write
                .route
                .validate_transition_from(&existing, write.expected_aggregate_version)
                .map_err(RepositoryError::Conflict)?;
            state.routes.insert(write.route.id, write.route.clone());
        }
        remember(
            &mut state,
            write.idempotency,
            InferenceRouteWriteReference::from_route(&write.route),
        );
        Ok(IdempotentWrite {
            value: write.route,
            replayed: false,
        })
    }
}

fn replay(
    state: &State,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<InferenceRoute>>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, reference)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    if reference.organization_id != organization_id {
        return Err(RepositoryError::Storage(
            "stored inference route idempotency reference is invalid".into(),
        ));
    }
    let route = state
        .routes
        .get(&reference.route_id)
        .filter(|route| {
            route.organization_id == organization_id
                && route.aggregate_version() == reference.aggregate_version
        })
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("inference route idempotency target is missing".into())
        })?;
    Ok(Some(IdempotentWrite {
        value: route,
        replayed: true,
    }))
}

fn remember(
    state: &mut State,
    idempotency: IdempotencyRequest,
    reference: InferenceRouteWriteReference,
) {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    state
        .idempotency
        .insert(key, (idempotency.request_digest, reference));
}
