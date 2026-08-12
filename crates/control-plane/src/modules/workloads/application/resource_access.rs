use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, OrganizationId, RepositoryError, WorkloadId,
};
use crate::modules::workloads::domain::entities::{Deployment, Workload};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use std::sync::Arc;

/// Resolves indirect Workloads identifiers through the owning repository before authorization.
///
/// Identity owns grant semantics; Workloads owns resource-to-scope resolution. Keeping that split
/// avoids a second resource ownership registry and makes missing and denied resources
/// indistinguishable at the application boundary.
#[derive(Clone)]
pub(crate) struct WorkloadResourceAccess {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl WorkloadResourceAccess {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }

    pub async fn workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<Workload> {
        let workload = self
            .workloads
            .find_workload(organization_id, workload_id)
            .await
            .map_err(|error| map_repository_error(error, "workload not found"))?;
        if !evaluator.allows(workload_scope(&workload)) {
            return Err(ApplicationError::NotFound("workload not found".into()));
        }
        Ok(workload)
    }

    pub async fn deployment(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<Deployment> {
        let deployment = self
            .workloads
            .find_deployment(organization_id, deployment_id)
            .await
            .map_err(|error| map_repository_error(error, "deployment not found"))?;
        let workload = self
            .workloads
            .find_workload(organization_id, deployment.workload_id)
            .await
            .map_err(|error| map_repository_error(error, "deployment not found"))?;
        if !evaluator.allows(workload_scope(&workload)) {
            return Err(ApplicationError::NotFound("deployment not found".into()));
        }
        Ok(deployment)
    }
}

fn workload_scope(workload: &Workload) -> ResourceGrantScope {
    ResourceGrantScope::Environment {
        project_id: workload.project_id,
        environment_id: workload.environment_id,
    }
}

fn map_repository_error(error: RepositoryError, not_found: &'static str) -> ApplicationError {
    match error {
        RepositoryError::NotFound => ApplicationError::NotFound(not_found.into()),
        error => error.into(),
    }
}
