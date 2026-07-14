use crate::modules::shared_kernel::domain::RepositoryError;

pub type ApplicationResult<T> = Result<T, ApplicationError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Internal(String),
}

impl From<RepositoryError> for ApplicationError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::NotFound => Self::NotFound("resource not found".into()),
            RepositoryError::Conflict(message) => Self::Conflict(message),
            RepositoryError::IdempotencyConflict => {
                Self::Conflict("idempotency key reused with different input".into())
            }
            RepositoryError::Storage(message) => Self::Internal(message),
        }
    }
}
