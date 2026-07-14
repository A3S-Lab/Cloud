use crate::modules::operations::domain::entities::{OperationProjection, OperationRequest};
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum OperationEngineError {
    #[error("operation definition conflicts with an existing durable run: {0}")]
    Conflict(String),
    #[error("operation engine failed: {0}")]
    Unavailable(String),
    #[error("operation history is invalid: {0}")]
    Invalid(String),
}

#[async_trait]
pub trait IOperationEngine: Send + Sync {
    async fn ensure(
        &self,
        request: &OperationRequest,
    ) -> Result<OperationProjection, OperationEngineError>;

    async fn projections(&self) -> Result<Vec<OperationProjection>, OperationEngineError>;
}
