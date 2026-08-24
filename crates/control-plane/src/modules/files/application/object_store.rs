use crate::modules::files::domain::{UserFileContentReference, UserFileObjectWrite};
use async_trait::async_trait;
use std::pin::Pin;
use tokio::io::AsyncRead;

/// Application-boundary byte stream accepted by the Files storage port.
///
/// The UserFile domain reasons only about immutable content references and a
/// matching durable-write receipt. Streaming and async-runtime mechanics stay
/// outside the aggregate boundary.
pub type UserFileObjectReader = Pin<Box<dyn AsyncRead + Send + Unpin + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserFileObjectError {
    #[error("UserFile object request is invalid: {0}")]
    Invalid(String),
    #[error("UserFile object conflicts with existing content: {0}")]
    Conflict(String),
    #[error("UserFile object was not found")]
    NotFound,
    #[error("UserFile object failed integrity validation: {0}")]
    Integrity(String),
    #[error("UserFile object storage is unavailable: {0}")]
    Unavailable(String),
}

/// Consumer-owned port for persisting and verifying exact UserFile bytes.
///
/// Implementations adapt the deployment's single immutable-object authority;
/// this interface does not make Files an object-provider owner.
#[async_trait]
pub trait IUserFileObjectStore: Send + Sync {
    async fn put(
        &self,
        reference: &UserFileContentReference,
        reader: UserFileObjectReader,
    ) -> Result<UserFileObjectWrite, UserFileObjectError>;

    async fn verify(&self, reference: &UserFileContentReference)
        -> Result<(), UserFileObjectError>;
}
