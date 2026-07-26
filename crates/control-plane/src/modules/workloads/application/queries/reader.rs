use super::{DeploymentQueryResult, WorkloadQueryResult, WorkloadReplicaQueryResult};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::domain::{
    DeploymentId, OrganizationId, RepositoryError, WorkloadId, WorkloadReplicaId,
    WorkloadReplicaMemberId,
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
        let control = self
            .workloads
            .find_workload_control(organization_id, workload.id)
            .await?;
        control
            .validate_against(&workload)
            .map_err(RepositoryError::Storage)?;
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
        let replica_id = WorkloadReplicaId::from_uuid(workload.id.as_uuid());
        let replica = self
            .workloads
            .find_workload_replica(organization_id, workload.id, replica_id)
            .await?;
        let member_id = WorkloadReplicaMemberId::from_uuid(workload.id.as_uuid());
        let member = self
            .workloads
            .find_workload_replica_member(organization_id, replica_id, member_id)
            .await?;
        if replica.organization_id != workload.organization_id
            || replica.project_id != workload.project_id
            || replica.environment_id != workload.environment_id
            || replica.workload_id != workload.id
            || member.organization_id != workload.organization_id
            || member.project_id != workload.project_id
            || member.environment_id != workload.environment_id
            || member.workload_id != workload.id
            || member.replica_id != replica.id
            || !revisions_by_id.contains_key(&replica.revision_id)
        {
            return Err(RepositoryError::Storage(
                "Workload replica projection is inconsistent with its control state".into(),
            ));
        }
        Ok(WorkloadQueryResult {
            workload,
            control,
            replicas: vec![WorkloadReplicaQueryResult {
                replica,
                members: vec![member],
            }],
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
        let replica_binding = self
            .workloads
            .find_deployment_replica_binding(deployment.organization_id, deployment.id)
            .await?;
        if replica_binding.workload_id != deployment.workload_id
            || replica_binding.revision_id != revision.id
            || replica_binding.node_id != deployment.node_id
            || replica_binding.runtime_unit_id != revision.runtime_unit_id()
            || replica_binding.runtime_generation != revision.generation
        {
            return Err(RepositoryError::Storage(
                "deployment replica binding is inconsistent with its Runtime projection".into(),
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
                        &replica_binding.runtime_unit_id,
                        replica_binding.runtime_generation,
                    )
                    .await?
            }
            None => None,
        };
        Ok(DeploymentQueryResult {
            deployment,
            replica_binding,
            revision,
            operation,
            observation,
        })
    }
}
