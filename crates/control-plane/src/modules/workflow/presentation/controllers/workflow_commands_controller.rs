use super::request::{
    actor_principal_id, request_identity, workflow_goal_acl, workflow_revision_control,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, WorkflowDefinitionId};
use crate::modules::workflow::presentation::dto::{
    PublishWorkflowDefinitionRequest, WorkflowDefinitionMutationResponse,
    WorkflowGoalMutationResponse,
};
use crate::modules::workflow::{
    CreateWorkflowDefinition, CreateWorkflowGoal, ReviseWorkflowDefinition,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn workflow_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_definition_bus = Arc::clone(&bus);
    let revise_definition_bus = Arc::clone(&bus);
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
        .post(
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
                    let expected_version = workflow_revision_control(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ReviseWorkflowDefinition {
                            organization_id,
                            workflow_definition_id,
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
        )?
        .post(
            "/{organization_id}/projects/{project_id}/workflow-goals",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
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
        )
}
