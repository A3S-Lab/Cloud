use super::request::{
    actor_principal_id, request_identity, resource_access, workflow_goal_acl,
    workflow_revision_control,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    with_deferred_resource_scope, DeferredResourceScope, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, WorkflowDefinitionId};
use crate::modules::workflow::presentation::dto::{
    CancelWorkflowRunRequest, PublishWorkflowDefinitionRequest, StartWorkflowRunRequest,
    WorkflowDefinitionMutationResponse, WorkflowGoalMutationResponse, WorkflowRunMutationResponse,
};
use crate::modules::workflow::{
    CancelWorkflowRun, CreateWorkflowDefinition, CreateWorkflowGoal, ReviseWorkflowDefinition,
    StartWorkflowRun,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn workflow_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_definition_bus = Arc::clone(&bus);
    let revise_definition_bus = Arc::clone(&bus);
    let create_goal_bus = Arc::clone(&bus);
    let start_run_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::WORKFLOW_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/workflow-definitions",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_definition_bus);
                async move {
                    let body: PublishWorkflowDefinitionRequest =
                        request.json_with_content_type()?;
                    let (definition_acl, payloads) = body.into_parts();
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateWorkflowDefinition {
                            organization_id,
                            project_id,
                            definition_acl,
                            payloads,
                            actor_principal_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &WorkflowDefinitionMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions",
                move |request: BootRequest| {
                    let bus = Arc::clone(&revise_definition_bus);
                    async move {
                        let body: PublishWorkflowDefinitionRequest =
                            request.json_with_content_type()?;
                        let (definition_acl, payloads) = body.into_parts();
                        let organization_id =
                            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                        let workflow_definition_id = WorkflowDefinitionId::from_uuid(
                            request.param_as::<Uuid>("workflow_definition_id")?,
                        );
                        let resource_access = resource_access(&request)?;
                        let expected_version = workflow_revision_control(&request)?;
                        let actor_principal_id = actor_principal_id(&request)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(ReviseWorkflowDefinition {
                                organization_id,
                                workflow_definition_id,
                                resource_access,
                                expected_version,
                                definition_acl,
                                payloads,
                                actor_principal_id,
                                idempotency_key,
                                request_id,
                            })
                            .await?
                        {
                            Ok(result) => BootResponse::json_with_status(
                                if result.replayed { 200 } else { 201 },
                                &WorkflowDefinitionMutationResponse::from(result),
                            ),
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .post(
            "/{organization_id}/projects/{project_id}/workflow-goals",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_goal_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let goal_acl = workflow_goal_acl(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateWorkflowGoal {
                            organization_id,
                            project_id,
                            goal_acl,
                            actor_principal_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &WorkflowGoalMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/workflow-runs",
            move |request: BootRequest| {
                let bus = Arc::clone(&start_run_bus);
                async move {
                    let body: StartWorkflowRunRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(StartWorkflowRun {
                            organization_id,
                            project_id,
                            workflow_goal_id:
                                crate::modules::shared_kernel::domain::WorkflowGoalId::from_uuid(
                                    body.workflow_goal_id,
                                ),
                            plan_revision_id:
                                crate::modules::shared_kernel::domain::PlanRevisionId::from_uuid(
                                    body.plan_revision_id,
                                ),
                            timeout_seconds: body.timeout_seconds,
                            actor_principal_id,
                            idempotency_key,
                            request_id,
                            requested_at: chrono::Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 202 },
                            &WorkflowRunMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/workflow-runs/{workflow_run_id}/cancel",
                move |request: BootRequest| {
                    let bus = Arc::clone(&bus);
                    async move {
                        let body: CancelWorkflowRunRequest = request.json_with_content_type()?;
                        let organization_id =
                            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                        let workflow_run_id =
                            crate::modules::shared_kernel::domain::WorkflowRunId::from_uuid(
                                request.param_as::<Uuid>("workflow_run_id")?,
                            );
                        let resource_access = resource_access(&request)?;
                        let actor_principal_id = actor_principal_id(&request)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(CancelWorkflowRun {
                                organization_id,
                                workflow_run_id,
                                resource_access,
                                reason: body.reason,
                                actor_principal_id,
                                idempotency_key,
                                request_id,
                                requested_at: chrono::Utc::now(),
                            })
                            .await?
                        {
                            Ok(result) => BootResponse::json_with_status(
                                if result.replayed { 200 } else { 202 },
                                &WorkflowRunMutationResponse::from(result),
                            ),
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)
}
