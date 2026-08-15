use super::{ConnectorExecutionEvidence, ConnectorExecutionEvidenceCursor};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError,
};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait IConnectorExecutionEvidenceRepository: Send + Sync {
    /// Evidence writes are owned exclusively by the attempt repository's
    /// atomic `settle` transition. This port is intentionally read-only.
    #[allow(clippy::too_many_arguments)]
    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionEvidence>, RepositoryError>;

    /// Returns a bounded keyset page ordered by completion time and attempt ID descending.
    #[allow(clippy::too_many_arguments)]
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionEvidenceCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionEvidence>, RepositoryError>;
}
