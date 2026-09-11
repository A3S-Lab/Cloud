use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, AuthorizationDecisionRef, OrganizationId, PrincipalId, ProjectId,
};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskSubmissionAuthorization {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl HumanTaskSubmissionAuthorization {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.credential_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err("HumanTask submission authorization request is invalid".into());
        }
        Ok(())
    }
}

/// Consumer-owned boundary for exact Identity authorization evidence on HumanTask submit.
/// Workflow owns the submission action and project scope vocabulary; Identity remains the
/// sole resource-authorization decision authority behind the adapter.
#[async_trait]
pub trait IHumanTaskAuthorizationPort: Send + Sync {
    async fn authorize_submission(
        &self,
        request: HumanTaskSubmissionAuthorization,
    ) -> ApplicationResult<AuthorizationDecisionRef>;
}
