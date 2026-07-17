use super::{DeploymentQueryResult, WorkloadQueryResult};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::domain::{
    DeploymentId, OrganizationId, RepositoryError, WorkloadId,
};
use crate::modules::workloads::domain::entities::Workload;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct WorkloadQueryReader {
    workloads: Arc<dyn IWorkloadRepository>,
    operations: Arc<dyn IOperationRepository>,
    node_control: Arc<dyn INodeControlRepository>,
}

impl WorkloadQueryReader {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        operations: Arc<dyn IOperationRepository>,
        node_control: Arc<dyn INodeControlRepository>,
    ) -> Self {
        Self {
            workloads,
            operations,
            node_control,
        }
    }

    pub async fn workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<WorkloadQueryResult, RepositoryError> {
        let workload = self
            .workloads
            .find_workload(organization_id, workload_id)
            .await?;
        self.view(organization_id, workload).await
    }

    pub async fn deployment(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<DeploymentQueryResult, RepositoryError> {
        let deployment = self
            .workloads
            .find_deployment(organization_id, deployment_id)
            .await?;
        let revision = self
            .workloads
            .find_revision(organization_id, deployment.revision_id)
            .await?;
        self.deployment_view(deployment, revision).await
    }

    pub async fn view(
        &self,
        organization_id: OrganizationId,
        workload: Workload,
    ) -> Result<WorkloadQueryResult, RepositoryError> {
        let revisions = self
            .workloads
            .list_revisions(organization_id, workload.id)
            .await?;
        let revisions_by_id = revisions
            .iter()
            .cloned()
            .map(|revision| (revision.id, revision))
            .collect::<BTreeMap<_, _>>();
        let deployments = self
            .workloads
            .list_deployments(organization_id, workload.id)
            .await?;
        let mut deployment_views = Vec::with_capacity(deployments.len());
        for deployment in deployments {
            let revision = revisions_by_id
                .get(&deployment.revision_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "deployment references a missing workload revision".into(),
                    )
                })?;
            deployment_views.push(self.deployment_view(deployment, revision).await?);
        }
        Ok(WorkloadQueryResult {
            workload,
            revisions,
            deployments: deployment_views,
        })
    }

    async fn deployment_view(
        &self,
        deployment: crate::modules::workloads::domain::entities::Deployment,
        revision: crate::modules::workloads::domain::entities::WorkloadRevision,
    ) -> Result<DeploymentQueryResult, RepositoryError> {
        if deployment.workload_id != revision.workload_id || deployment.revision_id != revision.id {
            return Err(RepositoryError::Storage(
                "deployment and workload revision identities are inconsistent".into(),
            ));
        }
        let operation = self
            .operations
            .find_projection(deployment.operation_id)
            .await?;
        let observation = match deployment.node_id {
            Some(node_id) => {
                self.node_control
                    .latest_runtime_observation(
                        node_id,
                        &revision.runtime_unit_id(),
                        revision.generation,
                    )
                    .await?
            }
            None => None,
        };
        Ok(DeploymentQueryResult {
            deployment,
            revision,
            operation,
            observation,
        })
    }
}
