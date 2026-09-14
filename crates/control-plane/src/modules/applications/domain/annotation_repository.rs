use super::ApplicationAnnotation;
use crate::modules::shared_kernel::domain::{
    ApplicationAnnotationId, ApplicationId, ApplicationSessionId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationAnnotationRepository: Send + Sync {
    async fn create_annotation(
        &self,
        annotation: ApplicationAnnotation,
    ) -> Result<IdempotentWrite<ApplicationAnnotation>, RepositoryError>;

    async fn find_annotation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        annotation_id: ApplicationAnnotationId,
    ) -> Result<Option<ApplicationAnnotation>, RepositoryError>;

    async fn list_annotations_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationAnnotation>, RepositoryError>;
}
