use crate::access_projection::identity_access;
use crate::modules::identity::application::commands::create_inference_key::CreateInferenceKey;
use crate::modules::identity::application::commands::revoke_inference_key::RevokeInferenceKey;
use crate::modules::identity::application::commands::rotate_inference_key::RotateInferenceKey;
use crate::modules::identity::application::queries::get_inference_key::GetInferenceKey;
use crate::modules::identity::application::queries::list_inference_keys::ListInferenceKeys;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateInferenceKeyRequest, InferenceKeyDeliveryResponse, InferenceKeyMutationResponse,
    InferenceKeyResponse, RevokeInferenceKeyRequest, RotateInferenceKeyRequest,
};
use crate::modules::identity::presentation::request_context::{mutation_identity, request_id};
use crate::modules::identity::presentation::{
    DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator,
    with_deferred_resource_scope,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn inference_key_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(InferenceKeyCommandsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct InferenceKeyCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::INFERENCE_WRITE])]
impl InferenceKeyCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateInferenceKeyRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        let requested_at = Utc::now();
        match self
            .bus
            .execute(CreateInferenceKey {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                expires_at: body.expires_at,
                idempotency_key,
                request_id,
                requested_at,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                delivery_response(status, InferenceKeyDeliveryResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys/{credential_id}/rotate",
        raw
    )]
    async fn rotate(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RotateInferenceKeyRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
            .execute(RotateInferenceKey {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                credential_id: InferenceCredentialId::from_uuid(
                    request.param_as::<Uuid>("credential_id")?,
                ),
                expected_aggregate_version: body.expected_aggregate_version,
                expires_at: body.expires_at,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                delivery_response(status, InferenceKeyDeliveryResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys/{credential_id}/revoke",
        raw
    )]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeInferenceKeyRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
            .execute(RevokeInferenceKey {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                credential_id: InferenceCredentialId::from_uuid(
                    request.param_as::<Uuid>("credential_id")?,
                ),
                expected_aggregate_version: body.expected_aggregate_version,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => Ok(BootResponse::json_with_status(
                202,
                &InferenceKeyMutationResponse::from(result),
            )?
            .with_header("cache-control", "no-store")
            .with_header("pragma", "no-cache")
            .with_header("referrer-policy", "no-referrer")),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

pub fn inference_key_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // List uses Nest macros; org-scoped get keeps deferred resource admission.
    let mut controller = Arc::new(InferenceKeyQueriesController {
        bus: Arc::clone(&bus),
    })
    .controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/inference/keys/{credential_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = identity_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
                        .execute(GetInferenceKey {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            credential_id: InferenceCredentialId::from_uuid(
                                request.param_as::<Uuid>("credential_id")?,
                            ),
                            access,
                        })
                        .await?
                    {
                        Ok(credential) => {
                            BootResponse::json(&InferenceKeyResponse::from(credential))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        // Credential ownership is resolved after load; restricted callers need
        // coarse admission before Identity fail-closes on environment visibility.
        DeferredResourceScope::Any,
    )?)?;
    Ok(controller)
}

#[derive(Debug, Clone)]
struct InferenceKeyQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::INFERENCE_READ])]
impl InferenceKeyQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = identity_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListInferenceKeys {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                access,
            })
            .await?
        {
            Ok(credentials) => BootResponse::json(
                &credentials
                    .into_iter()
                    .map(InferenceKeyResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn delivery_response(status: u16, response: InferenceKeyDeliveryResponse) -> Result<BootResponse> {
    Ok(BootResponse::json_with_status(status, &response)?
        .with_header("cache-control", "no-store")
        .with_header("pragma", "no-cache")
        .with_header("referrer-policy", "no-referrer"))
}

#[cfg(test)]
mod nest_macro_inference_key_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn inference_key_commands_register_scoped_guarded_posts_via_nest_macros() {
        let controller = inference_key_commands_controller(Arc::new(CommandBus::new()))
            .expect("inference key nest commands");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes
            .iter()
            .all(|route| route.method() == HttpMethod::Post));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys".to_string(),
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys/{credential_id}/rotate".to_string(),
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys/{credential_id}/revoke".to_string(),
            ])
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::INFERENCE_WRITE])
        );
    }

    #[test]
    fn inference_key_queries_register_list_via_nest_macros_and_deferred_get() {
        let controller = inference_key_queries_controller(Arc::new(QueryBus::new()))
            .expect("inference key nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys".to_string(),
                "/organizations/{organization_id}/inference/keys/{credential_id}".to_string(),
            ])
        );
    }
}
