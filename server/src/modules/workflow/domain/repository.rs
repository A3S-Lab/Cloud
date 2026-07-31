use async_trait::async_trait;

use super::{WorkflowDefinition, WorkflowResult};

#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    async fn list(&self) -> WorkflowResult<Vec<WorkflowDefinition>>;
    async fn find(&self, id: &str) -> WorkflowResult<Option<WorkflowDefinition>>;
    async fn create(&self, workflow: &WorkflowDefinition) -> WorkflowResult<()>;
    async fn update(
        &self,
        workflow: &WorkflowDefinition,
        expected_version: u64,
    ) -> WorkflowResult<()>;
    async fn delete(&self, id: &str) -> WorkflowResult<bool>;
}
