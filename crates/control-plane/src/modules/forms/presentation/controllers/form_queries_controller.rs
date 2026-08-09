use super::request::request_id;
use crate::modules::forms::presentation::{FormDraftResponse, FormReleaseResponse};
use crate::modules::forms::{GetFormDraft, GetFormRelease, ListFormDrafts, ListFormReleases};
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{FormId, FormReleaseId, OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn form_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_drafts_bus = Arc::clone(&bus);
    let get_draft_bus = Arc::clone(&bus);
    let list_releases_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/forms",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_drafts_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListFormDrafts {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(values) => {
                            let values = values
                                .into_iter()
                                .map(FormDraftResponse::try_from)
                                .collect::<std::result::Result<Vec<_>, String>>()
                                .map_err(BootError::Internal)?;
                            BootResponse::json(&values)
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/forms/{form_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_draft_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetFormDraft {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            form_id: FormId::from_uuid(request.param_as::<Uuid>("form_id")?),
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(
                            &FormDraftResponse::try_from(value).map_err(BootError::Internal)?,
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/forms/{form_id}/releases",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_releases_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListFormReleases {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            form_id: FormId::from_uuid(request.param_as::<Uuid>("form_id")?),
                        })
                        .await?
                    {
                        Ok(values) => {
                            let values = values
                                .into_iter()
                                .map(FormReleaseResponse::try_from)
                                .collect::<std::result::Result<Vec<_>, String>>()
                                .map_err(BootError::Internal)?;
                            BootResponse::json(&values)
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/forms/{form_id}/releases/{release_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetFormRelease {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            form_id: FormId::from_uuid(request.param_as::<Uuid>("form_id")?),
                            release_id: FormReleaseId::from_uuid(
                                request.param_as::<Uuid>("release_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(
                            &FormReleaseResponse::try_from(value).map_err(BootError::Internal)?,
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
