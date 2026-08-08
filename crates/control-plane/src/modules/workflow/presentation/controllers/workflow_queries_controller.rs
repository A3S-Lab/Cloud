use super::request::request_id;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PlanRevisionId, ProjectId, WorkflowDefinitionId, WorkflowGoalId,
    WorkflowRevisionId,
};
use crate::modules::workflow::presentation::dto::{
    PlanRevisionResponse, WorkflowDefinitionResponse, WorkflowGoalResponse,
    WorkflowRevisionResponse, WorkflowRevisionSummaryResponse,
};
use crate::modules::workflow::{
    GetPlanRevision, GetWorkflowDefinition, GetWorkflowGoal, GetWorkflowRevision,
    ListWorkflowDefinitions, ListWorkflowGoals, ListWorkflowRevisions,
};
use crate::presentation::application_error_response;
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn workflow_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_definitions_bus = Arc::clone(&bus);
    let get_definition_bus = Arc::clone(&bus);
    let list_revisions_bus = Arc::clone(&bus);
    let get_revision_bus = Arc::clone(&bus);
    let list_goals_bus = Arc::clone(&bus);
    let get_goal_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
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
        .get(
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
        )?
        .get(
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
        )?
        .get(
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
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowRevisionResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
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
        .get(
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
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&WorkflowGoalResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/workflow-goals/{workflow_goal_id}/plan-revisions/{plan_revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
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
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&PlanRevisionResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
