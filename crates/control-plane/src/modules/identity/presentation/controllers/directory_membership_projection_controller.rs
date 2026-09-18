use crate::modules::identity::application::commands::replace_directory_membership_projection::ReplaceDirectoryMembershipProjection;
use crate::modules::identity::application::queries::list_directory_membership_projections::{
    DirectoryMembershipProjectionListFilter, ListDirectoryMembershipProjections,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    DirectoryMembershipProjectionBindingResponse, DirectoryMembershipProjectionMutationResponse,
    ListDirectoryMembershipProjectionsQuery, ReplaceDirectoryMembershipProjectionRequest,
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
    controller, get, metadata, put, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn directory_membership_projection_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(DirectoryMembershipProjectionController {
        command_bus,
        query_bus,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct DirectoryMembershipProjectionController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl DirectoryMembershipProjectionController {
    #[get("/{organization_id}/directory-membership-projections", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let query: ListDirectoryMembershipProjectionsQuery = request.query()?;
        let request_id = request_id(&request)?;
        let filter = match (query.subject_ref, query.principal_id) {
            (Some(subject_ref), None) => {
                DirectoryMembershipProjectionListFilter::SubjectRef(subject_ref)
            }
            (None, Some(principal_id)) => {
                DirectoryMembershipProjectionListFilter::PrincipalId(PrincipalId::from_uuid(
                    principal_id,
                ))
            }
            _ => {
                return Err(BootError::BadRequest(
                    "exactly one of subjectRef or principalId is required".into(),
                ))
            }
        };
        match self
            .query_bus
            .execute(ListDirectoryMembershipProjections {
                organization_id,
                filter,
            })
            .await?
        {
            Ok(bindings) => BootResponse::json(
                &bindings
                    .into_iter()
                    .map(DirectoryMembershipProjectionBindingResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[put("/{organization_id}/directory-membership-projections", raw)]
    async fn replace(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ReplaceDirectoryMembershipProjectionRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(ReplaceDirectoryMembershipProjection {
                organization_id,
                subject_ref: body.subject_ref,
                principal_ids: body.principal_ids,
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
                    &DirectoryMembershipProjectionMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_directory_membership_projection_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn directory_membership_projection_controller_registers_routes() {
        let controller = directory_membership_projection_controller(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
        )
        .expect("directory membership projection nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/directory-membership-projections"
        );
        assert_eq!(routes[1].method(), HttpMethod::Put);
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/directory-membership-projections"
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
