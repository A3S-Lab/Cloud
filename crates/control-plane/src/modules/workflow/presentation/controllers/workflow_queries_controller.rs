use super::request::{actor_principal_id, request_id, workflow_access};
use crate::modules::shared_kernel::domain::{
    HumanTaskId, OrganizationId, PlanRevisionId, ProjectId, WorkflowDefinitionId, WorkflowGoalId,
    WorkflowRevisionId, WorkflowRunId,
};
use crate::modules::workflow::presentation::dto::{
    HumanTaskResponse, HumanTaskSummaryResponse, PlanRevisionResponse, WorkflowDefinitionResponse,
    WorkflowGoalResponse, WorkflowNodeCatalogResponse, WorkflowRevisionResponse,
    WorkflowRevisionSummaryResponse, WorkflowRunOutputResponse, WorkflowRunResponse,
    WorkflowRunVariableInspectionResponse,
};
use crate::modules::workflow::{
    GetHumanTask, GetPlanRevision, GetWorkflowDefinition, GetWorkflowGoal, GetWorkflowNodeCatalog,
    GetWorkflowRevision, GetWorkflowRun, GetWorkflowRunDiagnostics, GetWorkflowRunHistory,
    GetWorkflowRunOutput, GetWorkflowRunVariables, HumanTaskStatus, ListHumanTasks,
    ListWorkflowDefinitions, ListWorkflowGoals, ListWorkflowRevisions, ListWorkflowRuns,
    WaitWorkflowRun,
};
use crate::presentation::{DeferredResourceScope, OrganizationTenantGuard, with_deferred_resource_scope, application_error_response};
use a3s_boot::{
    controller, get, use_guard, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub fn workflow_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // Nest macros own project-scoped list/catalog reads; org-scoped resource
    // reads keep deferred project admission (not a Nest attribute today).
    let get_definition_bus = Arc::clone(&bus);
    let list_revisions_bus = Arc::clone(&bus);
    let get_revision_bus = Arc::clone(&bus);
    let get_goal_bus = Arc::clone(&bus);
    let get_plan_bus = Arc::clone(&bus);
    let get_task_bus = Arc::clone(&bus);
    let get_run_bus = Arc::clone(&bus);
    let wait_run_bus = Arc::clone(&bus);
    let output_run_bus = Arc::clone(&bus);
    let variables_run_bus = Arc::clone(&bus);
    let diagnostics_run_bus = Arc::clone(&bus);
    let history_run_bus = Arc::clone(&bus);
    let mut controller = Arc::new(WorkflowQueriesController { bus }).controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-definitions/{workflow_definition_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_definition_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowDefinition {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                                request.param_as::<Uuid>("workflow_definition_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowDefinitionResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_revisions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListWorkflowRevisions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                                request.param_as::<Uuid>("workflow_definition_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(values) => BootResponse::json(
                            &values
                                .into_iter()
                                .map(WorkflowRevisionSummaryResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions/{workflow_revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_revision_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                                request.param_as::<Uuid>("workflow_definition_id")?,
                            ),
                            workflow_revision_id: WorkflowRevisionId::from_uuid(
                                request.param_as::<Uuid>("workflow_revision_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowRevisionResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-goals/{workflow_goal_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_goal_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowGoal {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_goal_id: WorkflowGoalId::from_uuid(
                                request.param_as::<Uuid>("workflow_goal_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowGoalResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-goals/{workflow_goal_id}/plan-revisions/{plan_revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_plan_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPlanRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_goal_id: WorkflowGoalId::from_uuid(
                                request.param_as::<Uuid>("workflow_goal_id")?,
                            ),
                            plan_revision_id: PlanRevisionId::from_uuid(
                                request.param_as::<Uuid>("plan_revision_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&PlanRevisionResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/human-tasks/{human_task_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_task_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetHumanTask {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            human_task_id: HumanTaskId::from_uuid(
                                request.param_as::<Uuid>("human_task_id")?,
                            ),
                            actor_principal_id: actor_principal_id(&request)?,
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&HumanTaskResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-runs/{workflow_run_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_run_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowRun {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_run_id: WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowRunResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-runs/{workflow_run_id}/wait",
            move |request: BootRequest| {
                let bus = Arc::clone(&wait_run_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: WorkflowRunWaitQuery = request.query()?;
                    match bus
                        .execute(WaitWorkflowRun {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_run_id: WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            ),
                            timeout: Duration::from_secs(parameters.timeout_seconds),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowRunResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-runs/{workflow_run_id}/output",
            move |request: BootRequest| {
                let bus = Arc::clone(&output_run_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowRunOutput {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_run_id: WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowRunOutputResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-runs/{workflow_run_id}/variables",
            move |request: BootRequest| {
                let bus = Arc::clone(&variables_run_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowRunVariables {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_run_id: WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(
                            &WorkflowRunVariableInspectionResponse::from(value),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-runs/{workflow_run_id}/diagnostics",
            move |request: BootRequest| {
                let bus = Arc::clone(&diagnostics_run_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowRunDiagnostics {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_run_id: WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            ),
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&value),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/workflow-runs/{workflow_run_id}/history",
            move |request: BootRequest| {
                let bus = Arc::clone(&history_run_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: WorkflowRunHistoryQuery = request.query()?;
                    match bus
                        .execute(GetWorkflowRunHistory {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workflow_run_id: WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            ),
                            after_sequence: parameters.after_sequence,
                            limit: parameters.limit,
                            access: workflow_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&value),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)
}

#[derive(Debug, Clone)]
struct WorkflowQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl WorkflowQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/workflow-node-catalog",
        raw
    )]
    async fn node_catalog(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetWorkflowNodeCatalog {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(value) => BootResponse::json(&WorkflowNodeCatalogResponse::from(value)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/workflow-definitions",
        raw
    )]
    async fn list_definitions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListWorkflowDefinitions {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(values) => BootResponse::json(
                &values
                    .into_iter()
                    .map(WorkflowDefinitionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/projects/{project_id}/workflow-goals", raw)]
    async fn list_goals(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListWorkflowGoals {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(values) => BootResponse::json(
                &values
                    .into_iter()
                    .map(WorkflowGoalResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/projects/{project_id}/workflow-runs", raw)]
    async fn list_runs(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let parameters: WorkflowRunListQuery = request.query()?;
        match self
            .bus
            .execute(ListWorkflowRuns {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access: workflow_access(&request)?,
                limit: parameters.limit,
            })
            .await?
        {
            Ok(values) => BootResponse::json(
                &values
                    .into_iter()
                    .map(WorkflowRunResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/projects/{project_id}/human-tasks", raw)]
    async fn list_human_tasks(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let parameters: HumanTaskListQuery = request.query()?;
        match self
            .bus
            .execute(ListHumanTasks {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                status: parameters.status,
                limit: parameters.limit,
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(values) => BootResponse::json(
                &values
                    .into_iter()
                    .map(HumanTaskSummaryResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowRunListQuery {
    #[serde(default = "default_workflow_run_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HumanTaskListQuery {
    #[serde(default)]
    status: Option<HumanTaskStatus>,
    #[serde(default = "default_human_task_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowRunWaitQuery {
    #[serde(default = "default_workflow_run_wait_seconds")]
    timeout_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowRunHistoryQuery {
    #[serde(default)]
    after_sequence: u64,
    #[serde(default = "default_workflow_run_history_limit")]
    limit: usize,
}

const fn default_workflow_run_limit() -> usize {
    100
}

const fn default_human_task_limit() -> usize {
    100
}

const fn default_workflow_run_wait_seconds() -> u64 {
    30
}

const fn default_workflow_run_history_limit() -> usize {
    100
}

#[cfg(test)]
mod nest_macro_workflow_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn workflow_queries_controller_registers_list_surfaces_via_nest_macros() {
        let controller = workflow_queries_controller(Arc::new(QueryBus::new()))
            .expect("workflow queries nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 17);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/workflow-definitions"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/workflow-runs"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/workflow-runs/{workflow_run_id}"
        }));
    }
}
