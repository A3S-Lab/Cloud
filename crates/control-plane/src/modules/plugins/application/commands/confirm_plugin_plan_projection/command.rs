use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PluginPlanProjectionId};
use a3s_boot::Command;
use a3s_use_core::PluginOperationConfirmation;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct ConfirmPluginPlanProjection {
    pub organization_id: OrganizationId,
    pub projection_id: PluginPlanProjectionId,
    pub confirmation: PluginOperationConfirmation,
    pub confirmed_at: DateTime<Utc>,
}

impl Command for ConfirmPluginPlanProjection {
    type Output = ApplicationResult<PluginPlanProjection>;
}
