use crate::modules::edge::domain::GatewayPublication;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayCommandDispatch {
    pub replayed: bool,
}

#[async_trait]
pub trait IGatewayCommandQueue: Send + Sync {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError>;
}
