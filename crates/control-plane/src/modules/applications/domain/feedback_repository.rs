use super::ApplicationFeedback;
use crate::modules::shared_kernel::domain::{
    ApplicationFeedbackId, ApplicationId, ApplicationSessionId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationFeedbackRepository: Send + Sync {
    async fn create_feedback(
        &self,
        feedback: ApplicationFeedback,
    ) -> Result<IdempotentWrite<ApplicationFeedback>, RepositoryError>;

    async fn find_feedback(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        feedback_id: ApplicationFeedbackId,
    ) -> Result<Option<ApplicationFeedback>, RepositoryError>;

    async fn list_feedback_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationFeedback>, RepositoryError>;
}
