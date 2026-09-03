pub mod commands;
pub mod queries;

mod encryption;
mod environment_access;
mod materialization;
mod materialization_authorization;
mod plaintext;
mod resource_access;
mod result;

pub use commands::{
    CreateSecret, CreateSecretHandler, RevokeSecretVersion, RevokeSecretVersionHandler,
    RotateSecret, RotateSecretHandler,
};
pub(crate) use encryption::encryption_error;
pub use environment_access::{ISecretEnvironmentAccess, SecretEnvironmentScope};
pub use materialization::exact_secret_version_access;
pub(crate) use materialization::{
    exact_secret_materializer, ExactSecretMaterializer, ExactSecretVersionAccess,
};
pub use materialization::{IExactSecretMaterializer, IExactSecretVersionAccess};
pub use materialization_authorization::{
    ISecretMaterializationAuthorizer, SecretMaterializationAuthorization,
    SecretMaterializationAuthorizationError, SecretMaterializationAuthorizationRequest,
};
pub use plaintext::SecretPlaintext;
pub use queries::{
    GetSecret, GetSecretHandler, ListSecrets, ListSecretsHandler, ResolveSecretMaterial,
    ResolveSecretMaterialHandler,
};
pub use resource_access::SecretAccess;
pub(crate) use resource_access::{SecretAccessScope, SecretResourceResolver};
pub use result::{SecretDetails, SecretMutationResult, SecretVersionResult};
