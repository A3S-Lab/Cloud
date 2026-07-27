use super::arguments::{
    DeploymentArguments, EnvironmentScopeArguments, WorkloadArguments, WorkloadLogArguments,
};
use super::tool_result;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::presentation::{
    DeploymentResponse, WorkloadLogsResponse, WorkloadResponse,
};
use crate::modules::workloads::{GetDeployment, GetWorkload, GetWorkloadLogs, ListWorkloads};
use a3s_boot::{QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn list_workloads(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: EnvironmentScopeArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListWorkloads {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
        })
        .await?
    {
        Ok(workloads) => tool_result::success(
            200,
            workloads
                .into_iter()
                .map(WorkloadResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_workload(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkloadArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkload {
            organization_id,
            workload_id: WorkloadId::from_uuid(arguments.workload_id),
        })
        .await?
    {
        Ok(workload) => tool_result::success(200, WorkloadResponse::from(workload), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_deployment(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: DeploymentArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetDeployment {
            organization_id,
            deployment_id: DeploymentId::from_uuid(arguments.deployment_id),
        })
        .await?
    {
        Ok(deployment) => {
            tool_result::success(200, DeploymentResponse::from(deployment), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_workload_logs(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkloadLogArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkloadLogs {
            organization_id,
            workload_id: WorkloadId::from_uuid(arguments.workload_id),
            revision_id: WorkloadRevisionId::from_uuid(arguments.revision_id),
            after_sequence: arguments.after_sequence,
            limit: arguments.limit,
            stream: arguments.stream.map(Into::into),
        })
        .await?
    {
        Ok(logs) => tool_result::success(200, WorkloadLogsResponse::from(logs), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
