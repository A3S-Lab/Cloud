use crate::modules::identity::application::commands::begin_recipient_contact_verification::BeginRecipientContactVerification;
use crate::modules::identity::application::commands::complete_recipient_contact_verification::CompleteRecipientContactVerification;
use crate::modules::identity::application::commands::revoke_recipient_contact::RevokeRecipientContact;
use crate::modules::identity::application::queries::get_recipient_contact::GetRecipientContact;
use crate::modules::identity::application::queries::list_recipient_contacts::ListRecipientContacts;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CompleteRecipientContactVerificationRequest, RecipientContactMutationResponse,
    RecipientContactResponse, RequestRecipientContactVerificationRequest,
    RevokeRecipientContactRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{OrganizationId, RecipientContactId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

pub fn recipient_contact_queries_controller(
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(RecipientContactQueriesController { bus: query_bus }).controller()
}

pub fn recipient_contact_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(RecipientContactCommandsController { bus: command_bus }).controller()
}

#[derive(Debug, Clone)]
struct RecipientContactQueriesController {
    bus: Arc<QueryBus>,
}

#[derive(Debug, Clone)]
struct RecipientContactCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl RecipientContactQueriesController {
    #[get("/{organization_id}/recipient-contacts", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id = organization_id(&request)?;
        let actor = actor(&request)?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListRecipientContacts {
                organization_id,
                actor_principal_id: actor.principal_id,
            })
            .await?
        {
            Ok(contacts) => BootResponse::json(
                &contacts
                    .into_iter()
                    .map(RecipientContactResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/recipient-contacts/{recipient_contact_id}", raw)]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let actor = actor(&request)?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetRecipientContact {
                organization_id: organization_id(&request)?,
                actor_principal_id: actor.principal_id,
                contact_id: contact_id(&request)?,
            })
            .await?
        {
            Ok(contact) => BootResponse::json(&RecipientContactResponse::from(contact)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl RecipientContactCommandsController {
    #[post("/{organization_id}/recipient-contacts", raw)]
    async fn begin(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RequestRecipientContactVerificationRequest = request.json_with_content_type()?;
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
            .execute(BeginRecipientContactVerification {
                organization_id: organization_id(&request)?,
                actor_principal_id: actor.principal_id,
                address: Zeroizing::new(body.address),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 202 };
                BootResponse::json_with_status(
                    status,
                    &RecipientContactMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/recipient-contacts/{recipient_contact_id}/verification",
        raw
    )]
    async fn complete(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CompleteRecipientContactVerificationRequest =
            request.json_with_content_type()?;
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
            .execute(CompleteRecipientContactVerification {
                organization_id: organization_id(&request)?,
                actor_principal_id: actor.principal_id,
                contact_id: contact_id(&request)?,
                proof: Zeroizing::new(body.proof),
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&RecipientContactMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/recipient-contacts/{recipient_contact_id}/revocation",
        raw
    )]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeRecipientContactRequest = request.json_with_content_type()?;
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
            .execute(RevokeRecipientContact {
                organization_id: organization_id(&request)?,
                actor_principal_id: actor.principal_id,
                contact_id: contact_id(&request)?,
                expected_version: body.expected_version,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&RecipientContactMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn organization_id(request: &BootRequest) -> Result<OrganizationId> {
    request
        .param_as::<Uuid>("organization_id")
        .map(OrganizationId::from_uuid)
}

fn contact_id(request: &BootRequest) -> Result<RecipientContactId> {
    request
        .param_as::<Uuid>("recipient_contact_id")
        .map(RecipientContactId::from_uuid)
}

#[cfg(test)]
mod nest_macro_recipient_contact_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::BTreeSet;

    #[test]
    fn recipient_contact_queries_register_scoped_guarded_gets_via_nest_macros() {
        let controller = recipient_contact_queries_controller(Arc::new(QueryBus::new()))
            .expect("recipient contact nest query controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/recipient-contacts".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}".into()
        )));
        for route in routes {
            assert_eq!(
                route
                    .metadata()
                    .get(AUTH_SCOPES_METADATA)
                    .cloned()
                    .expect("auth.scopes"),
                serde_json::json!([ApiTokenScope::CLOUD_READ])
            );
        }
    }

    #[test]
    fn recipient_contact_commands_register_scoped_guarded_posts_via_nest_macros() {
        let controller = recipient_contact_commands_controller(Arc::new(CommandBus::new()))
            .expect("recipient contact nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/recipient-contacts".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}/verification"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}/revocation"
                .into()
        )));
        for route in routes {
            assert_eq!(
                route
                    .metadata()
                    .get(AUTH_SCOPES_METADATA)
                    .cloned()
                    .expect("auth.scopes"),
                serde_json::json!([ApiTokenScope::IDENTITY_WRITE])
            );
        }
    }
}
