use super::request::{actor_principal_id, expected_version, form_access, request_identity};
use crate::modules::forms::presentation::{
    FormDraftMutationResponse, FormDraftRequest, FormPublicationMutationResponse,
};
use crate::modules::forms::{CreateFormDraft, PublishFormRelease, ReviseFormDraft};
use crate::modules::shared_kernel::domain::{FormId, OrganizationId, ProjectId};
use crate::presentation::{
    application_error_response, organization_tenant_form_write_controller,
    with_deferred_project_scope,
};
use a3s_boot::{
    controller, post, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn form_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    // Nest macros own create; revise/publish keep deferred project admission.
    // Tenant admission stays on the Forms entry helper (architecture boundary).
    let revise_bus = Arc::clone(&bus);
    let publish_bus = Arc::clone(&bus);
    let mut controller = Arc::new(FormCommandsController { bus }).controller()?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::post(
        "/{organization_id}/forms/{form_id}/draft-revisions",
        move |request: BootRequest| {
            let bus = Arc::clone(&revise_bus);
            async move {
                let body: FormDraftRequest = request.json_with_content_type()?;
                let (name, description, document_json) =
                    body.into_parts().map_err(BootError::BadRequest)?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let form_id = FormId::from_uuid(request.param_as::<Uuid>("form_id")?);
                let access = form_access(&request)?;
                let expected_version = expected_version(&request)?;
                let actor_principal_id = actor_principal_id(&request)?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(ReviseFormDraft {
                        organization_id,
                        form_id,
                        access,
                        expected_version,
                        name,
                        description,
                        document_json,
                        actor_principal_id,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        let response = FormDraftMutationResponse::try_from(result)
                            .map_err(BootError::Internal)?;
                        BootResponse::json_with_status(status, &response)
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?)?)?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::post(
        "/{organization_id}/forms/{form_id}/releases",
        move |request: BootRequest| {
            let bus = Arc::clone(&publish_bus);
            async move {
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let form_id = FormId::from_uuid(request.param_as::<Uuid>("form_id")?);
                let access = form_access(&request)?;
                let expected_version = expected_version(&request)?;
                let actor_principal_id = actor_principal_id(&request)?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(PublishFormRelease {
                        organization_id,
                        form_id,
                        access,
                        expected_version,
                        actor_principal_id,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        let response = FormPublicationMutationResponse::try_from(result)
                            .map_err(BootError::Internal)?;
                        BootResponse::json_with_status(status, &response)
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?)?)?;
    organization_tenant_form_write_controller(controller)
}

#[derive(Debug, Clone)]
struct FormCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
impl FormCommandsController {
    #[post("/{organization_id}/projects/{project_id}/forms", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: FormDraftRequest = request.json_with_content_type()?;
        let (name, description, document_json) =
            body.into_parts().map_err(BootError::BadRequest)?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let access = form_access(&request)?;
        let actor_principal_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateFormDraft {
                organization_id,
                project_id,
                access,
                name,
                description,
                document_json,
                actor_principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                let response =
                    FormDraftMutationResponse::try_from(result).map_err(BootError::Internal)?;
                BootResponse::json_with_status(status, &response)
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_form_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn form_commands_controller_registers_create_via_nest_macros() {
        let controller = form_commands_controller(Arc::new(CommandBus::new()))
            .expect("form commands nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/forms"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path()
                    == "/organizations/{organization_id}/forms/{form_id}/draft-revisions"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Post
                && route.path() == "/organizations/{organization_id}/forms/{form_id}/releases"
        }));
    }
}
