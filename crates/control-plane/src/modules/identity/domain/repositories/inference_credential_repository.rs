use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
pub trait IInferenceCredentialRepository: Send + Sync {
    async fn create_inference_credential(
        &self,
        credential: InferenceCredential,
    ) -> Result<InferenceCredential, RepositoryError>;

    async fn update_inference_credential(
        &self,
        credential: InferenceCredential,
        expected_aggregate_version: u64,
    ) -> Result<InferenceCredential, RepositoryError>;

    async fn find_inference_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: InferenceCredentialId,
    ) -> Result<Option<InferenceCredential>, RepositoryError>;

    async fn list_inference_credentials_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceCredential>, RepositoryError>;
}
