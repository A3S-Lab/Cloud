use super::request::{actor_principal_id, expected_version, request_identity};
use crate::modules::forms::presentation::{
    FormDraftMutationResponse, FormDraftRequest, FormPublicationMutationResponse,
};
use crate::modules::forms::{CreateFormDraft, PublishFormRelease, ReviseFormDraft};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{FormId, OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn form_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    let revise_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::FORM_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/forms",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: FormDraftRequest = request.json_with_content_type()?;
                    let (name, description, document_json) =
                        body.into_parts().map_err(BootError::BadRequest)?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateFormDraft {
                            organization_id,
                            project_id,
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
        )?
        .post(
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
                    let expected_version = expected_version(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ReviseFormDraft {
                            organization_id,
                            form_id,
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
        )?
        .post(
            "/{organization_id}/forms/{form_id}/releases",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let form_id = FormId::from_uuid(request.param_as::<Uuid>("form_id")?);
                    let expected_version = expected_version(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(PublishFormRelease {
                            organization_id,
                            form_id,
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
        )
}
