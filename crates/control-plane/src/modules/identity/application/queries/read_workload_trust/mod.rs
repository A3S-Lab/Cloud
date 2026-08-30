mod handlers;
mod queries;

pub use handlers::{
    GetCurrentTrustDomainHandler, GetCurrentWorkloadIdentityPolicyForWorkloadHandler,
    GetCurrentWorkloadIdentityPolicyHandler, GetTrustDomainRevisionHandler,
    GetWorkloadIdentityPolicyRevisionHandler, InspectCurrentTrustDomainProviderHandler,
    ListTrustDomainRevisionsHandler, ListWorkloadIdentityPolicyRevisionsHandler,
};
pub use queries::{
    GetCurrentTrustDomain, GetCurrentWorkloadIdentityPolicy,
    GetCurrentWorkloadIdentityPolicyForWorkload, GetTrustDomainRevision,
    GetWorkloadIdentityPolicyRevision, InspectCurrentTrustDomainProvider, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions,
};
