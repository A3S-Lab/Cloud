pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    CreateSecret, CreateSecretHandler, GetSecret, GetSecretHandler, ListSecrets,
    ListSecretsHandler, RevokeSecretVersion, RevokeSecretVersionHandler, RotateSecret,
    RotateSecretHandler, SecretDetails, SecretMutationResult, SecretPlaintext, SecretVersionResult,
};
pub use domain::{
    CreateSecretWrite, EncryptedSecretValue, ISecretEncryptionService, ISecretRepository,
    RotateSecretWrite, Secret, SecretChanged, SecretEncryptionError, SecretState, SecretVersion,
    SecretVersionState, SecretWrite, SecretWriteReference, TransitionSecretVersion,
};
pub use infrastructure::{InMemorySecretRepository, PostgresSecretRepository};
pub use presentation::SecretsModule;
