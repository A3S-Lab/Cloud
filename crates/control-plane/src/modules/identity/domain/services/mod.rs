mod oidc_provider;
mod privileged_authorization_decision;
mod recipient_contact_proof;
mod recipient_contact_verification_delivery;
mod resource_access_evaluator;
mod resource_authorization_decision;
mod workload_identity_provider;

pub use oidc_provider::{
    IOidcProviderService, OidcAuthorization, OidcAuthorizationRequest, OidcCodeVerificationRequest,
    OidcProviderError, VerifiedOidcIdentity,
};
pub use privileged_authorization_decision::{
    PlatformRoleBindingDecisionEvidence, PlatformRolePolicyDecisionEvidence,
    PrivilegedAuthorizationDecision, PrivilegedAuthorizationDecisionRequest,
    PrivilegedCredentialDecisionEvidence, TenantSupportGrantDecisionEvidence,
};
pub use recipient_contact_proof::{IRecipientContactProofService, RecipientContactProofError};
pub use recipient_contact_verification_delivery::{
    IPreparedRecipientContactVerificationDelivery, IRecipientContactVerificationDeliveryService,
    RecipientContactVerificationDeliveryPreparationError,
    RecipientContactVerificationDeliveryRequest, RecipientContactVerificationProviderOutcome,
};
pub use resource_access_evaluator::ResourceAccessEvaluator;
pub use resource_authorization_decision::{
    ResourceAuthorizationBasis, ResourceAuthorizationCredentialEvidence,
    ResourceAuthorizationDecision, ResourceAuthorizationDecisionRequest,
    ResourceAuthorizationGrantEvidence,
};
pub use workload_identity_provider::{
    IWorkloadIdentityProviderService, WorkloadIdentityProviderCapabilities,
    WorkloadIdentityProviderError,
};
