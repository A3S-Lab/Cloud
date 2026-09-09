//! Narrow Identity owner query for Edge Gateway snapshot ACL projection.
//!
//! Consumers receive only contract-level ACL projections. Prefix secrets,
//! repository entities, and Identity persistence stay private to Identity.

use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::InferenceCredentialAclProjection;
use async_trait::async_trait;

/// Exact Identity-owned environment scope used to load inference credentials
/// for one managed Gateway snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferenceCredentialEnvironmentScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

impl InferenceCredentialEnvironmentScope {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || environment_id.as_uuid().is_nil()
        {
            return Err("inference credential environment scope requires non-nil identities".into());
        }
        Ok(Self {
            organization_id,
            project_id,
            environment_id,
        })
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(self) -> EnvironmentId {
        self.environment_id
    }
}

/// Identity-owned projection port for managed Gateway inference ACL.
#[async_trait]
pub trait IInferenceCredentialAclProjectionPort: Send + Sync {
    async fn list_inference_credential_acl_projections(
        &self,
        scopes: &[InferenceCredentialEnvironmentScope],
    ) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_identities() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        assert!(InferenceCredentialEnvironmentScope::new(
            organization_id,
            project_id,
            environment_id
        )
        .is_ok());
        assert!(InferenceCredentialEnvironmentScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            project_id,
            environment_id,
        )
        .is_err());
    }
}
