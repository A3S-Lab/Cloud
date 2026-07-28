use super::arguments::{
    self, BuildRunArguments, BuildRunListArguments, BuildRunLogArguments, DeploymentArguments,
    EmptyArguments, EnvironmentScopeArguments, NodeArguments, OperationListArguments,
    RouteArguments, WorkloadArguments, WorkloadLogArguments,
};
use super::artifacts::BuildRunMutationArguments;
use super::catalog::ManagementTool;
use super::projects::{CreateEnvironmentArguments, CreateProjectArguments, ProjectArguments};
use super::search::SearchArguments;
use super::workloads::{
    CancelDeploymentArguments, RollbackWorkloadArguments, StopWorkloadArguments,
};
use super::{artifacts, edge, nodes, operations, projects, search, workloads};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CommandBus, QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn execute(
    tool: ManagementTool,
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: Value,
    request_id: Uuid,
) -> Option<Result<Value>> {
    let result = match tool {
        ManagementTool::EnvironmentsCreate => {
            let arguments = arguments::parse::<CreateEnvironmentArguments>(arguments).ok()?;
            projects::create_environment(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::EnvironmentsList => {
            let arguments = arguments::parse::<ProjectArguments>(arguments).ok()?;
            projects::list_environments(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ProjectsCreate => {
            let arguments = arguments::parse::<CreateProjectArguments>(arguments).ok()?;
            projects::create_project(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ProjectsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            projects::list_projects(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::Search => {
            let arguments = arguments::parse::<SearchArguments>(arguments).ok()?;
            search::search(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::NodesList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            nodes::list_nodes(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::NodesGet => {
            let arguments = arguments::parse::<NodeArguments>(arguments).ok()?;
            nodes::get_node(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OperationsList => {
            let arguments = arguments::parse::<OperationListArguments>(arguments).ok()?;
            operations::list_operations(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkloadsList => {
            let arguments = arguments::parse::<EnvironmentScopeArguments>(arguments).ok()?;
            workloads::list_workloads(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkloadsGet => {
            let arguments = arguments::parse::<WorkloadArguments>(arguments).ok()?;
            workloads::get_workload(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkloadLogsGet => {
            let arguments = arguments::parse::<WorkloadLogArguments>(arguments).ok()?;
            workloads::get_workload_logs(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkloadsStop => {
            let arguments = arguments::parse::<StopWorkloadArguments>(arguments).ok()?;
            workloads::stop_workload(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkloadsRollback => {
            let arguments = arguments::parse::<RollbackWorkloadArguments>(arguments).ok()?;
            workloads::rollback_workload(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::DeploymentsGet => {
            let arguments = arguments::parse::<DeploymentArguments>(arguments).ok()?;
            workloads::get_deployment(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::DeploymentsCancel => {
            let arguments = arguments::parse::<CancelDeploymentArguments>(arguments).ok()?;
            workloads::cancel_deployment(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::RoutesList => {
            let arguments = arguments::parse::<EnvironmentScopeArguments>(arguments).ok()?;
            edge::list_routes(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::RoutesGet => {
            let arguments = arguments::parse::<RouteArguments>(arguments).ok()?;
            edge::get_route(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildRunsList => {
            let arguments = arguments::parse::<BuildRunListArguments>(arguments).ok()?;
            artifacts::list_build_runs(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildRunsGet => {
            let arguments = arguments::parse::<BuildRunArguments>(arguments).ok()?;
            artifacts::get_build_run(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildRunLogsGet => {
            let arguments = arguments::parse::<BuildRunLogArguments>(arguments).ok()?;
            artifacts::get_build_run_logs(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildEvidenceGet => {
            let arguments = arguments::parse::<BuildRunArguments>(arguments).ok()?;
            artifacts::get_build_evidence(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildRunsCancel => {
            let arguments = arguments::parse::<BuildRunMutationArguments>(arguments).ok()?;
            artifacts::cancel_build_run(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::BuildRunsRetry => {
            let arguments = arguments::parse::<BuildRunMutationArguments>(arguments).ok()?;
            artifacts::retry_build_run(command_bus, organization_id, arguments, request_id).await
        }
    };
    Some(result)
}
