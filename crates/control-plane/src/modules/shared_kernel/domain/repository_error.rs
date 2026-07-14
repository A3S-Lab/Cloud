#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryError {
    #[error("resource was not found")]
    NotFound,
    #[error("resource already exists: {0}")]
    Conflict(String),
    #[error("idempotency key was reused with different input")]
    IdempotencyConflict,
    #[error("repository failed: {0}")]
    Storage(String),
}
