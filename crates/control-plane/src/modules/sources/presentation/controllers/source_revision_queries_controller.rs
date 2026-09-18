use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::application::queries::list_source_revisions::ListSourceRevisions;
use crate::modules::sources::presentation::dto::SourceRevisionResponse;
use crate::presentation::{
    OrganizationTenantGuard, resource_access_evaluator, application_error_response, source_access};
use a3s_boot::{
    controller, get, use_guard, BootError, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn source_revision_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(SourceRevisionQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct SourceRevisionQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl SourceRevisionQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let request_id = request_id(&request)?;
        let access = source_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListSourceRevisions {
                organization_id,
                project_id,
                environment_id,
                access,
            })
            .await?
        {
            Ok(revisions) => BootResponse::json(
                &revisions
                    .into_iter()
                    .map(SourceRevisionResponse::from_revision)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

#[cfg(test)]
mod nest_macro_source_revision_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn source_revision_queries_controller_registers_guarded_get_via_nest_macros() {
        let controller = source_revision_queries_controller(Arc::new(QueryBus::new()))
            .expect("source revision nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions"
        );
    }
}
