//! Applications management REST for exact-release publication route intents.
//!
//! `APP0.3-C13` exposes C12 project-authorized create/get/list-by-release over
//! the management bus. Gateway Edge projection and Delivery rate middleware stay
//! later.

use super::dto::{
    ApplicationPublicationRouteIntentMutationResponse, ApplicationPublicationRouteIntentResponse,
    CreateApplicationPublicationRouteIntentRequest,
};
use crate::access_projection::application_access;
use crate::modules::applications::application::{
    CreateApplicationPublicationRouteIntent, GetApplicationPublicationRouteIntent,
    ListApplicationPublicationRouteIntentsByRelease,
};
use crate::modules::applications::domain::{
    ApplicationPublicationChannel, ApplicationPublicationRateShapingPolicyRef,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, OrganizationId,
    ProjectId, Sha256Digest,
};
use crate::presentation::{actor_principal_id, application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, post, use_guard, BootRequest, BootResponse, CommandBus,
    ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn application_publication_route_intent_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationPublicationRouteIntentCommandsController { bus }).controller()
}

pub fn application_publication_route_intent_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApplicationPublicationRouteIntentQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct ApplicationPublicationRouteIntentCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct ApplicationPublicationRouteIntentQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::APPLICATION_WRITE])]
impl ApplicationPublicationRouteIntentCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/releases/{release_id}/publication-route-intents",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateApplicationPublicationRouteIntentRequest =
            request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        let digest = match Sha256Digest::parse(&body.application_release_digest) {
            Ok(value) => value,
            Err(error) => {
                return application_error_response(ApplicationError::Invalid(error), request_id);
            }
        };
        let channels = match parse_channels(&body.channels) {
            Ok(value) => value,
            Err(error) => {
                return application_error_response(ApplicationError::Invalid(error), request_id);
            }
        };
        let rate_shaping_policy = match parse_rate_policy(&body.rate_shaping_policy) {
            Ok(value) => value,
            Err(error) => {
                return application_error_response(ApplicationError::Invalid(error), request_id);
            }
        };
        match self
            .bus
            .execute(CreateApplicationPublicationRouteIntent {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                application_release_id: ApplicationReleaseId::from_uuid(
                    request.param_as::<Uuid>("release_id")?,
                ),
                application_release_digest: digest,
                channels,
                embed_origin_allowlist: body.embed_origin_allowlist,
                rate_shaping_policy,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &ApplicationPublicationRouteIntentMutationResponse::from(result),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl ApplicationPublicationRouteIntentQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/publication-route-intents/{intent_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetApplicationPublicationRouteIntent {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                intent_id: ApplicationPublicationRouteIntentId::from_uuid(
                    request.param_as::<Uuid>("intent_id")?,
                ),
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(intent) => {
                BootResponse::json(&ApplicationPublicationRouteIntentResponse::from(intent))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/applications/{application_id}/releases/{release_id}/publication-route-intents",
        raw
    )]
    async fn list_by_release(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let digest_raw = request.query_value_as::<String>("applicationReleaseDigest")?;
        let digest = match Sha256Digest::parse(&digest_raw) {
            Ok(value) => value,
            Err(error) => {
                return application_error_response(ApplicationError::Invalid(error), request_id);
            }
        };
        match self
            .bus
            .execute(ListApplicationPublicationRouteIntentsByRelease {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                application_id: ApplicationId::from_uuid(
                    request.param_as::<Uuid>("application_id")?,
                ),
                application_release_id: ApplicationReleaseId::from_uuid(
                    request.param_as::<Uuid>("release_id")?,
                ),
                application_release_digest: digest,
                actor_principal_id: actor_principal_id(&request)?,
                access: application_access(&resource_access_evaluator(
                    &request.require_auth_principal()?,
                )?),
            })
            .await?
        {
            Ok(intents) => BootResponse::json(
                &intents
                    .into_iter()
                    .map(ApplicationPublicationRouteIntentResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn parse_channels(
    values: &[String],
) -> std::result::Result<Vec<ApplicationPublicationChannel>, String> {
    values
        .iter()
        .map(|value| ApplicationPublicationChannel::parse(value))
        .collect()
}

fn parse_rate_policy(
    value: &super::dto::ApplicationPublicationRateShapingPolicyRequest,
) -> std::result::Result<ApplicationPublicationRateShapingPolicyRef, String> {
    let digest = Sha256Digest::parse(&value.policy_revision_digest)?;
    ApplicationPublicationRateShapingPolicyRef::create(value.profile_id.clone(), digest)
}

#[cfg(test)]
mod nest_macro_publication_route_intent_controller_tests {
    use super::*;

    #[test]
    fn publication_route_intent_controllers_register_release_and_intent_routes_via_nest_macros() {
        let commands =
            application_publication_route_intent_commands_controller(Arc::new(CommandBus::new()))
                .expect("commands controller");
        let queries =
            application_publication_route_intent_queries_controller(Arc::new(QueryBus::new()))
                .expect("queries controller");
        assert_routes_contain(
            &commands,
            &[(
                "POST",
                "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/releases/{release_id}/publication-route-intents",
            )],
        );
        assert_routes_contain(
            &queries,
            &[
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/publication-route-intents/{intent_id}",
                ),
                (
                    "GET",
                    "/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/releases/{release_id}/publication-route-intents",
                ),
            ],
        );
    }

    fn assert_routes_contain(controller: &ControllerDefinition, expected: &[(&str, &str)]) {
        let routes = controller.routes();
        for (method, path) in expected {
            assert!(
                routes.iter().any(|route| {
                    route.method().as_str().eq_ignore_ascii_case(method) && route.path() == *path
                }),
                "missing {method} {path}; have {:?}",
                routes
                    .iter()
                    .map(|route| format!("{} {}", route.method().as_str(), route.path()))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
