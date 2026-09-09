use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::inference::application::{GetUsageRequestFact, ListDailyUsageRollups};
use crate::modules::inference::presentation::dto::{
    DailyUsageRollupResponse, UsageRequestFactResponse,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
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
    let list_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference-usage/daily-rollups",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    let parameters: DailyRollupsQuery = request.query()?;
                    match bus
                        .execute(ListDailyUsageRollups {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            from_day: parameters.from_day,
                            to_day: parameters.to_day,
                            resource_access,
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
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference-usage/requests/{request_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id_header = request_id(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    match bus
                        .execute(GetUsageRequestFact {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            request_id: request.param_as::<Uuid>("request_id")?,
                            resource_access,
                        })
                        .await?
                    {
                        Ok(fact) => BootResponse::json(&UsageRequestFactResponse::from(fact)),
                        Err(error) => application_error_response(error, request_id_header),
                    }
                }
            },
        )
}
