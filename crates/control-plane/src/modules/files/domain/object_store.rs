use super::UserFileContentReference;
use async_trait::async_trait;
use std::pin::Pin;
use tokio::io::AsyncRead;

pub type UserFileObjectReader = Pin<Box<dyn AsyncRead + Send + Unpin + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFileObjectWrite {
    reference: UserFileContentReference,
    replayed: bool,
}

impl UserFileObjectWrite {
    pub(in crate::modules::files) fn stored(
        reference: UserFileContentReference,
        replayed: bool,
    ) -> Self {
        Self {
            reference,
            replayed,
        }
    }

    pub const fn reference(&self) -> &UserFileContentReference {
        &self.reference
    }

    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

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
