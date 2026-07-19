mod entities;
mod events;
mod repositories;
mod services;
mod value_objects;

pub use entities::{Secret, SecretState, SecretVersion, SecretVersionState};
pub use events::SecretChanged;
pub use repositories::{
    CreateSecretWrite, ISecretRepository, RotateSecretWrite, SecretWrite, SecretWriteReference,
    TransitionSecretVersion,
};
pub use services::{ISecretEncryptionService, SecretEncryptionError};
pub use value_objects::{secret_encryption_context, EncryptedSecretValue};

#[cfg(test)]
mod tests;
