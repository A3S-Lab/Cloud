#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("invalid workflow: {0}")]
    Validation(String),
    #[error("workflow {0} was not found")]
    NotFound(String),
    #[error("workflow conflict: {0}")]
    Conflict(String),
    #[error("workflow persistence failed: {0}")]
    Persistence(String),
    #[error("workflow execution failed: {0}")]
    Execution(String),
    #[error("external step failed: {0}")]
    External(String),
}

pub type WorkflowResult<T> = std::result::Result<T, WorkflowError>;
