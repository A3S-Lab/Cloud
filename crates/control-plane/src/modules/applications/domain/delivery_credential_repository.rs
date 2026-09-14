use super::ApplicationDeliveryCredential;
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IApplicationDeliveryCredentialRepository: Send + Sync {
    async fn create_delivery_credential(
        &self,
        credential: ApplicationDeliveryCredential,
    ) -> Result<ApplicationDeliveryCredential, RepositoryError>;

    async fn update_delivery_credential(
        &self,
        credential: ApplicationDeliveryCredential,
        expected_generation: u64,
    ) -> Result<ApplicationDeliveryCredential, RepositoryError>;

    async fn find_delivery_credential(
        &self,
        organization_id: OrganizationId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
    ) -> Result<Option<ApplicationDeliveryCredential>, RepositoryError>;

    async fn find_delivery_credential_by_lookup_key(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        lookup_key: &str,
    ) -> Result<Option<ApplicationDeliveryCredential>, RepositoryError>;

    async fn list_delivery_credentials_by_application(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> Result<Vec<ApplicationDeliveryCredential>, RepositoryError>;
}
