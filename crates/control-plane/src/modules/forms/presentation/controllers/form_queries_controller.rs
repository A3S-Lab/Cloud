use super::request::{form_access, request_id};
use crate::modules::forms::presentation::{FormDraftResponse, FormReleaseResponse};
use crate::modules::forms::{GetFormDraft, GetFormRelease, ListFormDrafts, ListFormReleases};
use crate::modules::shared_kernel::domain::{FormId, FormReleaseId, OrganizationId, ProjectId};
use crate::presentation::{
    application_error_response, organization_tenant_form_read_controller,
    with_deferred_project_scope,
};
use a3s_boot::{
    controller, get, BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn form_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // Nest macros own list; org-scoped draft/release reads keep deferred project
    // admission. Tenant admission stays on the Forms entry helper.
    let get_draft_bus = Arc::clone(&bus);
    let list_releases_bus = Arc::clone(&bus);
    let get_release_bus = Arc::clone(&bus);
    let mut controller = Arc::new(FormQueriesController { bus }).controller()?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::get(
        "/{organization_id}/forms/{form_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&get_draft_bus);
            async move {
                let request_id = request_id(&request)?;
                let access = form_access(&request)?;
                match bus
                    .execute(GetFormDraft {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        form_id: FormId::from_uuid(request.param_as::<Uuid>("form_id")?),
                        access,
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
    )?)?)?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::get(
        "/{organization_id}/forms/{form_id}/releases",
        move |request: BootRequest| {
            let bus = Arc::clone(&list_releases_bus);
            async move {
                let request_id = request_id(&request)?;
                let access = form_access(&request)?;
                match bus
                    .execute(ListFormReleases {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        form_id: FormId::from_uuid(request.param_as::<Uuid>("form_id")?),
                        access,
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
    )?)?)?;
    controller = controller.route(with_deferred_project_scope(RouteDefinition::get(
        "/{organization_id}/forms/{form_id}/releases/{release_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&get_release_bus);
            async move {
                let request_id = request_id(&request)?;
                let access = form_access(&request)?;
                match bus
                    .execute(GetFormRelease {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        form_id: FormId::from_uuid(request.param_as::<Uuid>("form_id")?),
                        release_id: FormReleaseId::from_uuid(
                            request.param_as::<Uuid>("release_id")?,
                        ),
                        access,
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
    )?)?)?;
    organization_tenant_form_read_controller(controller)
}

#[derive(Debug, Clone)]
struct FormQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl FormQueriesController {
    #[get("/{organization_id}/projects/{project_id}/forms", raw)]
    async fn list_drafts(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = form_access(&request)?;
        match self
            .bus
            .execute(ListFormDrafts {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access,
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
}

#[cfg(test)]
mod nest_macro_form_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn form_queries_controller_registers_list_via_nest_macros() {
        let controller = form_queries_controller(Arc::new(QueryBus::new()))
            .expect("form queries nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/forms"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path() == "/organizations/{organization_id}/forms/{form_id}"
        }));
    }
}
