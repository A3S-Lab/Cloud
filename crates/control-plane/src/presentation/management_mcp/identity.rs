use super::arguments::EmptyArguments;
use super::tool_result;
use crate::modules::identity::domain::entities::IdentityPrincipalKind;
use crate::modules::identity::presentation::{
    MembershipInvitationAcceptanceResponse, MembershipInvitationMutationResponse,
    MembershipInvitationResponse, MembershipMutationResponse, MembershipResponse,
    RecipientContactMutationResponse, RecipientContactResponse, ResourceGrantMutationResponse,
    ResourceGrantResponse, ResourceGrantScopeDto,
};
use crate::modules::identity::{
    AcceptMembershipInvitation, ChangeMembershipRole, CreateMembership, CreateMembershipInvitation,
    CreateResourceGrant, GetMembership, GetMembershipInvitation, GetRecipientContact,
    GetResourceGrant, ListMembershipInvitations, ListMemberships, ListMyMembershipInvitations,
    ListRecipientContacts, ListResourceGrants, RevokeMembership, RevokeMembershipInvitation,
    RevokeRecipientContact, RevokeResourceGrant,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    MembershipId, MembershipInvitationId, OrganizationId, PrincipalId, RecipientContactId,
    ResourceGrantId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipArguments {
    membership_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMembershipArguments {
    principal_kind: IdentityPrincipalKind,
    name: String,
    role: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeMembershipRoleArguments {
    membership_id: Uuid,
    role: String,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeMembershipArguments {
    membership_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipInvitationArguments {
    invitation_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMembershipInvitationArguments {
    principal_id: Uuid,
    role: String,
    expires_at: DateTime<Utc>,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembershipInvitationMutationArguments {
    invitation_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListResourceGrantsArguments {
    membership_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceGrantArguments {
    resource_grant_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateResourceGrantArguments {
    membership_id: Uuid,
    scope: ResourceGrantScopeDto,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeResourceGrantArguments {
    resource_grant_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipientContactArguments {
    recipient_contact_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeRecipientContactArguments {
    recipient_contact_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    idempotency_key: String,
}

pub async fn list_memberships(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus.execute(ListMemberships { organization_id }).await? {
        Ok(memberships) => tool_result::success(
            200,
            memberships
                .into_iter()
                .map(MembershipResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_membership(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: MembershipArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetMembership {
            organization_id,
            membership_id: MembershipId::from_uuid(arguments.membership_id),
        })
        .await?
    {
        Ok(membership) => {
            tool_result::success(200, MembershipResponse::from(membership), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_membership(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateMembershipArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateMembership {
            organization_id,
            principal_kind: arguments.principal_kind.as_str().into(),
            name: arguments.name,
            role: arguments.role,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(status, MembershipMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn change_membership_role(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ChangeMembershipRoleArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ChangeMembershipRole {
            organization_id,
            membership_id: MembershipId::from_uuid(arguments.membership_id),
            role: arguments.role,
            expected_version: arguments.expected_version,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            tool_result::success(200, MembershipMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revoke_membership(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: RevokeMembershipArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokeMembership {
            organization_id,
            membership_id: MembershipId::from_uuid(arguments.membership_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            tool_result::success(200, MembershipMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_membership_invitations(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListMembershipInvitations { organization_id })
        .await?
    {
        Ok(invitations) => tool_result::success(
            200,
            invitations
                .into_iter()
                .map(MembershipInvitationResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_membership_invitation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: MembershipInvitationArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetMembershipInvitation {
            organization_id,
            invitation_id: MembershipInvitationId::from_uuid(arguments.invitation_id),
        })
        .await?
    {
        Ok(invitation) => tool_result::success(
            200,
            MembershipInvitationResponse::from(invitation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_membership_invitation(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateMembershipInvitationArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateMembershipInvitation {
            organization_id,
            principal_id: PrincipalId::from_uuid(arguments.principal_id),
            role: arguments.role,
            expires_at: arguments.expires_at,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(
                status,
                MembershipInvitationMutationResponse::from(result),
                request_id,
            )
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revoke_membership_invitation(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: MembershipInvitationMutationArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokeMembershipInvitation {
            organization_id,
            invitation_id: MembershipInvitationId::from_uuid(arguments.invitation_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            MembershipInvitationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_my_membership_invitations(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListMyMembershipInvitations {
            principal_id: actor_principal_id,
        })
        .await?
    {
        Ok(invitations) => tool_result::success(
            200,
            invitations
                .into_iter()
                .map(MembershipInvitationResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn accept_membership_invitation(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    arguments: MembershipInvitationMutationArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AcceptMembershipInvitation {
            invitation_id: MembershipInvitationId::from_uuid(arguments.invitation_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(
                status,
                MembershipInvitationAcceptanceResponse::from(result),
                request_id,
            )
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_resource_grants(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListResourceGrantsArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListResourceGrants {
            organization_id,
            membership_id: Some(MembershipId::from_uuid(arguments.membership_id)),
        })
        .await?
    {
        Ok(grants) => tool_result::success(
            200,
            grants
                .into_iter()
                .map(ResourceGrantResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_resource_grant(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ResourceGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetResourceGrant {
            organization_id,
            resource_grant_id: ResourceGrantId::from_uuid(arguments.resource_grant_id),
        })
        .await?
    {
        Ok(grant) => tool_result::success(200, ResourceGrantResponse::from(grant), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_resource_grant(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateResourceGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    let scope = match arguments.scope.try_into() {
        Ok(scope) => scope,
        Err(error) => {
            return tool_result::application_error(ApplicationError::Invalid(error), request_id)
        }
    };
    match bus
        .execute(CreateResourceGrant {
            organization_id,
            membership_id: MembershipId::from_uuid(arguments.membership_id),
            scope,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(
                status,
                ResourceGrantMutationResponse::from(result),
                request_id,
            )
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revoke_resource_grant(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: RevokeResourceGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokeResourceGrant {
            organization_id,
            resource_grant_id: ResourceGrantId::from_uuid(arguments.resource_grant_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            tool_result::success(200, ResourceGrantMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_recipient_contacts(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListRecipientContacts {
            organization_id,
            actor_principal_id,
        })
        .await?
    {
        Ok(contacts) => tool_result::success(
            200,
            contacts
                .into_iter()
                .map(RecipientContactResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_recipient_contact(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: RecipientContactArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetRecipientContact {
            organization_id,
            actor_principal_id,
            contact_id: RecipientContactId::from_uuid(arguments.recipient_contact_id),
        })
        .await?
    {
        Ok(contact) => {
            tool_result::success(200, RecipientContactResponse::from(contact), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revoke_recipient_contact(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: RevokeRecipientContactArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokeRecipientContact {
            organization_id,
            actor_principal_id,
            contact_id: RecipientContactId::from_uuid(arguments.recipient_contact_id),
            expected_version: arguments.expected_version,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            RecipientContactMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
