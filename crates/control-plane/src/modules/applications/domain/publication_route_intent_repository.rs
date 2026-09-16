use super::ApplicationPublicationRouteIntent;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationPublicationRouteIntentRepository: Send + Sync {
    async fn create_intent(
        &self,
        intent: ApplicationPublicationRouteIntent,
    ) -> Result<IdempotentWrite<ApplicationPublicationRouteIntent>, RepositoryError>;

    async fn find_intent(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        intent_id: ApplicationPublicationRouteIntentId,
    ) -> Result<Option<ApplicationPublicationRouteIntent>, RepositoryError>;

    async fn list_intents_by_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        application_release_id: ApplicationReleaseId,
        application_release_digest: &Sha256Digest,
    ) -> Result<Vec<ApplicationPublicationRouteIntent>, RepositoryError>;

    /// List every immutable intent under one org/project (Gateway snapshot compile scope).
    async fn list_intents_by_project(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<ApplicationPublicationRouteIntent>, RepositoryError>;
}