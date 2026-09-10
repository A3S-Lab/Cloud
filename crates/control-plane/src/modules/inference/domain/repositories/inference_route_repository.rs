use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, InferenceRouteId, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Idempotent publish of one Inference route catalog head.
#[derive(Debug, Clone)]
pub struct PublishInferenceRouteWrite {
    pub route: InferenceRoute,
    pub idempotency: IdempotencyRequest,
}

impl PublishInferenceRouteWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.idempotency.validate()?;
        if self.route.aggregate_version() != 1
            || self.route.policy_revision() != 1
            || self.route.created_at() != self.route.updated_at()
            || self.route.retired_at().is_some()
        {
            return Err("published inference route must be at its initial revision".into());
        }
        self.route.gateway_projection().map(|_| ())
    }
}

/// Idempotent revision of one Inference route catalog head.
#[derive(Debug, Clone)]
pub struct ReviseInferenceRouteWrite {
    pub route: InferenceRoute,
    pub expected_aggregate_version: u64,
    pub idempotency: IdempotencyRequest,
}

impl ReviseInferenceRouteWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.idempotency.validate()?;
        if self.route.retired_at().is_some() {
            return Err("revised inference route write must not be retired".into());
        }
        if self.route.policy_revision() < 2 {
            return Err("revised inference route must advance policy_revision".into());
        }
        if self.route.aggregate_version() != self.expected_aggregate_version.saturating_add(1) {
            return Err("revised inference route aggregate version is inconsistent".into());
        }
        self.route.gateway_projection().map(|_| ())
    }
}

/// Idempotent retirement of one Inference route catalog head.
#[derive(Debug, Clone)]
pub struct RetireInferenceRouteWrite {
    pub route: InferenceRoute,
    pub expected_aggregate_version: u64,
    pub idempotency: IdempotencyRequest,
}

impl RetireInferenceRouteWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.idempotency.validate()?;
        if self.route.retired_at().is_none() {
            return Err("retired inference route write requires a retired route head".into());
        }
        let version = self.route.aggregate_version();
        if version != self.expected_aggregate_version
            && version != self.expected_aggregate_version.saturating_add(1)
        {
            return Err("retired inference route aggregate version is inconsistent".into());
        }
        Ok(())
    }

    pub fn is_noop(&self) -> bool {
        self.route.aggregate_version() == self.expected_aggregate_version
    }
}

/// Stable idempotency response reference for Inference route writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceRouteWriteReference {
    pub organization_id: OrganizationId,
    pub route_id: InferenceRouteId,
    pub aggregate_version: u64,
}

impl InferenceRouteWriteReference {
    pub fn from_route(route: &InferenceRoute) -> Self {
        Self {
            organization_id: route.organization_id,
            route_id: route.id,
            aggregate_version: route.aggregate_version(),
        }
    }
}

#[async_trait]
pub trait IInferenceRouteRepository: Send + Sync {
    async fn find_inference_route(
        &self,
        organization_id: OrganizationId,
        route_id: InferenceRouteId,
    ) -> Result<Option<InferenceRoute>, RepositoryError>;

    /// List Inference route catalog heads for one environment.
    ///
    /// Returns **non-retired** routes only, sorted by `route_id` ascending.
    /// Retired heads remain readable via [`Self::find_inference_route`] so
    /// clients can inspect retirement. Optional `includeRetired` is deferred.
    async fn list_inference_routes_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceRoute>, RepositoryError>;

    async fn replay_inference_route_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<InferenceRoute>>, RepositoryError>;

    async fn publish_inference_route(
        &self,
        write: PublishInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError>;

    async fn revise_inference_route(
        &self,
        write: ReviseInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError>;

    async fn retire_inference_route(
        &self,
        write: RetireInferenceRouteWrite,
    ) -> Result<IdempotentWrite<InferenceRoute>, RepositoryError>;
}

pub const INFERENCE_ROUTE_REPOSITORY: &str = "INFERENCE_ROUTE_REPOSITORY";
