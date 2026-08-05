use crate::modules::agents::domain::{AgentCodeRunBinding, AgentCodeRunWrite};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::Command;

#[derive(Debug, Clone)]
pub struct BindAgentCodeRun {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub binding: AgentCodeRunBinding,
}

impl Command for BindAgentCodeRun {
    type Output = ApplicationResult<AgentCodeRunWrite>;
}
