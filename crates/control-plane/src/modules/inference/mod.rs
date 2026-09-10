//! Inference bounded context — usage ledger, showback, and Gateway ACL projections.

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::*;
pub use domain::*;
pub use infrastructure::*;
pub use presentation::InferenceModule;
