//! Deliberate public Secrets contracts.
//!
//! Concrete `infrastructure` and `presentation` modules stay crate-private.
//! Selected adapters, repositories, and the module wiring surface are
//! re-exported from the bounded-context root so consumers never depend on an
//! outer-layer module path.

pub use super::infrastructure::{
    InMemorySecretRepository, PostgresSecretRepository, ProjectsSecretEnvironmentAccessAdapter,
    WorkloadsSecretMaterializationAuthorizerAdapter,
};
pub use super::presentation::SecretsModule;
