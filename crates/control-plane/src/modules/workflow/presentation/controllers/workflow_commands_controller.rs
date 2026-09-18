use super::request::{
    actor_principal_id, credential_actor, expected_version, request_id, request_identity,
    workflow_access, workflow_goal_acl,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    HumanTaskId, OrganizationId, ProjectId, WorkflowDefinitionId,
};
use crate::modules::workflow::presentation::dto::{
    CancelWorkflowRunRequest, HumanTaskMutationResponse, PublishWorkflowDefinitionRequest,
    StartWorkflowRunRequest, WorkflowDefinitionMutationResponse, WorkflowGoalMutationResponse,
    WorkflowRunMutationResponse,
};
use crate::modules::workflow::{
    CancelWorkflowRun, ChangeHumanTaskAssignment, CreateWorkflowDefinition, CreateWorkflowGoal,
    HumanTaskAssignmentAction, ReviseWorkflowDefinition, StartWorkflowRun, SubmitHumanTask,
};
use crate::presentation::{DeferredResourceScope, OrganizationTenantGuard, with_deferred_resource_scope, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use a3s_form_core::FormInteractionSubmission;
use std::sync::Arc;
use uuid::Uuid;

pub fn workflow_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    // Nest macros own create-definition / create-goal / start-run; revise/cancel/
    // human-task mutations keep deferred resource admission (not a Nest attribute).
    let revise_definition_bus = Arc::clone(&bus);
    let cancel_run_bus = Arc::clone(&bus);
    let claim_human_task_bus = Arc::clone(&bus);
    let release_human_task_bus = Arc::clone(&bus);
    let submit_human_task_bus = Arc::clone(&bus);
    let mut controller = Arc::new(WorkflowCommandsController { bus }).controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&revise_definition_bus);
                async move {
                    let body: PublishWorkflowDefinitionRequest =
                        request.json_with_content_type()?;
                    let (definition_acl, payloads, semantic_contracts) = body.into_parts();
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workflow_definition_id = WorkflowDefinitionId::from_uuid(
                        request.param_as::<Uuid>("workflow_definition_id")?,
                    );
                    let expected_version = expected_version(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let access = workflow_access(&request)?;
                    match bus
                        .execute(ReviseWorkflowDefinition {
                            organization_id,
                            workflow_definition_id,
                            access,
                            definition_acl,
                            payloads,
                            semantic_contracts,
                            expected_version,
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
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/workflow-runs/{workflow_run_id}/cancel",
            move |request: BootRequest| {
                let bus = Arc::clone(&cancel_run_bus);
                async move {
                    let body: CancelWorkflowRunRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workflow_run_id =
                        crate::modules::shared_kernel::domain::WorkflowRunId::from_uuid(
                            request.param_as::<Uuid>("workflow_run_id")?,
                        );
                    let access = workflow_access(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CancelWorkflowRun {
                            organization_id,
                            workflow_run_id,
                            access,
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
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/human-tasks/{human_task_id}/claim",
            move |request: BootRequest| {
                let bus = Arc::clone(&claim_human_task_bus);
                async move {
                    change_human_task_assignment(bus, request, HumanTaskAssignmentAction::Claim)
                        .await
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/human-tasks/{human_task_id}/release",
            move |request: BootRequest| {
                let bus = Arc::clone(&release_human_task_bus);
                async move {
                    change_human_task_assignment(
                        bus,
                        request,
                        HumanTaskAssignmentAction::Release,
                    )
                    .await
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    controller.route(with_deferred_resource_scope(
        RouteDefinition::post(
            "/{organization_id}/human-tasks/{human_task_id}/submission",
            move |request: BootRequest| {
                let bus = Arc::clone(&submit_human_task_bus);
                async move {
                    let submission: FormInteractionSubmission =
                        request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let human_task_id =
                        HumanTaskId::from_uuid(request.param_as::<Uuid>("human_task_id")?);
                    let access = workflow_access(&request)?;
                    let actor = credential_actor(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(SubmitHumanTask {
                            organization_id,
                            human_task_id,
                            access,
                            submission,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                            requested_at: chrono::Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            BootResponse::json(&HumanTaskMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)
}

#[derive(Debug, Clone)]
struct WorkflowCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::WORKFLOW_WRITE])]
impl WorkflowCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/workflow-definitions",
        raw
    )]
    async fn create_definition(&self, request: BootRequest) -> Result<BootResponse> {
        let body: PublishWorkflowDefinitionRequest = request.json_with_content_type()?;
        let (definition_acl, payloads, semantic_contracts) = body.into_parts();
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let actor_principal_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = workflow_access(&request)?;
        match self
            .bus
            .execute(CreateWorkflowDefinition {
                organization_id,
                project_id,
                access,
                definition_acl,
                payloads,
                semantic_contracts,
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

    #[post("/{organization_id}/projects/{project_id}/workflow-goals", raw)]
    async fn create_goal(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let goal_acl = workflow_goal_acl(&request)?;
        let actor_principal_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = workflow_access(&request)?;
        match self
            .bus
            .execute(CreateWorkflowGoal {
                organization_id,
                project_id,
                access,
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

    #[post("/{organization_id}/projects/{project_id}/workflow-runs", raw)]
    async fn start_run(&self, request: BootRequest) -> Result<BootResponse> {
        let body: StartWorkflowRunRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let actor_principal_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = workflow_access(&request)?;
        match self
            .bus
            .execute(StartWorkflowRun {
                organization_id,
                project_id,
                access,
                workflow_goal_id: crate::modules::shared_kernel::domain::WorkflowGoalId::from_uuid(
                    body.workflow_goal_id,
                ),
                plan_revision_id: crate::modules::shared_kernel::domain::PlanRevisionId::from_uuid(
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
}

async fn change_human_task_assignment(
    bus: Arc<CommandBus>,
    request: BootRequest,
    action: HumanTaskAssignmentAction,
) -> Result<BootResponse> {
    let organization_id = OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
    let human_task_id = HumanTaskId::from_uuid(request.param_as::<Uuid>("human_task_id")?);
    let access = workflow_access(&request)?;
    let expected_version = expected_version(&request)?;
    let actor_principal_id = actor_principal_id(&request)?;
    let (idempotency_key, request_id) = request_identity(&request)?;
    match bus
        .execute(ChangeHumanTaskAssignment {
            organization_id,
            human_task_id,
            access,
            action,
            expected_version,
            actor_principal_id,
            idempotency_key,
            request_id,
            requested_at: chrono::Utc::now(),
        })
        .await?
    {
        Ok(result) => BootResponse::json(&HumanTaskMutationResponse::from(result)),
        Err(error) => application_error_response(error, request_id),
    }
}

#[cfg(test)]
mod nest_macro_workflow_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn workflow_commands_controller_registers_create_surfaces_via_nest_macros() {
        let controller = workflow_commands_controller(Arc::new(CommandBus::new()))
            .expect("workflow commands nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 8);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/workflow-definitions"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/workflow-goals"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/workflow-runs"
        }));
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::WORKFLOW_WRITE])
        );
    }
}
