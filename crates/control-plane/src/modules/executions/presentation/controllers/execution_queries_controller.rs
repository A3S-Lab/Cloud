use super::request::request_id;
use crate::modules::executions::application::{GetExecution, ListExecutions};
use crate::modules::executions::presentation::dto::ExecutionResponse;
use crate::modules::identity::presentation::{
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn execution_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/executions",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let limit = request
                        .optional_query_value_as::<usize>("limit")?
                        .unwrap_or(50);
                    if limit == 0 || limit > 200 {
                        return Err(BootError::BadRequest(
                            "limit must be between 1 and 200".into(),
                        ));
                    }
                    match bus
                        .execute(ListExecutions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            limit,
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
            },
        )?
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
                                resource_access: resource_access_evaluator(
                                    &request.require_auth_principal()?,
                                )?,
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
