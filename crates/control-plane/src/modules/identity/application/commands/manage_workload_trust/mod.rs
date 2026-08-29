mod commands;
mod handlers;

pub use commands::{AcceptTrustDomainRevision, AcceptWorkloadIdentityPolicyRevision};
pub use handlers::{AcceptTrustDomainRevisionHandler, AcceptWorkloadIdentityPolicyRevisionHandler};
