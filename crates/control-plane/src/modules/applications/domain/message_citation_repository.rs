use super::ApplicationMessageCitation;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageCitationId, ApplicationSessionId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationMessageCitationRepository: Send + Sync {
    async fn create_message_citation(
        &self,
        citation: ApplicationMessageCitation,
    ) -> Result<IdempotentWrite<ApplicationMessageCitation>, RepositoryError>;

    async fn find_message_citation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        citation_id: ApplicationMessageCitationId,
    ) -> Result<Option<ApplicationMessageCitation>, RepositoryError>;

    async fn list_message_citations_by_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Vec<ApplicationMessageCitation>, RepositoryError>;
}

