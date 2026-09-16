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
    GetWorkloadIdentityPolicyRevision, InspectCurrentTrustDomainProvider, ListTrustDomainRevisions,
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
    WorkloadIdentityProviderInspectionResponse,
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
    AUTH_SCOPES_METADATA, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result, controller, get, metadata, post,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn platform_rbac_queries_controller(query_bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(PlatformRbacQueriesController { bus: query_bus }).controller()
}

#[derive(Debug, Clone)]
struct PlatformRbacQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/platform")]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl PlatformRbacQueriesController {
    #[get("/role-policy", raw)]
    async fn get_current_role_policy(&self, request: BootRequest) -> Result<BootResponse> {
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get("/role-policy/revisions/{revision_id}", raw)]
    async fn get_role_policy_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let revision_id =
            PlatformRolePolicyRevisionId::from_uuid(request.param_as::<Uuid>("revision_id")?);
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get("/role-bindings/{binding_id}", raw)]
    async fn get_role_binding(&self, request: BootRequest) -> Result<BootResponse> {
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let binding_id = PlatformRoleBindingId::from_uuid(request.param_as::<Uuid>("binding_id")?);
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetPlatformRoleBinding {
                binding_id,
                actor_principal_id: actor.principal_id,
                credential_id: actor.credential_id,
                request_id,
            })
            .await?
        {
            Ok(binding) => BootResponse::json(&PlatformRoleBindingResponse::from(binding)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/principals/{principal_id}/role-binding", raw)]
    async fn get_principal_role_binding(&self, request: BootRequest) -> Result<BootResponse> {
        let authenticated = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&authenticated)?;
        let principal_id = PrincipalId::from_uuid(request.param_as::<Uuid>("principal_id")?);
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetPrincipalPlatformRoleBinding {
                principal_id,
                actor_principal_id: actor.principal_id,
                credential_id: actor.credential_id,
                request_id,
            })
            .await?
        {
            Ok(binding) => BootResponse::json(&PlatformRoleBindingResponse::from(binding)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

pub fn platform_rbac_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(PlatformRbacCommandsController { bus: command_bus }).controller()
}

#[derive(Debug, Clone)]
struct PlatformRbacCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/platform")]
#[metadata("auth.scopes", vec![ApiTokenScope::PLATFORM_WRITE])]
impl PlatformRbacCommandsController {
    #[post("/role-policy/revisions", raw)]
    async fn accept_role_policy(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AcceptPlatformRolePolicyRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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

    #[post("/role-bindings", raw)]
    async fn create_role_binding(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreatePlatformRoleBindingRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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

    #[post("/role-bindings/{binding_id}/role", raw)]
    async fn change_role_binding(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ChangePlatformRoleBindingRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let binding_id = PlatformRoleBindingId::from_uuid(request.param_as::<Uuid>("binding_id")?);
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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
            Ok(result) => BootResponse::json(&PlatformRoleBindingMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/role-bindings/{binding_id}/revocation", raw)]
    async fn revoke_role_binding(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ExpectedVersionRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let binding_id = PlatformRoleBindingId::from_uuid(request.param_as::<Uuid>("binding_id")?);
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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
            Ok(result) => BootResponse::json(&PlatformRoleBindingMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

pub fn workload_trust_queries_controller(query_bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(WorkloadTrustQueriesController { bus: query_bus }).controller()
}

#[derive(Debug, Clone)]
struct WorkloadTrustQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/platform")]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl WorkloadTrustQueriesController {
    #[get("/trust-domains/{trust_domain_id}", raw)]
    async fn get_trust_domains(&self, request: BootRequest) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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
            Ok(revision) => BootResponse::json(&TrustDomainRevisionResponse::from(revision)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/trust-domains/{trust_domain_id}/provider-inspection", raw)]
    async fn get_trust_domains_provider_inspection(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(InspectCurrentTrustDomainProvider {
                trust_domain_id: TrustDomainId::from_uuid(
                    request.param_as::<Uuid>("trust_domain_id")?,
                ),
                actor_principal_id: actor.principal_id,
                credential_id: actor.credential_id,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                BootResponse::json(&WorkloadIdentityProviderInspectionResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/trust-domains/{trust_domain_id}/revisions/{revision_id}", raw)]
    async fn get_trust_domains_revisions(&self, request: BootRequest) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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
            Ok(revision) => BootResponse::json(&TrustDomainRevisionResponse::from(revision)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/trust-domains/{trust_domain_id}/revisions", raw)]
    async fn get_trust_domains_revisions_2(&self, request: BootRequest) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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

    #[get(
        "/organizations/{organization_id}/workload-identity-policies/{policy_id}",
        raw
    )]
    async fn get_organizations_workload_identity_policies(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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
            Ok(revision) => {
                BootResponse::json(&WorkloadIdentityPolicyRevisionResponse::from(revision))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/organizations/{organization_id}/workloads/{workload_id}/identity-policy",
        raw
    )]
    async fn get_organizations_workloads_identity_policy(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetCurrentWorkloadIdentityPolicyForWorkload {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                workload_id: WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?),
                actor_principal_id: actor.principal_id,
                credential_id: actor.credential_id,
                request_id,
            })
            .await?
        {
            Ok(revision) => {
                BootResponse::json(&WorkloadIdentityPolicyRevisionResponse::from(revision))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions/{revision_id}",
        raw
    )]
    async fn get_organizations_workload_identity_policies_revisions(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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
            Ok(revision) => {
                BootResponse::json(&WorkloadIdentityPolicyRevisionResponse::from(revision))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions",
        raw
    )]
    async fn get_organizations_workload_identity_policies_revisions_2(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let request_id = request_id(&request)?;
        match self
            .bus
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
}

pub fn workload_trust_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(WorkloadTrustCommandsController { bus: command_bus }).controller()
}

#[derive(Debug, Clone)]
struct WorkloadTrustCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/platform")]
#[metadata("auth.scopes", vec![ApiTokenScope::PLATFORM_WRITE])]
impl WorkloadTrustCommandsController {
    #[post("/trust-domains/{trust_domain_id}/revisions", raw)]
    async fn post_trust_domains_revisions(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AcceptTrustDomainRevisionRequest = request.json_with_content_type()?;
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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

    #[post(
        "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions",
        raw
    )]
    async fn post_organizations_workload_identity_policies_revisions(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let body: AcceptWorkloadIdentityPolicyRevisionRequest = request.json_with_content_type()?;
        let actor = authenticated_credential_actor(&request.require_auth_principal()?)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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
}

pub fn tenant_support_query_controller(query_bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(TenantSupportQueryController { bus: query_bus }).controller()
}

#[derive(Debug, Clone)]
struct TenantSupportQueryController {
    bus: Arc<QueryBus>,
}

#[controller("/platform")]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl TenantSupportQueryController {
    #[get("/tenant-support-grants/{grant_id}", raw)]
    async fn get_tenant_support_grants(&self, request: BootRequest) -> Result<BootResponse> {
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let grant_id = TenantSupportGrantId::from_uuid(request.param_as::<Uuid>("grant_id")?);
        let request_id = request_id(&request)?;
        match self
            .bus
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
}

pub fn tenant_support_commands_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(TenantSupportCommandsController { bus: command_bus }).controller()
}

#[derive(Debug, Clone)]
struct TenantSupportCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/platform")]
#[metadata("auth.scopes", vec![ApiTokenScope::PLATFORM_WRITE])]
impl TenantSupportCommandsController {
    #[post("/tenant-support-grants", raw)]
    async fn post_tenant_support_grants(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ProposeTenantSupportGrantRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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

    #[post("/tenant-support-grants/{grant_id}/approvals", raw)]
    async fn post_tenant_support_grants_approvals(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let body: ApproveTenantSupportGrantRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let grant_id = TenantSupportGrantId::from_uuid(request.param_as::<Uuid>("grant_id")?);
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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
            Ok(result) => {
                BootResponse::json(&TenantSupportGrantApprovalMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/tenant-support-grants/{grant_id}/revocation", raw)]
    async fn post_tenant_support_grants_revocation(
        &self,
        request: BootRequest,
    ) -> Result<BootResponse> {
        let body: ExpectedVersionRequest = request.json_with_content_type()?;
        let principal = request.require_auth_principal()?;
        let actor = authenticated_credential_actor(&principal)?;
        let grant_id = TenantSupportGrantId::from_uuid(request.param_as::<Uuid>("grant_id")?);
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .bus
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
            Ok(result) => BootResponse::json(&TenantSupportGrantMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
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

#[cfg(test)]
mod nest_macro_privileged_management_controller_tests {
    use super::*;

    #[test]
    fn privileged_management_controllers_register_scoped_routes_via_nest_macros() {
        let rbac_q = platform_rbac_queries_controller(Arc::new(QueryBus::new()))
            .expect("platform rbac queries");
        assert_eq!(rbac_q.prefix(), "/platform");
        assert_eq!(rbac_q.routes().len(), 4);
        assert!(rbac_q.metadata().get(AUTH_SCOPES_METADATA).is_some());

        let rbac_c = platform_rbac_commands_controller(Arc::new(CommandBus::new()))
            .expect("platform rbac commands");
        assert_eq!(rbac_c.routes().len(), 4);

        let trust_q = workload_trust_queries_controller(Arc::new(QueryBus::new()))
            .expect("workload trust queries");
        assert_eq!(trust_q.routes().len(), 8);

        let trust_c = workload_trust_commands_controller(Arc::new(CommandBus::new()))
            .expect("workload trust commands");
        assert_eq!(trust_c.routes().len(), 2);

        let support_q = tenant_support_query_controller(Arc::new(QueryBus::new()))
            .expect("tenant support query");
        assert_eq!(support_q.routes().len(), 1);

        let support_c = tenant_support_commands_controller(Arc::new(CommandBus::new()))
            .expect("tenant support commands");
        assert_eq!(support_c.routes().len(), 3);
    }
}
