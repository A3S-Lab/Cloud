//! Inference bounded context — usage ledger, showback, and Gateway ACL projections.

pub mod application;
pub mod domain;
pub(crate) mod infrastructure;
pub(crate) mod presentation;

mod facade;

pub use application::*;
pub use domain::*;
pub use facade::{
    inference_route_commands_controller, inference_route_queries_controller,
    usage_queries_controller, usage_retention_controller, InMemoryInferenceRouteRepository,
    InMemoryInferenceUsageRepository, InferenceModule, InferenceRouteAclProjectionAdapter,
    PostgresInferenceRouteRepository, PostgresInferenceUsageRepository,
    ProjectsInferenceEnvironmentAccessAdapter,
};
