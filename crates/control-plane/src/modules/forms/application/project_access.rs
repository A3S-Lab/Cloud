use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, RepositoryError};
use async_trait::async_trait;

/// Exact Projects ownership key required when creating one Form aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormProjectScope {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
}

/// Forms-owned read port for the Projects authority.
///
/// The port exposes only existence evidence. It deliberately does not leak the
/// Project aggregate or repository vocabulary into Forms application code.
#[async_trait]
pub trait IFormProjectAccess: Send + Sync {
    async fn project_exists(&self, scope: FormProjectScope) -> Result<bool, RepositoryError>;
}
