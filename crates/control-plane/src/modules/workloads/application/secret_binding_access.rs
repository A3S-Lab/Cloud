use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use async_trait::async_trait;

/// Exact Secrets-owned binding identity required by Workloads admission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadsSecretBindingScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    secret_id: SecretId,
    version: u64,
}

impl WorkloadsSecretBindingScope {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        secret_id: SecretId,
        version: u64,
    ) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            project_id,
            environment_id,
            secret_id,
            version,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
        {
            return Err("Workloads Secret binding scope requires non-nil identities".into());
        }
        Ok(())
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

    pub const fn secret_id(self) -> SecretId {
        self.secret_id
    }

    pub const fn version(self) -> u64 {
        self.version
    }
}

/// Workloads-owned read port for Secrets binding admission.
#[async_trait]
pub trait IWorkloadsSecretBindingAccess: Send + Sync {
    async fn binding_is_admissible(
        &self,
        scope: WorkloadsSecretBindingScope,
    ) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let secret_id = SecretId::new();
        assert!(WorkloadsSecretBindingScope::new(
            organization_id,
            project_id,
            environment_id,
            secret_id,
            1,
        )
        .is_ok());
        assert!(WorkloadsSecretBindingScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            project_id,
            environment_id,
            secret_id,
            1,
        )
        .is_err());
        assert!(WorkloadsSecretBindingScope::new(
            organization_id,
            project_id,
            environment_id,
            SecretId::from_uuid(Uuid::nil()),
            1,
        )
        .is_err());
    }
}
