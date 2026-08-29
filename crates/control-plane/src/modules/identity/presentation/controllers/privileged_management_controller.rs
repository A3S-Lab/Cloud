use crate::modules::identity::application::commands::manage_platform_rbac::{
    AcceptPlatformRolePolicy, ChangePlatformRoleBinding, CreatePlatformRoleBinding,
    RevokePlatformRoleBinding,
};
use crate::modules::identity::application::commands::manage_tenant_support::{
    ApproveTenantSupportGrant, ProposeTenantSupportGrant, RevokeTenantSupportGrant,
};
use crate::modules::identity::application::queries::read_platform_rbac::{
    GetCurrentPlatformRolePolicy, GetPlatformRoleBinding, GetPlatformRolePolicyRevision,
    GetPrincipalPlatformRoleBinding,
};
use crate::modules::identity::application::queries::read_tenant_support::GetTenantSupportGrant;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    AcceptPlatformRolePolicyRequest, ApproveTenantSupportGrantRequest,
    ChangePlatformRoleBindingRequest, CreatePlatformRoleBindingRequest, ExpectedVersionRequest,
    PlatformRoleBindingMutationResponse, PlatformRoleBindingResponse,
    PlatformRolePolicyMutationResponse, PlatformRolePolicyResponse,
    ProposeTenantSupportGrantRequest, TenantSupportGrantApprovalMutationResponse,
    TenantSupportGrantMutationResponse, TenantSupportGrantProposalMutationResponse,
    TenantSupportGrantResponse,
};
use crate::modules::identity::presentation::request_context::{
    authenticated_credential_actor, mutation_identity, request_id,
};
use crate::modules::shared_kernel::domain::{
    PlatformRoleBindingId, PlatformRolePolicyRevisionId, PrincipalId, TenantSupportGrantId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn platform_rbac_queries_controller(query_bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let current_policy_bus = Arc::clone(&query_bus);
    let policy_revision_bus = Arc::clone(&query_bus);
    let binding_bus = Arc::clone(&query_bus);
    ControllerDefinition::new("/platform")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get("/role-policy", move |request: BootRequest| {
            let bus = Arc::clone(&current_policy_bus);
            async move {
                let principal = request.require_auth_principal()?;
                let actor = authenticated_credential_actor(&principal)?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetCurrentPlatformRolePolicy {
                        actor_principal_id: actor.principal_id,
                        credential_id: actor.credential_id,
                        request_id,
                    })
                    .await?
                {
                    Ok(policy) => BootResponse::json(&PlatformRolePolicyResponse::from(policy)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .get(
            "/role-policy/revisions/{revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&policy_revision_bus);
                async move {
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let revision_id = PlatformRolePolicyRevisionId::from_uuid(
                        request.param_as::<Uuid>("revision_id")?,
                    );
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPlatformRolePolicyRevision {
                            revision_id,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(policy) => BootResponse::json(&PlatformRolePolicyResponse::from(policy)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/role-bindings/{binding_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&binding_bus);
                async move {
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let binding_id =
                        PlatformRoleBindingId::from_uuid(request.param_as::<Uuid>("binding_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPlatformRoleBinding {
                            binding_id,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(binding) => {
                            BootResponse::json(&PlatformRoleBindingResponse::from(binding))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/principals/{principal_id}/role-binding",
            move |request: BootRequest| {
                let bus = Arc::clone(&query_bus);
                async move {
                    let authenticated = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&authenticated)?;
                    let principal_id =
                        PrincipalId::from_uuid(request.param_as::<Uuid>("principal_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPrincipalPlatformRoleBinding {
                            principal_id,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(binding) => {
                            BootResponse::json(&PlatformRoleBindingResponse::from(binding))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn platform_rbac_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let accept_policy_bus = Arc::clone(&command_bus);
    let create_binding_bus = Arc::clone(&command_bus);
    let change_binding_bus = Arc::clone(&command_bus);
    ControllerDefinition::new("/platform")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::PLATFORM_WRITE])?
        .post("/role-policy/revisions", move |request: BootRequest| {
            let bus = Arc::clone(&accept_policy_bus);
            async move {
                let body: AcceptPlatformRolePolicyRequest = request.json_with_content_type()?;
                let principal = request.require_auth_principal()?;
                let actor = authenticated_credential_actor(&principal)?;
                let (idempotency_key, request_id) = mutation_identity(&request)?;
                match bus
                    .execute(AcceptPlatformRolePolicy {
                        canonical_acl: body.canonical_acl,
                        revision_number: body.revision_number,
                        expected_current_revision_id: PlatformRolePolicyRevisionId::from_uuid(
                            body.expected_current_revision_id,
                        ),
                        actor_principal_id: actor.principal_id,
                        credential_id: actor.credential_id,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(
                            status,
                            &PlatformRolePolicyMutationResponse::from(result),
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post("/role-bindings", move |request: BootRequest| {
            let bus = Arc::clone(&create_binding_bus);
            async move {
                let body: CreatePlatformRoleBindingRequest = request.json_with_content_type()?;
                let principal = request.require_auth_principal()?;
                let actor = authenticated_credential_actor(&principal)?;
                let (idempotency_key, request_id) = mutation_identity(&request)?;
                match bus
                    .execute(CreatePlatformRoleBinding {
                        principal_id: PrincipalId::from_uuid(body.principal_id),
                        role: body.role,
                        expected_policy_revision_id: PlatformRolePolicyRevisionId::from_uuid(
                            body.expected_policy_revision_id,
                        ),
                        actor_principal_id: actor.principal_id,
                        credential_id: actor.credential_id,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(
                            status,
                            &PlatformRoleBindingMutationResponse::from(result),
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post(
            "/role-bindings/{binding_id}/role",
            move |request: BootRequest| {
                let bus = Arc::clone(&change_binding_bus);
                async move {
                    let body: ChangePlatformRoleBindingRequest =
                        request.json_with_content_type()?;
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let binding_id =
                        PlatformRoleBindingId::from_uuid(request.param_as::<Uuid>("binding_id")?);
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(ChangePlatformRoleBinding {
                            binding_id,
                            role: body.role,
                            expected_version: body.expected_version,
                            expected_policy_revision_id: PlatformRolePolicyRevisionId::from_uuid(
                                body.expected_policy_revision_id,
                            ),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            BootResponse::json(&PlatformRoleBindingMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/role-bindings/{binding_id}/revocation",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: ExpectedVersionRequest = request.json_with_content_type()?;
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let binding_id =
                        PlatformRoleBindingId::from_uuid(request.param_as::<Uuid>("binding_id")?);
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokePlatformRoleBinding {
                            binding_id,
                            expected_version: body.expected_version,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            BootResponse::json(&PlatformRoleBindingMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn tenant_support_query_controller(query_bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/platform")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/tenant-support-grants/{grant_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&query_bus);
                async move {
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let grant_id =
                        TenantSupportGrantId::from_uuid(request.param_as::<Uuid>("grant_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetTenantSupportGrant {
                            grant_id,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(record) => BootResponse::json(&TenantSupportGrantResponse::from(record)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn tenant_support_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let propose_bus = Arc::clone(&command_bus);
    let approve_bus = Arc::clone(&command_bus);
    ControllerDefinition::new("/platform")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::PLATFORM_WRITE])?
        .post("/tenant-support-grants", move |request: BootRequest| {
            let bus = Arc::clone(&propose_bus);
            async move {
                let body: ProposeTenantSupportGrantRequest = request.json_with_content_type()?;
                let principal = request.require_auth_principal()?;
                let actor = authenticated_credential_actor(&principal)?;
                let (idempotency_key, request_id) = mutation_identity(&request)?;
                match bus
                    .execute(ProposeTenantSupportGrant {
                        canonical_acl: body.canonical_acl,
                        actor_principal_id: actor.principal_id,
                        credential_id: actor.credential_id,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 202 };
                        BootResponse::json_with_status(
                            status,
                            &TenantSupportGrantProposalMutationResponse::from(result),
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .post(
            "/tenant-support-grants/{grant_id}/approvals",
            move |request: BootRequest| {
                let bus = Arc::clone(&approve_bus);
                async move {
                    let body: ApproveTenantSupportGrantRequest =
                        request.json_with_content_type()?;
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let grant_id =
                        TenantSupportGrantId::from_uuid(request.param_as::<Uuid>("grant_id")?);
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(ApproveTenantSupportGrant {
                            grant_id,
                            expected_contract_digest: body.expected_contract_digest,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json(
                            &TenantSupportGrantApprovalMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/tenant-support-grants/{grant_id}/revocation",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: ExpectedVersionRequest = request.json_with_content_type()?;
                    let principal = request.require_auth_principal()?;
                    let actor = authenticated_credential_actor(&principal)?;
                    let grant_id =
                        TenantSupportGrantId::from_uuid(request.param_as::<Uuid>("grant_id")?);
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokeTenantSupportGrant {
                            grant_id,
                            expected_version: body.expected_version,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            BootResponse::json(&TenantSupportGrantMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
