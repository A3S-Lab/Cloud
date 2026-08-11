use crate::modules::forms::domain::FormSubmission;
use crate::modules::shared_kernel::domain::{
    FormSubmissionId, HumanTaskId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IFormSubmissionRepository: Send + Sync {
    async fn find_submission(
        &self,
        organization_id: OrganizationId,
        submission_id: FormSubmissionId,
    ) -> Result<Option<FormSubmission>, RepositoryError>;

    async fn find_task_submission(
        &self,
        organization_id: OrganizationId,
        human_task_id: HumanTaskId,
    ) -> Result<Option<FormSubmission>, RepositoryError>;
}
