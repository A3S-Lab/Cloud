#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormSemanticCoreError {
    #[error("native Form compilation failed: {0}")]
    Compilation(String),
    #[error("native Form submitted-value evaluation failed: {0}")]
    Evaluation(String),
}

/// Application port for the exact, version-pinned A3S Form semantic core.
///
/// Requests and responses retain the owner-defined bounded byte protocols so
/// Cloud cannot introduce an independent compiler or submitted-value validator.
pub trait IFormSemanticCore: Send + Sync {
    fn compiler_revision(&self) -> &'static str;

    fn compile(&self, request: &[u8]) -> Result<Vec<u8>, FormSemanticCoreError>;

    fn evaluate(&self, request: &[u8]) -> Result<Vec<u8>, FormSemanticCoreError>;
}
