use crate::modules::identity::application::commands::link_partner_subject::LinkPartnerSubject;
use crate::modules::identity::application::commands::revoke_partner_subject_link::RevokePartnerSubjectLink;
use crate::modules::identity::application::queries::list_partner_subject_links::ListPartnerSubjectLinks;
use crate::modules::identity::application::queries::resolve_partner_subject::ResolvePartnerSubject;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    LinkPartnerSubjectRequest, ListPartnerSubjectLinksQuery, PartnerSubjectLinkMutationResponse,
    PartnerSubjectLinkResponse, ResolvePartnerSubjectQuery, RevokePartnerSubjectLinkRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, HttpMethod, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn partner_subject_link_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PartnerSubjectLinkCommandsController { command_bus }).controller()
}

pub fn partner_subject_link_queries_controller(
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PartnerSubjectLinkQueriesController { query_bus }).controller()
}

/// Admin reverse-query surface for SubjectLink (matches directory grant list guards).
pub fn partner_subject_link_admin_queries_controller(
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PartnerSubjectLinkAdminQueriesController { query_bus }).controller()
}

#[derive(Debug, Clone)]
struct PartnerSubjectLinkCommandsController {
    command_bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct PartnerSubjectLinkQueriesController {
    query_bus: Arc<QueryBus>,
}

#[derive(Debug, Clone)]
struct PartnerSubjectLinkAdminQueriesController {
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl PartnerSubjectLinkCommandsController {
    #[post("/{organization_id}/partner-subject-links", raw)]
    async fn link(&self, request: BootRequest) -> Result<BootResponse> {
        let body: LinkPartnerSubjectRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(LinkPartnerSubject {
                organization_id,
                provider_key: body.provider_key,
                issuer: body.issuer,
                subject: body.subject,
                principal_id: PrincipalId::from_uuid(body.principal_id),
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &PartnerSubjectLinkMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/partner-subject-links/revocation", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokePartnerSubjectLinkRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(RevokePartnerSubjectLink {
                organization_id,
                provider_key: body.provider_key,
                issuer: body.issuer,
                subject: body.subject,
                expected_version: body.expected_version,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&PartnerSubjectLinkMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl PartnerSubjectLinkQueriesController {
    #[get("/{organization_id}/partner-subject-links/resolve", raw)]
    async fn resolve(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let query: ResolvePartnerSubjectQuery = request.query()?;
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ResolvePartnerSubject {
                organization_id,
                provider_key: query.provider_key,
                issuer: query.issuer,
                subject: query.subject,
            })
            .await?
        {
            Ok(view) => BootResponse::json(&PartnerSubjectLinkResponse::from(view)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl PartnerSubjectLinkAdminQueriesController {
    #[get("/{organization_id}/partner-subject-links", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let query: ListPartnerSubjectLinksQuery = request.query()?;
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListPartnerSubjectLinks {
                organization_id,
                principal_id: PrincipalId::from_uuid(query.principal_id),
                provider_key: query.provider_key,
            })
            .await?
        {
            Ok(links) => BootResponse::json(
                &links
                    .into_iter()
                    .map(PartnerSubjectLinkResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_partner_subject_link_controller_tests {
    use super::*;

    #[test]
    fn partner_subject_link_commands_register_admin_write_routes() {
        let controller =
            partner_subject_link_commands_controller(Arc::new(CommandBus::new()))
                .expect("partner subject link commands");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/partner-subject-links"
        );
        assert_eq!(routes[1].method(), HttpMethod::Post);
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/partner-subject-links/revocation"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::IDENTITY_WRITE])
        );
    }

    #[test]
    fn partner_subject_link_queries_register_member_readable_resolve() {
        let controller =
            partner_subject_link_queries_controller(Arc::new(QueryBus::new()))
                .expect("partner subject link queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/partner-subject-links/resolve"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::CLOUD_READ])
        );
    }

    #[test]
    fn partner_subject_link_admin_queries_register_list_by_principal() {
        let controller =
            partner_subject_link_admin_queries_controller(Arc::new(QueryBus::new()))
                .expect("partner subject link admin queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/partner-subject-links"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::IDENTITY_WRITE])
        );
    }
}
