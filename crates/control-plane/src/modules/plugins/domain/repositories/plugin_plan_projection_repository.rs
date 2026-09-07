use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PluginAssignmentId, PluginPlanProjectionId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;

#[async_trait]
pub trait IPluginPlanProjectionRepository: Send + Sync {
    async fn create(
        &self,
        projection: PluginPlanProjection,
    ) -> Result<PluginPlanProjection, RepositoryError>;

    async fn update(
        &self,
        projection: PluginPlanProjection,
        expected_confirmation_digest: Option<&Sha256Digest>,
    ) -> Result<PluginPlanProjection, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        projection_id: PluginPlanProjectionId,
    ) -> Result<Option<PluginPlanProjection>, RepositoryError>;

    async fn find_by_plan_digest(
        &self,
        organization_id: OrganizationId,
        plan_digest: &Sha256Digest,
    ) -> Result<Option<PluginPlanProjection>, RepositoryError>;

    async fn list_for_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
    ) -> Result<Vec<PluginPlanProjection>, RepositoryError>;
}
