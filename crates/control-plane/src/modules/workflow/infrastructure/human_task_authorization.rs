use crate::modules::identity::domain::repositories::IResourceAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::ResourceAuthorizationDecisionRequest;
use crate::modules::identity::domain::value_objects::{ApiTokenScope, ResourceGrantScope};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::AuthorizationDecisionRef;
use crate::modules::workflow::application::{
    HumanTaskSubmissionAuthorization, IHumanTaskAuthorizationPort,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct IdentityHumanTaskAuthorizationAdapter {
    decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
}

impl IdentityHumanTaskAuthorizationAdapter {
    pub fn new(decisions: Arc<dyn IResourceAuthorizationDecisionRepository>) -> Self {
        Self { decisions }
    }
}

#[async_trait]
impl IHumanTaskAuthorizationPort for IdentityHumanTaskAuthorizationAdapter {
    async fn authorize_submission(
        &self,
        request: HumanTaskSubmissionAuthorization,
    ) -> ApplicationResult<AuthorizationDecisionRef> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let required_scope = ApiTokenScope::parse(ApiTokenScope::WORKFLOW_WRITE)
            .map_err(ApplicationError::Internal)?;
        self.decisions
            .authorize_resource(ResourceAuthorizationDecisionRequest {
                organization_id: request.organization_id,
                principal_id: request.principal_id,
                credential_id: request.credential_id,
                required_scope,
                action: "workflow.human-task.submit".into(),
                resource: ResourceGrantScope::Project {
                    project_id: request.project_id,
                },
                request_id: request.request_id,
            })
            .await
            .map_err(ApplicationError::from)
    }
}
