pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod published;

pub use application::{
    ActiveHumanMembershipScope, IActiveHumanMembershipQueryPort,
    IRecipientContactVerificationDispatcher, IWorkloadRuntimeEvidenceCandidatePort,
    IWorkloadRuntimeExecutionAuthorizationQueryPort,
    RecipientContactVerificationDeliveryDispatcher, RecipientContactVerificationDispatchResult,
    RecordWorkloadRuntimeEvidence, WorkloadRuntimeEvidenceRecorder, WorkloadRuntimeEvidenceRequest,
    WorkloadRuntimeExecutionAuthorizationQuery, WorkloadRuntimeExecutionAuthorizationQueryService,
};

pub use application::commands::accept_membership_invitation::{
    AcceptMembershipInvitation, AcceptMembershipInvitationHandler,
};
pub use application::commands::authorize_privileged_access::{
    AuthorizePrivilegedAccess, AuthorizePrivilegedAccessHandler,
};
pub use application::commands::begin_oidc_flow::{
    BeginOidcFlow, BeginOidcFlowHandler, BeginOidcFlowResult,
};
pub use application::commands::begin_recipient_contact_verification::{
    BeginRecipientContactVerification, BeginRecipientContactVerificationHandler,
};
pub use application::commands::bootstrap_identity::{
    BootstrapIdentity, BootstrapIdentityHandler, BootstrapIdentityResult,
};
pub use application::commands::change_membership_role::{
    ChangeMembershipRole, ChangeMembershipRoleHandler,
};
pub use application::commands::complete_oidc_flow::{
    CompleteOidcFlow, CompleteOidcFlowHandler, CompleteOidcFlowResult,
};
pub use application::commands::complete_recipient_contact_verification::{
    CompleteRecipientContactVerification, CompleteRecipientContactVerificationHandler,
};
pub use application::commands::create_api_token::{
    CreateApiToken, CreateApiTokenHandler, CreateApiTokenResult,
};
pub use application::commands::create_membership::{CreateMembership, CreateMembershipHandler};
pub use application::commands::create_membership_invitation::{
    CreateMembershipInvitation, CreateMembershipInvitationHandler,
};
pub use application::commands::create_organization::{
    CreateOrganization, CreateOrganizationHandler, CreateOrganizationResult,
};
pub use application::commands::create_resource_grant::{
    CreateResourceGrant, CreateResourceGrantHandler,
};
pub use application::commands::manage_platform_rbac::{
    AcceptPlatformRolePolicy, AcceptPlatformRolePolicyHandler, ChangePlatformRoleBinding,
    ChangePlatformRoleBindingHandler, CreatePlatformRoleBinding, CreatePlatformRoleBindingHandler,
    RevokePlatformRoleBinding, RevokePlatformRoleBindingHandler,
};
pub use application::commands::manage_tenant_support::{
    ApproveTenantSupportGrant, ApproveTenantSupportGrantHandler, ProposeTenantSupportGrant,
    ProposeTenantSupportGrantHandler, RevokeTenantSupportGrant, RevokeTenantSupportGrantHandler,
};
pub use application::commands::manage_workload_trust::{
    AcceptTrustDomainRevision, AcceptTrustDomainRevisionHandler,
    AcceptWorkloadIdentityPolicyRevision, AcceptWorkloadIdentityPolicyRevisionHandler,
};
pub use application::commands::revoke_api_token::{
    RevokeApiToken, RevokeApiTokenHandler, RevokeApiTokenResult,
};
pub use application::commands::revoke_membership::{RevokeMembership, RevokeMembershipHandler};
pub use application::commands::revoke_membership_invitation::{
    RevokeMembershipInvitation, RevokeMembershipInvitationHandler,
};
pub use application::commands::revoke_recipient_contact::{
    RevokeRecipientContact, RevokeRecipientContactHandler,
};
pub use application::commands::revoke_resource_grant::{
    RevokeResourceGrant, RevokeResourceGrantHandler,
};
pub use application::queries::get_api_token::{GetApiToken, GetApiTokenHandler};
pub use application::queries::get_membership::{GetMembership, GetMembershipHandler};
pub use application::queries::get_membership_invitation::{
    GetMembershipInvitation, GetMembershipInvitationHandler,
};
pub use application::queries::get_recipient_contact::{
    GetRecipientContact, GetRecipientContactHandler,
};
pub use application::queries::get_resource_grant::{GetResourceGrant, GetResourceGrantHandler};
pub use application::queries::list_api_tokens::{ListApiTokens, ListApiTokensHandler};
pub use application::queries::list_membership_invitations::{
    ListMembershipInvitations, ListMembershipInvitationsHandler,
};
pub use application::queries::list_memberships::{ListMemberships, ListMembershipsHandler};
pub use application::queries::list_my_membership_invitations::{
    ListMyMembershipInvitations, ListMyMembershipInvitationsHandler,
};
pub use application::queries::list_organizations::{ListOrganizations, ListOrganizationsHandler};
pub use application::queries::list_recipient_contacts::{
    ListRecipientContacts, ListRecipientContactsHandler,
};
pub use application::queries::list_resource_grants::{
    ListResourceGrants, ListResourceGrantsHandler,
};
pub use application::queries::read_platform_rbac::{
    GetCurrentPlatformRolePolicy, GetCurrentPlatformRolePolicyHandler, GetPlatformRoleBinding,
    GetPlatformRoleBindingHandler, GetPlatformRolePolicyRevision,
    GetPlatformRolePolicyRevisionHandler, GetPrincipalPlatformRoleBinding,
    GetPrincipalPlatformRoleBindingHandler,
};
pub use application::queries::read_tenant_support::{
    GetTenantSupportGrant, GetTenantSupportGrantHandler,
};
pub use application::queries::read_workload_trust::{
    GetCurrentTrustDomain, GetCurrentTrustDomainHandler, GetCurrentWorkloadIdentityPolicy,
    GetCurrentWorkloadIdentityPolicyForWorkload,
    GetCurrentWorkloadIdentityPolicyForWorkloadHandler, GetCurrentWorkloadIdentityPolicyHandler,
    GetTrustDomainRevision, GetTrustDomainRevisionHandler, GetWorkloadIdentityPolicyRevision,
    GetWorkloadIdentityPolicyRevisionHandler, InspectCurrentTrustDomainProvider,
    InspectCurrentTrustDomainProviderHandler, ListTrustDomainRevisions,
    ListTrustDomainRevisionsHandler, ListWorkloadIdentityPolicyRevisions,
    ListWorkloadIdentityPolicyRevisionsHandler,
};
pub use domain::repositories::{
    IOidcIdentityRepository, IPlatformRbacRepository, IPrivilegedAuthorizationDecisionRepository,
    IRecipientContactRepository, IRecipientContactVerificationDeliveryRepository,
    IResourceAuthorizationDecisionRepository, ITenantSupportGrantRepository,
    ITrustDomainRepository, IWorkloadIdentityPolicyRepository, IWorkloadRuntimeEvidenceRepository,
};
pub use domain::services::{
    IOidcProviderService, IRecipientContactVerificationDeliveryService,
    IWorkloadIdentityProviderService, OidcAuthorization, OidcAuthorizationRequest,
    OidcCodeVerificationRequest, OidcProviderError, ResourceAuthorizationDecision,
    ResourceAuthorizationDecisionRequest, VerifiedOidcIdentity,
};
pub use infrastructure::persistence::{InMemoryIdentityRepository, PostgresIdentityRepository};
pub use infrastructure::OpenIdConnectProviderService;
pub use infrastructure::{
    A3sEventRecipientContactVerificationConsumer, OwnerWorkloadRuntimeEvidenceAdapter,
    SmtpRecipientContactVerificationCredentials, SmtpRecipientContactVerificationDeliveryOptions,
    SmtpRecipientContactVerificationDeliveryService, SmtpRecipientContactVerificationTlsPolicy,
    SpiffeHttpsWebWorkloadIdentityProviderOptions, SpiffeHttpsWebWorkloadIdentityProviderService,
    RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY,
};
pub(crate) use presentation::{authenticated_credential_actor, AuthenticatedCredentialActor};
pub use presentation::{IdentityModule, OrganizationTenantGuard};
