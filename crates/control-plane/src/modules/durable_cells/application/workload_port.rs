use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, EnvironmentId, OrganizationId, ProjectId,
};
use async_trait::async_trait;

/// Exact identity of the Durable Cell application whose managed Workload
/// projection must be reconciled.
///
/// Workloads remains the authority for replica state, scheduling, Runtime
/// fencing, and retirement. Durable Cells sends only this bounded intent
/// through its consumer-owned port; no Workloads aggregate or repository type
/// crosses the application boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCellWorkloadReconciliationRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
}

impl DurableCellWorkloadReconciliationRequest {
    pub const fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            application_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
        {
            return Err("Durable Cell Workload reconciliation identity is invalid".into());
        }
        Ok(())
    }
}

/// Durable Cells' sole application boundary for converging its deterministic
/// managed Workload projection. The owner adapter performs all aggregate
/// loading, authorization/integrity checks, idempotency, and replica-set
/// mutation while Workloads retains lifecycle authority.
#[async_trait]
pub trait IDurableCellWorkloadPort: Send + Sync {
    async fn converge_managed_replicas(
        &self,
        request: &DurableCellWorkloadReconciliationRequest,
    ) -> ApplicationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn reconciliation_request_requires_complete_scope() {
        let request = DurableCellWorkloadReconciliationRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            DurableCellApplicationId::new(),
        );
        request.validate().expect("valid reconciliation request");
    }

    #[test]
    fn reconciliation_request_rejects_nil_identity() {
        let request = DurableCellWorkloadReconciliationRequest::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            DurableCellApplicationId::new(),
        );
        let mut invalid = request;
        invalid.application_id = DurableCellApplicationId::from_uuid(Uuid::nil());
        assert!(invalid.validate().is_err());
    }
}
