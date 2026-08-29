use crate::modules::identity::application::commands::manage_platform_rbac::{
    AcceptPlatformRolePolicy, ChangePlatformRoleBinding, CreatePlatformRoleBinding,
    RevokePlatformRoleBinding,
};
use crate::modules::identity::application::commands::manage_tenant_support::{
    ApproveTenantSupportGrant, ProposeTenantSupportGrant, RevokeTenantSupportGrant,
};
use crate::modules::identity::application::commands::manage_workload_trust::{
    AcceptTrustDomainRevision, AcceptWorkloadIdentityPolicyRevision,
};
use crate::modules::identity::application::queries::read_platform_rbac::{
    GetCurrentPlatformRolePolicy, GetPlatformRoleBinding, GetPlatformRolePolicyRevision,
    GetPrincipalPlatformRoleBinding,
};
use crate::modules::identity::application::queries::read_tenant_support::GetTenantSupportGrant;
use crate::modules::identity::application::queries::read_workload_trust::{
    GetCurrentTrustDomain, GetCurrentWorkloadIdentityPolicy,
    GetCurrentWorkloadIdentityPolicyForWorkload, GetTrustDomainRevision,
    GetWorkloadIdentityPolicyRevision, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions,
};
use crate::modules::identity::domain::repositories::{
    DEFAULT_WORKLOAD_IDENTITY_REVISIONS_PAGE, MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    AcceptPlatformRolePolicyRequest, AcceptTrustDomainRevisionRequest,
    AcceptWorkloadIdentityPolicyRevisionRequest, ApproveTenantSupportGrantRequest,
    ChangePlatformRoleBindingRequest, CreatePlatformRoleBindingRequest, ExpectedVersionRequest,
    PlatformRoleBindingMutationResponse, PlatformRoleBindingResponse,
    PlatformRolePolicyMutationResponse, PlatformRolePolicyResponse,
    ProposeTenantSupportGrantRequest, TenantSupportGrantApprovalMutationResponse,
    TenantSupportGrantMutationResponse, TenantSupportGrantProposalMutationResponse,
    TenantSupportGrantResponse, TrustDomainRevisionMutationResponse, TrustDomainRevisionResponse,
    WorkloadIdentityPolicyRevisionMutationResponse, WorkloadIdentityPolicyRevisionResponse,
};
use crate::modules::identity::presentation::request_context::{
    authenticated_credential_actor, mutation_identity, request_id,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, PlatformRoleBindingId, PlatformRolePolicyRevisionId, PrincipalId,
    TenantSupportGrantId, TrustDomainId, TrustDomainRevisionId, WorkloadId,
    WorkloadIdentityPolicyId, WorkloadIdentityPolicyRevisionId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
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

pub fn workload_trust_queries_controller(query_bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let current_trust_domain_bus = Arc::clone(&query_bus);
    let trust_domain_revision_bus = Arc::clone(&query_bus);
    let trust_domain_revisions_bus = Arc::clone(&query_bus);
    let current_policy_bus = Arc::clone(&query_bus);
    let workload_policy_bus = Arc::clone(&query_bus);
    let policy_revision_bus = Arc::clone(&query_bus);
    ControllerDefinition::new("/platform")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get(
            "/trust-domains/{trust_domain_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&current_trust_domain_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetCurrentTrustDomain {
                            trust_domain_id: TrustDomainId::from_uuid(
                                request.param_as::<Uuid>("trust_domain_id")?,
                            ),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revision) => {
                            BootResponse::json(&TrustDomainRevisionResponse::from(revision))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/trust-domains/{trust_domain_id}/revisions/{revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&trust_domain_revision_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetTrustDomainRevision {
                            trust_domain_id: TrustDomainId::from_uuid(
                                request.param_as::<Uuid>("trust_domain_id")?,
                            ),
                            revision_id: TrustDomainRevisionId::from_uuid(
                                request.param_as::<Uuid>("revision_id")?,
                            ),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revision) => {
                            BootResponse::json(&TrustDomainRevisionResponse::from(revision))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/trust-domains/{trust_domain_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&trust_domain_revisions_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListTrustDomainRevisions {
                            trust_domain_id: TrustDomainId::from_uuid(
                                request.param_as::<Uuid>("trust_domain_id")?,
                            ),
                            limit: workload_trust_limit(&request)?,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revisions) => BootResponse::json(
                            &revisions
                                .into_iter()
                                .map(TrustDomainRevisionResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&current_policy_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetCurrentWorkloadIdentityPolicy {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            policy_id: WorkloadIdentityPolicyId::from_uuid(
                                request.param_as::<Uuid>("policy_id")?,
                            ),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json(
                            &WorkloadIdentityPolicyRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/organizations/{organization_id}/workloads/{workload_id}/identity-policy",
            move |request: BootRequest| {
                let bus = Arc::clone(&workload_policy_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetCurrentWorkloadIdentityPolicyForWorkload {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            workload_id: WorkloadId::from_uuid(
                                request.param_as::<Uuid>("workload_id")?,
                            ),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json(
                            &WorkloadIdentityPolicyRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions/{revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&policy_revision_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetWorkloadIdentityPolicyRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            policy_id: WorkloadIdentityPolicyId::from_uuid(
                                request.param_as::<Uuid>("policy_id")?,
                            ),
                            revision_id: WorkloadIdentityPolicyRevisionId::from_uuid(
                                request.param_as::<Uuid>("revision_id")?,
                            ),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revision) => BootResponse::json(
                            &WorkloadIdentityPolicyRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&query_bus);
                async move {
                    let actor = authenticated_credential_actor(
                        &request.require_auth_principal()?,
                    )?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListWorkloadIdentityPolicyRevisions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            policy_id: WorkloadIdentityPolicyId::from_uuid(
                                request.param_as::<Uuid>("policy_id")?,
                            ),
                            limit: workload_trust_limit(&request)?,
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            request_id,
                        })
                        .await?
                    {
                        Ok(revisions) => BootResponse::json(
                            &revisions
                                .into_iter()
                                .map(WorkloadIdentityPolicyRevisionResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

pub fn workload_trust_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let trust_domain_bus = Arc::clone(&command_bus);
    ControllerDefinition::new("/platform")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::PLATFORM_WRITE])?
        .post(
            "/trust-domains/{trust_domain_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&trust_domain_bus);
                async move {
                    let body: AcceptTrustDomainRevisionRequest =
                        request.json_with_content_type()?;
                    let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(AcceptTrustDomainRevision {
                            trust_domain_id: TrustDomainId::from_uuid(
                                request.param_as::<Uuid>("trust_domain_id")?,
                            ),
                            canonical_acl: body.canonical_acl,
                            revision_number: body.revision_number,
                            expected_previous_revision_id: body
                                .expected_previous_revision_id
                                .map(TrustDomainRevisionId::from_uuid),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &TrustDomainRevisionMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: AcceptWorkloadIdentityPolicyRevisionRequest =
                        request.json_with_content_type()?;
                    let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(AcceptWorkloadIdentityPolicyRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            policy_id: WorkloadIdentityPolicyId::from_uuid(
                                request.param_as::<Uuid>("policy_id")?,
                            ),
                            canonical_acl: body.canonical_acl,
                            revision_number: body.revision_number,
                            expected_previous_revision_id: body
                                .expected_previous_revision_id
                                .map(WorkloadIdentityPolicyRevisionId::from_uuid),
                            actor_principal_id: actor.principal_id,
                            credential_id: actor.credential_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &WorkloadIdentityPolicyRevisionMutationResponse::from(result),
                        ),
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

fn workload_trust_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_WORKLOAD_IDENTITY_REVISIONS_PAGE);
    if limit == 0 || limit > MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE}"
        )));
    }
    Ok(limit)
}
