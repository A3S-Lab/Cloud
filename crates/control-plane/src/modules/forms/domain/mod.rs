#[cfg(test)]
mod contract_tests;

pub mod entities;

pub use entities::{AcceptedFormSubmission, FormSubmission, CLOUD_FORM_SUBMISSION_MAX_VALUE_BYTES};
