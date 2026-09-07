use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OperationId, OrganizationId, PluginAssignmentId, PluginPlanProjectionId,
};
use a3s_boot::Command;
use a3s_use_core::PluginOperationPlanEnvelope;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct RecordPluginPlanProjection {
    pub organization_id: OrganizationId,
    pub projection_id: PluginPlanProjectionId,
    pub assignment_id: PluginAssignmentId,
    pub operation_id: OperationId,
    pub envelope: PluginOperationPlanEnvelope,
    pub recorded_at: DateTime<Utc>,
}

impl Command for RecordPluginPlanProjection {
    type Output = ApplicationResult<PluginPlanProjection>;
}
