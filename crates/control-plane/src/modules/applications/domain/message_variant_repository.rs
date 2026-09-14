use super::ApplicationMessageVariant;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageVariantId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationMessageVariantRepository: Send + Sync {
    async fn create_message_variant(
        &self,
        variant: ApplicationMessageVariant,
    ) -> Result<IdempotentWrite<ApplicationMessageVariant>, RepositoryError>;

    async fn find_message_variant(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        variant_id: ApplicationMessageVariantId,
    ) -> Result<Option<ApplicationMessageVariant>, RepositoryError>;

    async fn list_message_variants_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageVariant>, RepositoryError>;
}
