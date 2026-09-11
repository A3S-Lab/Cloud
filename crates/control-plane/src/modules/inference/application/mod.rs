pub mod commands;
mod edge_route_binding_admission;
mod empty_inference_route_acl_projection;
mod empty_inference_worker_acl_projection;
mod environment_access;
mod grant_credential_admission;
mod inference_usage_retention_worker;
pub mod queries;
mod resource_access;
mod route_acl_projection;
mod worker_acl_projection;

pub use commands::*;
pub use edge_route_binding_admission::{
    EDGE_ROUTE_BINDING_INVALID, IInferenceEdgeRouteBindingAdmissionPort,
    InferenceEdgeRouteBindingAdmissionRequest, PermitInferenceEdgeRouteBindingAdmission,
};
pub use empty_inference_route_acl_projection::EmptyInferenceRouteAclProjectionPort;
pub use empty_inference_worker_acl_projection::{
    EmptyInferenceWorkerAclProjectionPort, postgres_inference_worker_acl_projections,
};
pub use environment_access::{IInferenceEnvironmentAccess, InferenceEnvironmentScope};
pub use grant_credential_admission::{
    IInferenceGrantCredentialAdmissionPort, INFERENCE_GRANT_CREDENTIAL_INVALID,
    InferenceGrantCredentialAdmissionRequest, PermitInferenceGrantCredentialAdmission,
};
pub use inference_usage_retention_worker::InferenceUsageRetentionWorker;
pub use queries::*;
pub use resource_access::{InferenceAccess, InferenceAccessScope};
pub use route_acl_projection::{IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope};
pub use worker_acl_projection::IInferenceWorkerAclProjectionPort;

#[cfg(test)]
#[path = "inference_route_catalog_tests.rs"]
mod inference_route_catalog_tests;

#[cfg(test)]
#[path = "inference_route_binding_admission_tests.rs"]
mod inference_route_binding_admission_tests;

#[cfg(test)]
#[path = "inference_route_grant_credential_admission_tests.rs"]
mod inference_route_grant_credential_admission_tests;

#[cfg(test)]
#[path = "inference_route_query_tests.rs"]
mod inference_route_query_tests;

#[cfg(test)]
#[path = "empty_inference_worker_acl_projection_tests.rs"]
mod empty_inference_worker_acl_projection_tests;
