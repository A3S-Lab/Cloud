pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub(crate) use application::SecretAccessScope;
pub use application::{
    CreateSecret, CreateSecretHandler, GetSecret, GetSecretHandler, IExactSecretMaterializer,
    IExactSecretVersionAccess, ISecretEnvironmentAccess, ISecretMaterializationAuthorizer,
    ListSecrets, ListSecretsHandler, ResolveSecretMaterial, ResolveSecretMaterialHandler,
    RevokeSecretVersion, RevokeSecretVersionHandler, RotateSecret, RotateSecretHandler,
    SecretAccess, SecretDetails, SecretEnvironmentScope, SecretMaterializationAuthorization,
    SecretMaterializationAuthorizationError, SecretMaterializationAuthorizationRequest,
    SecretMutationResult, SecretPlaintext, SecretVersionResult,
};
pub use domain::{
    CreateSecretWrite, EncryptedSecretValue, ISecretEncryptionService, ISecretRepository,
    RotateSecretWrite, Secret, SecretChanged, SecretEncryptionError, SecretState, SecretVersion,
    SecretVersionState, SecretWrite, SecretWriteReference, TransitionSecretVersion,
};
pub use infrastructure::{
    InMemorySecretRepository, PostgresSecretRepository, ProjectsSecretEnvironmentAccessAdapter,
    WorkloadsSecretMaterializationAuthorizerAdapter,
};
pub use presentation::SecretsModule;
