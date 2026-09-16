use crate::access_projection::inference_access;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::inference::application::{GetUsageRequestFact, ListDailyUsageRollups};
use crate::modules::inference::presentation::dto::{
    DailyUsageRollupResponse, UsageRequestFactResponse,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    ControllerDefinition, QueryBus, Result,
};
use chrono::NaiveDate;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct DailyRollupsQuery {
    from_day: NaiveDate,
    to_day: NaiveDate,
}

pub fn usage_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(UsageQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct UsageQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::INFERENCE_READ])]
impl UsageQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference-usage/daily-rollups",
        raw
    )]
    async fn list_daily_rollups(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = inference_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let parameters: DailyRollupsQuery = request.query()?;
        match self
            .bus
            .execute(ListDailyUsageRollups {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                from_day: parameters.from_day,
                to_day: parameters.to_day,
                access,
            })
            .await?
        {
            Ok(rollups) => BootResponse::json(
                &rollups
                    .into_iter()
                    .map(DailyUsageRollupResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference-usage/requests/{request_id}",
        raw
    )]
    async fn get_request_fact(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id_header = request_id(&request)?;
        let access = inference_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(GetUsageRequestFact {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                request_id: request.param_as::<Uuid>("request_id")?,
                access,
            })
            .await?
        {
            Ok(fact) => BootResponse::json(&UsageRequestFactResponse::from(fact)),
            Err(error) => application_error_response(error, request_id_header),
        }
    }
}

#[cfg(test)]
mod nest_macro_usage_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn usage_queries_register_scoped_guarded_gets_via_nest_macros() {
        let controller =
            usage_queries_controller(Arc::new(QueryBus::new())).expect("usage nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference-usage/daily-rollups".to_string(),
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference-usage/requests/{request_id}".to_string(),
            ])
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::INFERENCE_READ])
        );
    }
}
