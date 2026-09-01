use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, SecretId, WorkloadRevisionId,
};
use async_trait::async_trait;

/// Exact authorization question owned by Secrets. Callers cannot supply the
/// project or environment used for materialization; those facts must come from
/// the Workloads owner through the anti-corruption adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretMaterializationAuthorizationRequest {
    organization_id: OrganizationId,
    node_id: NodeId,
    workload_revision_id: WorkloadRevisionId,
    secret_id: SecretId,
    secret_version: u64,
}

impl SecretMaterializationAuthorizationRequest {
    pub fn new(
        organization_id: OrganizationId,
        node_id: NodeId,
        workload_revision_id: WorkloadRevisionId,
        secret_id: SecretId,
        secret_version: u64,
    ) -> Result<Self, String> {
        let request = Self {
            organization_id,
            node_id,
            workload_revision_id,
            secret_id,
            secret_version,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
            || self.secret_version == 0
        {
            return Err("Secret materialization authorization request is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn secret_id(&self) -> SecretId {
        self.secret_id
    }

    pub const fn secret_version(&self) -> u64 {
        self.secret_version
    }
}

/// Secrets-owned proof carrying the exact scope accepted for one
/// materialization. Only an adapter inside Secrets may issue this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretMaterializationAuthorization {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    node_id: NodeId,
    workload_revision_id: WorkloadRevisionId,
    secret_id: SecretId,
    secret_version: u64,
}

impl SecretMaterializationAuthorization {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::modules::secrets) fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
        workload_revision_id: WorkloadRevisionId,
        secret_id: SecretId,
        secret_version: u64,
    ) -> Result<Self, String> {
        let authorization = Self {
            organization_id,
            project_id,
            environment_id,
            node_id,
            workload_revision_id,
            secret_id,
            secret_version,
        };
        authorization.validate()?;
        Ok(authorization)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
            || self.secret_version == 0
        {
            return Err("Secret materialization authorization is invalid".into());
        }
        Ok(())
    }

    pub fn validate_for(
        &self,
        request: &SecretMaterializationAuthorizationRequest,
    ) -> Result<(), String> {
        self.validate()?;
        request.validate()?;
        if self.organization_id != request.organization_id
            || self.node_id != request.node_id
            || self.workload_revision_id != request.workload_revision_id
            || self.secret_id != request.secret_id
            || self.secret_version != request.secret_version
        {
            return Err("Secret materialization authorization scope is inconsistent".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn secret_id(&self) -> SecretId {
        self.secret_id
    }

    pub const fn secret_version(&self) -> u64 {
        self.secret_version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretMaterializationAuthorizationError {
    #[error("Secret material is not authorized for this node")]
    Forbidden,
    #[error("Secret materialization authorization is unavailable: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait ISecretMaterializationAuthorizer: Send + Sync {
    async fn authorize(
        &self,
        request: SecretMaterializationAuthorizationRequest,
    ) -> Result<SecretMaterializationAuthorization, SecretMaterializationAuthorizationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_is_bound_to_the_exact_request() {
        let request = SecretMaterializationAuthorizationRequest::new(
            OrganizationId::new(),
            NodeId::new(),
            WorkloadRevisionId::new(),
            SecretId::new(),
            7,
        )
        .expect("request");
        let authorization = SecretMaterializationAuthorization::new(
            request.organization_id(),
            ProjectId::new(),
            EnvironmentId::new(),
            request.node_id(),
            request.workload_revision_id(),
            request.secret_id(),
            request.secret_version(),
        )
        .expect("authorization");
        assert!(authorization.validate_for(&request).is_ok());

        let drifted = SecretMaterializationAuthorizationRequest::new(
            request.organization_id(),
            NodeId::new(),
            request.workload_revision_id(),
            request.secret_id(),
            request.secret_version(),
        )
        .expect("drifted request");
        assert!(authorization.validate_for(&drifted).is_err());
    }
}
