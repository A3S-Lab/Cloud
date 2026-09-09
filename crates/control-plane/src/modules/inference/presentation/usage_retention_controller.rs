use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::inference::application::GetInferenceUsageRetentionStatus;
use crate::modules::inference::presentation::dto::InferenceUsageRetentionStatusResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::presentation::request_id;

pub fn usage_retention_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_guard(OrganizationAdministratorGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/inference-usage/retention",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetInferenceUsageRetentionStatus {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(status) => {
                            BootResponse::json(&InferenceUsageRetentionStatusResponse::from(status))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
