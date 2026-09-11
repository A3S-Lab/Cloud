use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, AuthorizationDecisionRef, EnvironmentId, OrganizationId, PrincipalId, ProjectId,
};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentApprovalAuthorization {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl AgentApprovalAuthorization {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.credential_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err("Agent approval authorization request is invalid".into());
        }
        Ok(())
    }
}

/// Consumer-owned boundary for exact Identity authorization evidence on agent
/// approval checkpoint decisions. Agents owns the decision action and environment
/// scope vocabulary; Identity remains the sole resource-authorization authority.
#[async_trait]
pub trait IAgentApprovalAuthorizationPort: Send + Sync {
    async fn authorize_decision(
        &self,
        request: AgentApprovalAuthorization,
    ) -> ApplicationResult<AuthorizationDecisionRef>;
}
