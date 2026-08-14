pub mod commands;
pub mod queries;

mod encryption;
mod materialization;
mod plaintext;
mod resource_access;
mod result;

pub use commands::{
    CreateSecret, CreateSecretHandler, RevokeSecretVersion, RevokeSecretVersionHandler,
    RotateSecret, RotateSecretHandler,
};
pub(crate) use encryption::encryption_error;
pub(crate) use materialization::{ExactSecretMaterializer, ExactSecretVersionAccess};
pub use plaintext::SecretPlaintext;
pub use queries::{
    GetSecret, GetSecretHandler, ListSecrets, ListSecretsHandler, ResolveSecretMaterial,
    ResolveSecretMaterialHandler,
};
pub use result::{SecretDetails, SecretMutationResult, SecretVersionResult};
