use crate::modules::fleet::domain::repositories::INodePoolRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    NodePoolId, OrganizationId, RepositoryError, WorkloadId,
};
use crate::modules::workloads::domain::entities::WorkloadControlSpec;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;

pub(in crate::modules::workloads::application) async fn validate_node_pool_selection(
    node_pools: &dyn INodePoolRepository,
    organization_id: OrganizationId,
    node_pool_id: Option<NodePoolId>,
) -> ApplicationResult<()> {
    let Some(node_pool_id) = node_pool_id else {
        return Ok(());
    };
    match node_pools.find(organization_id, node_pool_id).await {
        Ok(pool) if pool.organization_id == organization_id && pool.id == node_pool_id => Ok(()),
        Ok(_) | Err(RepositoryError::NotFound) => {
            Err(ApplicationError::NotFound("node pool not found".into()))
        }
        Err(error) => Err(error.into()),
    }
}

pub(in crate::modules::workloads::application) async fn load_direct_workload_control(
    workloads: &dyn IWorkloadRepository,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> ApplicationResult<WorkloadControlSpec> {
    let control = match workloads
        .find_workload_control(organization_id, workload_id)
        .await
    {
        Ok(control)
            if control.organization_id == organization_id && control.workload_id == workload_id =>
        {
            control
        }
        Ok(_) | Err(RepositoryError::NotFound) => {
            return Err(ApplicationError::Internal(
                "workload durable control is unavailable".into(),
            ))
        }
        Err(error) => return Err(error.into()),
    };
    control
        .require_direct_mutation()
        .map_err(ApplicationError::Conflict)?;
    Ok(control.spec)
}

pub(in crate::modules::workloads::application) fn require_acl_node_pool_selection(
    control: &WorkloadControlSpec,
    expected_node_pool_id: Option<Option<NodePoolId>>,
) -> ApplicationResult<()> {
    if expected_node_pool_id
        .is_some_and(|expected| control.placement_policy.node_pool_id() != expected)
    {
        return Err(ApplicationError::Conflict(
            "workload ACL placement does not match the immutable target node pool".into(),
        ));
    }
    Ok(())
}
