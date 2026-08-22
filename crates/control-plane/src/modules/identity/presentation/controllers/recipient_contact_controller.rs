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
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

pub fn recipient_contact_queries_controller(
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&query_bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/{organization_id}/recipient-contacts",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let actor = actor(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
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
            },
        )?
        .get(
            "/{organization_id}/recipient-contacts/{recipient_contact_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&query_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let contact_id = RecipientContactId::from_uuid(
                        request.param_as::<Uuid>("recipient_contact_id")?,
                    );
                    let actor = actor(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetRecipientContact {
                            organization_id,
                            actor_principal_id: actor.principal_id,
                            contact_id,
                        })
                        .await?
                    {
                        Ok(contact) => BootResponse::json(&RecipientContactResponse::from(contact)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn recipient_contact_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let begin_bus = Arc::clone(&command_bus);
    let complete_bus = Arc::clone(&command_bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::IDENTITY_WRITE])?
        .post(
            "/{organization_id}/recipient-contacts",
            move |request: BootRequest| {
                let bus = Arc::clone(&begin_bus);
                async move {
                    let body: RequestRecipientContactVerificationRequest =
                        request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(BeginRecipientContactVerification {
                            organization_id,
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
            },
        )?
        .post(
            "/{organization_id}/recipient-contacts/{recipient_contact_id}/verification",
            move |request: BootRequest| {
                let bus = Arc::clone(&complete_bus);
                async move {
                    let body: CompleteRecipientContactVerificationRequest =
                        request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let contact_id = RecipientContactId::from_uuid(
                        request.param_as::<Uuid>("recipient_contact_id")?,
                    );
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(CompleteRecipientContactVerification {
                            organization_id,
                            actor_principal_id: actor.principal_id,
                            contact_id,
                            proof: Zeroizing::new(body.proof),
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            BootResponse::json(&RecipientContactMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/recipient-contacts/{recipient_contact_id}/revocation",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: RevokeRecipientContactRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let contact_id = RecipientContactId::from_uuid(
                        request.param_as::<Uuid>("recipient_contact_id")?,
                    );
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokeRecipientContact {
                            organization_id,
                            actor_principal_id: actor.principal_id,
                            contact_id,
                            expected_version: body.expected_version,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            BootResponse::json(&RecipientContactMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
