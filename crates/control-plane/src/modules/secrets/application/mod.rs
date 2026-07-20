pub mod commands;
pub mod queries;

mod encryption;
mod plaintext;
mod result;

pub use commands::{
    CreateSecret, CreateSecretHandler, RevokeSecretVersion, RevokeSecretVersionHandler,
    RotateSecret, RotateSecretHandler,
};
pub(crate) use encryption::encryption_error;
pub use plaintext::SecretPlaintext;
pub use queries::{
    GetSecret, GetSecretHandler, ListSecrets, ListSecretsHandler, ResolveSecretMaterial,
    ResolveSecretMaterialHandler,
};
pub use result::{SecretDetails, SecretMutationResult, SecretVersionResult};
