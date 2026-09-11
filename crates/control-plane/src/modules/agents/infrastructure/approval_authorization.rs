use crate::modules::agents::application::{
    AgentApprovalAuthorization, IAgentApprovalAuthorizationPort,
};
use crate::modules::identity::domain::repositories::IResourceAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::ResourceAuthorizationDecisionRequest;
use crate::modules::identity::domain::value_objects::{ApiTokenScope, ResourceGrantScope};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::AuthorizationDecisionRef;
use async_trait::async_trait;
use std::sync::Arc;

pub struct IdentityAgentApprovalAuthorizationAdapter {
    decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
}

impl IdentityAgentApprovalAuthorizationAdapter {
    pub fn new(decisions: Arc<dyn IResourceAuthorizationDecisionRepository>) -> Self {
        Self { decisions }
    }
}

#[async_trait]
impl IAgentApprovalAuthorizationPort for IdentityAgentApprovalAuthorizationAdapter {
    async fn authorize_decision(
        &self,
        request: AgentApprovalAuthorization,
    ) -> ApplicationResult<AuthorizationDecisionRef> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let required_scope = ApiTokenScope::parse(ApiTokenScope::EXECUTION_WRITE)
            .map_err(ApplicationError::Internal)?;
        self.decisions
            .authorize_resource(ResourceAuthorizationDecisionRequest {
                organization_id: request.organization_id,
                principal_id: request.principal_id,
                credential_id: request.credential_id,
                required_scope,
                action: "agent.execution.approval.decide".into(),
                resource: ResourceGrantScope::Environment {
                    project_id: request.project_id,
                    environment_id: request.environment_id,
                },
                request_id: request.request_id,
            })
            .await
            .map_err(ApplicationError::from)
    }
}
