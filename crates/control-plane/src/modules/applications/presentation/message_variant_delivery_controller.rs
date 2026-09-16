use super::delivery_dto::{
    ApplicationMessageVariantMutationResponse, ApplicationMessageVariantResponse,
    CreateApplicationMessageVariantRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    CreateApplicationMessageVariant, GetApplicationMessageVariant,
    ListApplicationMessageVariantsBySession,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageId, ApplicationMessageVariantId, ApplicationSessionId,
    OrganizationId, ProjectId,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn application_message_variant_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationMessageVariantCommandsController { bus }).controller()
}

pub fn application_message_variant_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationMessageVariantQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationMessageVariantCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationMessageVariantQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationMessageVariantCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-variants",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationMessageVariantRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(CreateApplicationMessageVariant {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                source_message_id: ApplicationMessageId::from_uuid(body.source_message_id),
                instruction: body.instruction,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
                created_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationMessageVariantMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationMessageVariantQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-variants",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListApplicationMessageVariantsBySession {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(items) => BootResponse::json(
                &items
                    .into_iter()
                    .map(ApplicationMessageVariantResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-variants/{variant_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationMessageVariant {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                session_id: ApplicationSessionId::from_uuid(
                    request.param_as::<Uuid>("session_id")?,
                ),
                variant_id: ApplicationMessageVariantId::from_uuid(
                    request.param_as::<Uuid>("variant_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(variant) => BootResponse::json(&ApplicationMessageVariantResponse::from(variant)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_message_variant_delivery_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn message_variant_controllers_register_scoped_routes_via_nest_macros() {
        let commands = application_message_variant_commands_controller(Arc::new(CommandBus::new()))
            .expect("message variant nest command controller");
        assert_eq!(commands.prefix(), "/organizations");
        assert_eq!(commands.routes().len(), 1);
        assert_eq!(commands.routes()[0].method(), HttpMethod::Post);
        assert_eq!(
            commands.routes()[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-variants"
        );

        let queries = application_message_variant_queries_controller(Arc::new(QueryBus::new()))
            .expect("message variant nest query controller");
        assert_eq!(queries.routes().len(), 2);
        assert_eq!(
            queries.routes()[1].path(),
            "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/message-variants/{variant_id}"
        );
        assert_eq!(
            queries.routes()[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::APPLICATION_WRITE])
        );
    }
}
