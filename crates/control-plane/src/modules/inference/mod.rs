//! Inference bounded context — Gateway usage ledger ingestion and showback reads (I0.2c/I0.2e).

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::*;
pub use domain::*;
pub use infrastructure::*;
pub use presentation::InferenceModule;
