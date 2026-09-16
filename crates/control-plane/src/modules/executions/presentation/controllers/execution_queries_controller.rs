use super::request::request_id;
use crate::access_projection::execution_access;
use crate::modules::executions::application::{
    GetExecution, GetExecutionTemplate, ListExecutionTemplates, ListExecutions,
};
use crate::modules::executions::presentation::dto::{
    ExecutionResponse, ExecutionTemplateRevisionResponse,
};
use crate::modules::identity::presentation::{
    DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator,
    with_deferred_resource_scope,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, ExecutionTemplateId, ExecutionTemplateRevisionId, OrganizationId,
    ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, use_guard, BootError, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn execution_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // Nest macros own project/environment-scoped lists and template get; org-scoped
    // execution get stays deferred project-scoped.
    let get_bus = Arc::clone(&bus);
    Arc::new(ExecutionQueriesController { bus })
        .controller()?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/executions/{execution_id}",
                move |request: BootRequest| {
                    let bus = Arc::clone(&get_bus);
                    async move {
                        let request_id = request_id(&request)?;
                        match bus
                            .execute(GetExecution {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                execution_id: ExecutionId::from_uuid(
                                    request.param_as::<Uuid>("execution_id")?,
                                ),
                                access: execution_access(&resource_access_evaluator(
                                    &request.require_auth_principal()?,
                                )?),
                            })
                            .await?
                        {
                            Ok(execution) => {
                                BootResponse::json(&ExecutionResponse::from(execution))
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
struct ExecutionQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl ExecutionQueriesController {
    #[get("/{organization_id}/projects/{project_id}/execution-templates", raw)]
    async fn list_templates(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = request
            .optional_query_value_as::<usize>("limit")?
            .unwrap_or(50);
        if limit == 0 || limit > 200 {
            return Err(BootError::BadRequest(
                "limit must be between 1 and 200".into(),
            ));
        }
        match self
            .bus
            .execute(ListExecutionTemplates {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                access: execution_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                limit,
            })
            .await?
        {
            Ok(revisions) => BootResponse::json(
                &revisions
                    .into_iter()
                    .map(ExecutionTemplateRevisionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/execution-templates/{template_id}/revisions/{revision_id}",
        raw
    )]
    async fn get_template(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetExecutionTemplate {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                template_id: ExecutionTemplateId::from_uuid(
                    request.param_as::<Uuid>("template_id")?,
                ),
                revision_id: ExecutionTemplateRevisionId::from_uuid(
                    request.param_as::<Uuid>("revision_id")?,
                ),
                access: execution_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(revision) => {
                BootResponse::json(&ExecutionTemplateRevisionResponse::from(revision))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/executions",
        raw
    )]
    async fn list_executions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let limit = request
            .optional_query_value_as::<usize>("limit")?
            .unwrap_or(50);
        if limit == 0 || limit > 200 {
            return Err(BootError::BadRequest(
                "limit must be between 1 and 200".into(),
            ));
        }
        match self
            .bus
            .execute(ListExecutions {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                limit,
                access: execution_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(executions) => BootResponse::json(
                &executions
                    .into_iter()
                    .map(ExecutionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_execution_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn execution_queries_controller_registers_lists_via_nest_macros() {
        let controller = execution_queries_controller(Arc::new(QueryBus::new()))
            .expect("execution queries nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/execution-templates"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/executions/{execution_id}"
        }));
    }
}
