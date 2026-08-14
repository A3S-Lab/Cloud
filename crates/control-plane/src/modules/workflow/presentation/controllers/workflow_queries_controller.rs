use super::request::{actor_principal_id, request_id, resource_access};
use crate::modules::identity::presentation::{
    with_deferred_resource_scope, DeferredResourceScope, OrganizationTenantGuard,
};
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
    GetWorkflowRevision, GetWorkflowRun, GetWorkflowRunHistory, GetWorkflowRunOutput,
    GetWorkflowRunVariables, HumanTaskStatus, ListHumanTasks, ListWorkflowDefinitions,
    ListWorkflowGoals, ListWorkflowRevisions, ListWorkflowRuns, WaitWorkflowRun,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub fn workflow_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_node_catalog_bus = Arc::clone(&bus);
    let list_definitions_bus = Arc::clone(&bus);
    let get_definition_bus = Arc::clone(&bus);
    let list_revisions_bus = Arc::clone(&bus);
    let get_revision_bus = Arc::clone(&bus);
    let list_goals_bus = Arc::clone(&bus);
    let get_goal_bus = Arc::clone(&bus);
    let get_plan_bus = Arc::clone(&bus);
    let list_runs_bus = Arc::clone(&bus);
    let list_tasks_bus = Arc::clone(&bus);
    let get_task_bus = Arc::clone(&bus);
    let get_run_bus = Arc::clone(&bus);
    let wait_run_bus = Arc::clone(&bus);
    let output_run_bus = Arc::clone(&bus);
    let variables_run_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/workflow-node-catalog",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_node_catalog_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkflowNodeCatalog {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            resource_access: resource_access(&request)?,
                        })
                        .await?
                    {
                        Ok(value) => {
                            BootResponse::json(&WorkflowNodeCatalogResponse::from(value))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/workflow-definitions",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_definitions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListWorkflowDefinitions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
                            })
                            .await?
                        {
                            Ok(value) => {
                                BootResponse::json(&WorkflowDefinitionResponse::from(value))
                            }
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .get(
            "/{organization_id}/projects/{project_id}/workflow-goals",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_goals_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListWorkflowGoals {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .get(
            "/{organization_id}/projects/{project_id}/workflow-runs",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_runs_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: WorkflowRunListQuery = request.query()?;
                    match bus
                        .execute(ListWorkflowRuns {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/human-tasks",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_tasks_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let parameters: HumanTaskListQuery = request.query()?;
                    match bus
                        .execute(ListHumanTasks {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            status: parameters.status,
                            limit: parameters.limit,
                            resource_access: resource_access(&request)?,
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
            },
        )?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
                            })
                            .await?
                        {
                            Ok(value) => {
                                BootResponse::json(&WorkflowRunOutputResponse::from(value))
                            }
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .route(with_deferred_resource_scope(
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
                                resource_access: resource_access(&request)?,
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
        )?)?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/workflow-runs/{workflow_run_id}/history",
                move |request: BootRequest| {
                    let bus = Arc::clone(&bus);
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
                                resource_access: resource_access(&request)?,
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
