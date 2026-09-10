use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{InferenceRouteId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Retire one Inference route so it is no longer projected to Gateway ACL.
#[derive(Debug, Clone)]
pub struct RetireInferenceRoute {
    pub organization_id: OrganizationId,
    pub route_id: InferenceRouteId,
    pub expected_aggregate_version: u64,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RetireInferenceRoute {
    type Output = ApplicationResult<InferenceRoute>;
}
