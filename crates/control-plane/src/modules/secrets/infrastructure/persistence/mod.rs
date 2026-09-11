mod in_memory;
mod postgres;

pub use in_memory::InMemorySecretRepository;
pub use postgres::PostgresSecretRepository;
pub(crate) use postgres::{SecretVersionRotationLock, lock_secret_version_for_rotation};
