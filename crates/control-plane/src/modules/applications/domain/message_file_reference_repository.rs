use super::ApplicationMessageFileReference;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageFileReferenceId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationMessageFileReferenceRepository: Send + Sync {
    async fn create_message_file_reference(
        &self,
        reference: ApplicationMessageFileReference,
    ) -> Result<IdempotentWrite<ApplicationMessageFileReference>, RepositoryError>;

    async fn find_message_file_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        reference_id: ApplicationMessageFileReferenceId,
    ) -> Result<Option<ApplicationMessageFileReference>, RepositoryError>;

    async fn list_message_file_references_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageFileReference>, RepositoryError>;
}
