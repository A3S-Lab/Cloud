use super::arguments::{
    self, BuildRunArguments, BuildRunListArguments, BuildRunLogArguments, DeploymentArguments,
    EmptyArguments, EnvironmentScopeArguments, NodeArguments, OperationListArguments,
    RouteArguments, WorkloadArguments, WorkloadLogArguments,
};
use super::artifacts::BuildRunMutationArguments;
use super::catalog::ManagementTool;
use super::forms::{
    CreateFormDraftArguments, FormDraftArguments, FormReleaseArguments, ListFormDraftsArguments,
    PublishFormReleaseArguments, ReviseFormDraftArguments,
};
use super::identity::{
    ChangeMembershipRoleArguments, CreateServiceMembershipArguments, MembershipArguments,
    RevokeMembershipArguments,
};
use super::ontology::{
    CreateOntologyArguments, ListOntologiesArguments, OntologyArguments, OntologyDiffArguments,
    OntologyRevisionArguments, ReviseOntologyArguments,
};
use super::projects::{CreateEnvironmentArguments, CreateProjectArguments, ProjectArguments};
use super::search::SearchArguments;
use super::workflow::{
    CreateWorkflowDefinitionArguments, CreateWorkflowGoalArguments, ListProjectWorkflowArguments,
    ReviseWorkflowDefinitionArguments, WorkflowDefinitionArguments, WorkflowGoalArguments,
    WorkflowPlanRevisionArguments, WorkflowRevisionArguments,
};
use super::workloads::{
    CancelDeploymentArguments, RollbackWorkloadArguments, StopWorkloadArguments,
};
use super::{
    artifacts, edge, forms, identity, nodes, ontology, operations, projects, search, workflow,
    workloads,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(super) struct ManagementExecutionContext {
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    actor_is_platform_admin: bool,
    request_id: Uuid,
}

impl ManagementExecutionContext {
    pub(super) const fn new(
        organization_id: OrganizationId,
        actor_principal_id: PrincipalId,
        actor_is_platform_admin: bool,
        request_id: Uuid,
    ) -> Self {
        Self {
            organization_id,
            actor_principal_id,
            actor_is_platform_admin,
            request_id,
        }
    }
}

pub async fn execute(
    tool: ManagementTool,
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
    context: ManagementExecutionContext,
    arguments: Value,
) -> Option<Result<Value>> {
    let ManagementExecutionContext {
        organization_id,
        actor_principal_id,
        actor_is_platform_admin,
        request_id,
    } = context;
    let result = match tool {
        ManagementTool::EnvironmentsCreate => {
            let arguments = arguments::parse::<CreateEnvironmentArguments>(arguments).ok()?;
            projects::create_environment(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::EnvironmentsList => {
            let arguments = arguments::parse::<ProjectArguments>(arguments).ok()?;
            projects::list_environments(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::MembershipsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            identity::list_memberships(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::MembershipsGet => {
            let arguments = arguments::parse::<MembershipArguments>(arguments).ok()?;
            identity::get_membership(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ServiceMembershipsCreate => {
            let arguments = arguments::parse::<CreateServiceMembershipArguments>(arguments).ok()?;
            identity::create_service_membership(
                command_bus,
                organization_id,
                actor_principal_id,
                actor_is_platform_admin,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipsChangeRole => {
            let arguments = arguments::parse::<ChangeMembershipRoleArguments>(arguments).ok()?;
            identity::change_membership_role(
                command_bus,
                organization_id,
                actor_principal_id,
                actor_is_platform_admin,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::MembershipsRevoke => {
            let arguments = arguments::parse::<RevokeMembershipArguments>(arguments).ok()?;
            identity::revoke_membership(
                command_bus,
                organization_id,
                actor_principal_id,
                actor_is_platform_admin,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::ProjectsCreate => {
            let arguments = arguments::parse::<CreateProjectArguments>(arguments).ok()?;
            projects::create_project(command_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::ProjectsList => {
            let arguments = arguments::parse::<EmptyArguments>(arguments).ok()?;
            projects::list_projects(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::FormsCreate => {
            let arguments = arguments::parse::<CreateFormDraftArguments>(arguments).ok()?;
            forms::create_draft(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::FormsRevise => {
            let arguments = arguments::parse::<ReviseFormDraftArguments>(arguments).ok()?;
            forms::revise_draft(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::FormsList => {
            let arguments = arguments::parse::<ListFormDraftsArguments>(arguments).ok()?;
            forms::list_drafts(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::FormsGet => {
            let arguments = arguments::parse::<FormDraftArguments>(arguments).ok()?;
            forms::get_draft(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::FormReleasesList => {
            let arguments = arguments::parse::<FormDraftArguments>(arguments).ok()?;
            forms::list_releases(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::FormReleasesGet => {
            let arguments = arguments::parse::<FormReleaseArguments>(arguments).ok()?;
            forms::get_release(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::FormReleasesPublish => {
            let arguments = arguments::parse::<PublishFormReleaseArguments>(arguments).ok()?;
            forms::publish_release(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::OntologiesCreate => {
            let arguments = arguments::parse::<CreateOntologyArguments>(arguments).ok()?;
            ontology::create_ontology(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::OntologiesRevise => {
            let arguments = arguments::parse::<ReviseOntologyArguments>(arguments).ok()?;
            ontology::revise_ontology(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::OntologiesList => {
            let arguments = arguments::parse::<ListOntologiesArguments>(arguments).ok()?;
            ontology::list_ontologies(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OntologiesGet => {
            let arguments = arguments::parse::<OntologyArguments>(arguments).ok()?;
            ontology::get_ontology(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OntologyRevisionsList => {
            let arguments = arguments::parse::<OntologyArguments>(arguments).ok()?;
            ontology::list_revisions(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OntologyRevisionsGet => {
            let arguments = arguments::parse::<OntologyRevisionArguments>(arguments).ok()?;
            ontology::get_revision(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::OntologyRevisionsDiff => {
            let arguments = arguments::parse::<OntologyDiffArguments>(arguments).ok()?;
            ontology::diff_revisions(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowDefinitionsCreate => {
            let arguments =
                arguments::parse::<CreateWorkflowDefinitionArguments>(arguments).ok()?;
            workflow::create_definition(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowDefinitionsRevise => {
            let arguments =
                arguments::parse::<ReviseWorkflowDefinitionArguments>(arguments).ok()?;
            workflow::revise_definition(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowDefinitionsList => {
            let arguments = arguments::parse::<ListProjectWorkflowArguments>(arguments).ok()?;
            workflow::list_definitions(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowDefinitionsGet => {
            let arguments = arguments::parse::<WorkflowDefinitionArguments>(arguments).ok()?;
            workflow::get_definition(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowRevisionsList => {
            let arguments = arguments::parse::<WorkflowDefinitionArguments>(arguments).ok()?;
            workflow::list_revisions(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowRevisionsGet => {
            let arguments = arguments::parse::<WorkflowRevisionArguments>(arguments).ok()?;
            workflow::get_revision(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowGoalsCreate => {
            let arguments = arguments::parse::<CreateWorkflowGoalArguments>(arguments).ok()?;
            workflow::create_goal(
                command_bus,
                organization_id,
                actor_principal_id,
                arguments,
                request_id,
            )
            .await
        }
        ManagementTool::WorkflowGoalsList => {
            let arguments = arguments::parse::<ListProjectWorkflowArguments>(arguments).ok()?;
            workflow::list_goals(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowGoalsGet => {
            let arguments = arguments::parse::<WorkflowGoalArguments>(arguments).ok()?;
            workflow::get_goal(query_bus, organization_id, arguments, request_id).await
        }
        ManagementTool::WorkflowPlanRevisionsGet => {
            let arguments = arguments::parse::<WorkflowPlanRevisionArguments>(arguments).ok()?;
            workflow::get_plan_revision(query_bus, organization_id, arguments, request_id).await
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
