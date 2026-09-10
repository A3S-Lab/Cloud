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
    resource_access_evaluator, with_deferred_resource_scope, DeferredResourceScope,
    OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn inference_key_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    let rotate_bus = Arc::clone(&bus);
    let revoke_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::INFERENCE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateInferenceKeyRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    let requested_at = Utc::now();
                    match bus
                        .execute(CreateInferenceKey {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys/{credential_id}/rotate",
            move |request: BootRequest| {
                let bus = Arc::clone(&rotate_bus);
                async move {
                    let body: RotateInferenceKeyRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RotateInferenceKey {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys/{credential_id}/revoke",
            move |request: BootRequest| {
                let bus = Arc::clone(&revoke_bus);
                async move {
                    let body: RevokeInferenceKeyRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokeInferenceKey {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )
}

pub fn inference_key_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::INFERENCE_READ])?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/keys",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    match bus
                        .execute(ListInferenceKeys {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            resource_access,
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
            },
        )?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/inference/keys/{credential_id}",
                move |request: BootRequest| {
                    let bus = Arc::clone(&bus);
                    async move {
                        let request_id = request_id(&request)?;
                        let resource_access =
                            resource_access_evaluator(&request.require_auth_principal()?)?;
                        match bus
                            .execute(GetInferenceKey {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                credential_id: InferenceCredentialId::from_uuid(
                                    request.param_as::<Uuid>("credential_id")?,
                                ),
                                resource_access,
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
        )?)
}

fn delivery_response(status: u16, response: InferenceKeyDeliveryResponse) -> Result<BootResponse> {
    Ok(BootResponse::json_with_status(status, &response)?
        .with_header("cache-control", "no-store")
        .with_header("pragma", "no-cache")
        .with_header("referrer-policy", "no-referrer"))
}
