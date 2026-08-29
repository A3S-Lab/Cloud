mod handlers;
mod queries;

pub use handlers::{
    GetCurrentTrustDomainHandler, GetCurrentWorkloadIdentityPolicyForWorkloadHandler,
    GetCurrentWorkloadIdentityPolicyHandler, GetTrustDomainRevisionHandler,
    GetWorkloadIdentityPolicyRevisionHandler, ListTrustDomainRevisionsHandler,
    ListWorkloadIdentityPolicyRevisionsHandler,
};
pub use queries::{
    GetCurrentTrustDomain, GetCurrentWorkloadIdentityPolicy,
    GetCurrentWorkloadIdentityPolicyForWorkload, GetTrustDomainRevision,
    GetWorkloadIdentityPolicyRevision, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions,
};
