use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::value_objects::EdgeRouteBindingRef;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Command;
use a3s_cloud_contracts::{InferenceGrantAclProjection, InferenceModelAclProjection};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Publish one environment-scoped Inference route catalog head.
#[derive(Debug, Clone)]
pub struct PublishInferenceRoute {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub router: String,
    pub models: Vec<InferenceModelAclProjection>,
    pub grants: Vec<InferenceGrantAclProjection>,
    pub binding: EdgeRouteBindingRef,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for PublishInferenceRoute {
    type Output = ApplicationResult<InferenceRoute>;
}
