//! Deliberate public Inference contracts.
//!
//! Concrete `infrastructure` and `presentation` modules stay crate-private.
//! Selected adapters, repositories, controllers, and the module wiring surface
//! are re-exported from the bounded-context root so consumers never depend on
//! an outer-layer module path.

pub use super::infrastructure::{
    InMemoryInferenceRouteRepository, InMemoryInferenceUsageRepository,
    InferenceRouteAclProjectionAdapter, PostgresInferenceRouteRepository,
    PostgresInferenceUsageRepository, ProjectsInferenceEnvironmentAccessAdapter,
};
pub use super::presentation::{
    inference_route_commands_controller, inference_route_queries_controller,
    usage_queries_controller, usage_retention_controller, InferenceModule,
};
