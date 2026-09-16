use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::inference::application::GetInferenceUsageRetentionStatus;
use crate::modules::inference::presentation::dto::InferenceUsageRetentionStatusResponse;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::application_error_response;
use crate::presentation::request_id;
use a3s_boot::{
    controller, get, metadata, use_guard, BootRequest, BootResponse, ControllerDefinition,
    QueryBus, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn usage_retention_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(UsageRetentionController { bus }).controller()
}

#[derive(Debug, Clone)]
struct UsageRetentionController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl UsageRetentionController {
    #[get("/{organization_id}/inference-usage/retention", raw)]
    async fn retention_status(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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
}

#[cfg(test)]
mod nest_macro_usage_retention_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn usage_retention_controller_registers_scoped_guarded_get_via_nest_macros() {
        let controller = usage_retention_controller(Arc::new(QueryBus::new()))
            .expect("usage retention nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/inference-usage/retention"
        );
        let scopes = routes[0]
            .metadata()
            .get(AUTH_SCOPES_METADATA)
            .cloned()
            .expect("auth.scopes metadata");
        assert_eq!(scopes, serde_json::json!([ApiTokenScope::CLOUD_READ]));
    }
}
