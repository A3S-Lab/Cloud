use super::arguments::EmptyArguments;
use super::tool_result;
use crate::modules::identity::presentation::{MembershipMutationResponse, MembershipResponse};
use crate::modules::identity::{
    ChangeMembershipRole, CreateServiceMembership, GetMembership, ListMemberships, RevokeMembership,
};
use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId, PrincipalId};
use a3s_boot::{CommandBus, QueryBus, Result};
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
pub struct CreateServiceMembershipArguments {
    name: String,
    role: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeMembershipRoleArguments {
    membership_id: Uuid,
    role: String,
    expected_version: u64,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeMembershipArguments {
    membership_id: Uuid,
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

pub async fn create_service_membership(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    actor_is_platform_admin: bool,
    arguments: CreateServiceMembershipArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateServiceMembership {
            organization_id,
            name: arguments.name,
            role: arguments.role,
            actor_principal_id,
            actor_is_platform_admin,
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
    actor_is_platform_admin: bool,
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
            actor_is_platform_admin,
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
    actor_is_platform_admin: bool,
    arguments: RevokeMembershipArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokeMembership {
            organization_id,
            membership_id: MembershipId::from_uuid(arguments.membership_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            actor_is_platform_admin,
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
